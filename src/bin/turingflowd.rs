use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use clap::Parser;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperBuilder;
use hyper_util::service::TowerToHyperService;
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tokio_rustls::TlsAcceptor;
use turingflow::tfpv1::agent_ref::AgentRef;
use turingflow::tfpv1::dedupe::{within_replay_window, DedupeCache, DedupeResult};
use turingflow::tfpv1::errors;
use turingflow::tfpv1::mtls::{build_server_config, extract_node_id_from_cert};
use turingflow::tfpv1::registry::{Registry, RegistryError};
use turingflow::tfpv1::router::{ClientTlsConfig, DestinationRoute, Router as TfpRouter, RouterError};
use turingflow::tfpv1::types::{AckRequest, HeartbeatRequest, RegisterRequest, SendRequest};
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    registry: Arc<RwLock<Registry>>,
    router: Arc<RwLock<TfpRouter>>,
    dedupe: Arc<RwLock<DedupeCache>>,
    replay_window_seconds: i64,
    metrics: Arc<Metrics>,
}

#[derive(Debug, Default)]
struct Metrics {
    messages_in: AtomicU64,
    messages_forwarded: AtomicU64,
    messages_failed: AtomicU64,
    dedupe_hits: AtomicU64,
}

#[derive(Debug, serde::Serialize)]
struct MetricsSnapshot {
    messages_in: u64,
    messages_forwarded: u64,
    messages_failed: u64,
    dedupe_hits: u64,
}

impl Metrics {
    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_in: self.messages_in.load(Ordering::Relaxed),
            messages_forwarded: self.messages_forwarded.load(Ordering::Relaxed),
            messages_failed: self.messages_failed.load(Ordering::Relaxed),
            dedupe_hits: self.dedupe_hits.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct ClientIdentity {
    node_id: String,
}

#[derive(Debug, Deserialize)]
struct ResolveQuery {
    kingdom_id: String,
}

#[derive(Debug, Parser)]
#[command(name = "turingflowd", version, about = "TuringFlow daemon")]
struct Args {
    #[arg(long = "listen", default_value = "0.0.0.0:8443")]
    listen: String,
    #[arg(long = "tls-cert")]
    tls_cert: PathBuf,
    #[arg(long = "tls-key")]
    tls_key: PathBuf,
    #[arg(long = "client-ca-cert")]
    client_ca_cert: PathBuf,
    #[arg(long = "node-id", default_value = "turingflowd")]
    node_id: String,
    #[arg(long = "upstream-ca-cert")]
    upstream_ca_cert: Option<PathBuf>,
    #[arg(long = "upstream-client-cert")]
    upstream_client_cert: Option<PathBuf>,
    #[arg(long = "upstream-client-key")]
    upstream_client_key: Option<PathBuf>,
    #[arg(long = "replay-window-seconds", default_value_t = 60)]
    replay_window_seconds: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().json().with_current_span(false).init();

    let args = Args::parse();
    let addr: SocketAddr = args.listen.parse()?;

    let tls_config = build_server_config(
        &args.tls_cert.to_string_lossy(),
        &args.tls_key.to_string_lossy(),
        &args.client_ca_cert.to_string_lossy(),
    )?;
    let tls_acceptor = TlsAcceptor::from(tls_config);

    let router = TfpRouter::new(
        args.node_id.clone(),
        ClientTlsConfig {
            ca_cert_path: args
                .upstream_ca_cert
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            client_cert_path: args
                .upstream_client_cert
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            client_key_path: args
                .upstream_client_key
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        },
    )?;

    let state = AppState {
        registry: Arc::new(RwLock::new(Registry::new())),
        router: Arc::new(RwLock::new(router)),
        dedupe: Arc::new(RwLock::new(DedupeCache::new())),
        replay_window_seconds: args.replay_window_seconds,
        metrics: Arc::new(Metrics::default()),
    };

    let gc_state = state.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(5)).await;
            let mut registry = gc_state.registry.write().await;
            registry.cleanup_expired_now();
            drop(registry);
            let mut dedupe = gc_state.dedupe.write().await;
            dedupe.cleanup_expired_now();
        }
    });

    let app = Router::new()
        .nest(
            "/tfpv1",
            Router::new()
                .route("/health", get(health))
                .route("/agents/register", post(register_agent))
                .route("/agents/heartbeat", post(heartbeat_agent))
                .route("/agents/resolve/{agent_ref}", get(resolve_agent))
                .route("/messages/send", post(send_message))
                .route("/messages/ack", post(ack_message)),
        )
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    info!(listen = %addr, "turingflowd_listening");

    loop {
        let (tcp_stream, _remote_addr) = listener.accept().await?;
        let tls_acceptor = tls_acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            if let Err(error) = handle_connection(tls_acceptor, tcp_stream, app).await {
                warn!(error = %error, "connection_error");
            }
        });
    }
}

async fn handle_connection(
    tls_acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    app: Router,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let tls_stream = tls_acceptor.accept(tcp_stream).await?;
    let identity = extract_client_identity(&tls_stream)
        .ok_or_else(|| "unable to extract client certificate identity".to_string())?;

    let io = TokioIo::new(tls_stream);
    let service = app.layer(Extension(identity));
    let hyper_service = TowerToHyperService::new(service);

    HyperBuilder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(io, hyper_service)
        .await?;

    Ok(())
}

fn extract_client_identity<I>(
    tls_stream: &tokio_rustls::server::TlsStream<I>,
) -> Option<ClientIdentity> {
    let (_, connection) = tls_stream.get_ref();
    let cert = connection.peer_certificates()?.first()?;
    let node_id = extract_node_id_from_cert(cert.as_ref())?;
    Some(ClientIdentity { node_id })
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "version": "TFPv1",
        "service": "turingflowd",
        "status": "ok",
        "metrics": state.metrics.snapshot()
    }))
}

async fn register_agent(
    State(state): State<AppState>,
    identity: Option<Extension<ClientIdentity>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let identity = match require_client_identity(identity) {
        Ok(identity) => identity,
        Err(error) => return error,
    };

    let request: RegisterRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(error) => return invalid_payload(format!("invalid register request JSON: {error}")),
    };

    if let Err(error) = request.validate() {
        return invalid_payload(error.to_string());
    }

    if request.node.node_id.to_ascii_lowercase() != identity.node_id {
        return identity_mismatch();
    }

    let mut registry = state.registry.write().await;
    match registry.register(request) {
        Ok(response) => ok_json(StatusCode::OK, response),
        Err(RegistryError::Invalid(message)) => invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => identity_mismatch(),
        Err(RegistryError::LeaseExpired) => lease_expired(),
    }
}

async fn heartbeat_agent(
    State(state): State<AppState>,
    identity: Option<Extension<ClientIdentity>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let identity = match require_client_identity(identity) {
        Ok(identity) => identity,
        Err(error) => return error,
    };

    let request: HeartbeatRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(error) => return invalid_payload(format!("invalid heartbeat request JSON: {error}")),
    };

    if let Err(error) = request.validate() {
        return invalid_payload(error.to_string());
    }

    if request.node_id.to_ascii_lowercase() != identity.node_id {
        return identity_mismatch();
    }

    let mut registry = state.registry.write().await;
    match registry.heartbeat(request) {
        Ok(response) => ok_json(StatusCode::OK, response),
        Err(RegistryError::Invalid(message)) => invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => identity_mismatch(),
        Err(RegistryError::LeaseExpired) => lease_expired(),
    }
}

async fn resolve_agent(
    State(state): State<AppState>,
    identity: Option<Extension<ClientIdentity>>,
    Path(agent_ref): Path<String>,
    Query(query): Query<ResolveQuery>,
) -> (StatusCode, Json<Value>) {
    if let Err(error) = require_client_identity(identity) {
        return error;
    }

    let parsed = match AgentRef::parse(&agent_ref) {
        Ok(parsed) => parsed,
        Err(error) => return invalid_payload(format!("invalid agent_ref: {error}")),
    };

    let mut registry = state.registry.write().await;
    let response = registry.resolve(&query.kingdom_id, &parsed.normalized());
    ok_json(StatusCode::OK, response)
}

async fn send_message(
    State(state): State<AppState>,
    identity: Option<Extension<ClientIdentity>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let identity = match require_client_identity(identity) {
        Ok(identity) => identity,
        Err(error) => return error,
    };

    let request: SendRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(error) => return invalid_payload(format!("invalid send request JSON: {error}")),
    };

    if let Err(error) = request.validate() {
        state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
        return invalid_payload(error.to_string());
    }

    state.metrics.messages_in.fetch_add(1, Ordering::Relaxed);

    let now = OffsetDateTime::now_utc();
    let message_timestamp = match OffsetDateTime::parse(&request.message.timestamp, &Rfc3339) {
        Ok(ts) => ts,
        Err(_) => {
            state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
            return invalid_payload("message.timestamp must be RFC3339".to_string());
        }
    };

    let message_id = request.message.message_id.clone();
    let trace_id = request.message.trace_id.clone();
    let from_ref = request.message.from_ref.clone();
    let to_ref = request.message.to_ref.clone();

    info!(
        message_id = %message_id,
        trace_id = %trace_id,
        from_ref = %from_ref,
        to_ref = %to_ref,
        "message_received"
    );

    if !within_replay_window(message_timestamp, now, state.replay_window_seconds) {
        state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
        warn!(
            message_id = %message_id,
            trace_id = %trace_id,
            from_ref = %from_ref,
            to_ref = %to_ref,
            "message_replay_rejected"
        );
        return replay_rejected();
    }

    let ttl_ms = request.message.ttl_ms;
    let dedupe_ttl_ms = ttl_ms.max((state.replay_window_seconds.max(1) as u64) * 1000);
    let dedupe_expiry = now + TimeDuration::milliseconds(dedupe_ttl_ms.min(i64::MAX as u64) as i64);
    let mut dedupe = state.dedupe.write().await;
    if dedupe.check_and_insert(&message_id, dedupe_expiry) == DedupeResult::Duplicate {
        state.metrics.dedupe_hits.fetch_add(1, Ordering::Relaxed);
        state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
        warn!(
            message_id = %message_id,
            trace_id = %trace_id,
            from_ref = %from_ref,
            to_ref = %to_ref,
            "message_duplicate_rejected"
        );
        return duplicate_message();
    }
    drop(dedupe);

    let mut registry = state.registry.write().await;
    let source = match registry.lookup_agent(&request.kingdom_id, &from_ref) {
        Some(source) => source,
        None => {
            state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_source_not_registered"
            );
            return identity_mismatch();
        }
    };

    if source.node_id.to_ascii_lowercase() != identity.node_id {
        state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
        warn!(
            message_id = %message_id,
            trace_id = %trace_id,
            from_ref = %from_ref,
            to_ref = %to_ref,
            "message_identity_mismatch"
        );
        return identity_mismatch();
    }

    let destination = match registry.lookup_agent(&request.kingdom_id, &to_ref) {
        Some(destination) => destination,
        None => {
            state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_destination_not_found"
            );
            return agent_not_found();
        }
    };
    drop(registry);

    let route = DestinationRoute {
        agent_ref: destination.agent_ref,
        deliver_url: destination.deliver_url,
    };

    let mut router = state.router.write().await;
    match router.forward_message(request.message, &route).await {
        Ok(response) => {
            state
                .metrics
                .messages_forwarded
                .fetch_add(1, Ordering::Relaxed);
            info!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_forwarded"
            );
            ok_json(StatusCode::OK, response)
        }
        Err(RouterError::DeliveryTimeout) => {
            state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_delivery_timeout"
            );
            delivery_timeout()
        }
        Err(RouterError::DestinationUnreachable { status, details }) => {
            state.metrics.messages_failed.fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                status = ?status,
                "message_destination_unreachable"
            );
            destination_unreachable(status, details)
        }
    }
}

async fn ack_message(
    State(state): State<AppState>,
    identity: Option<Extension<ClientIdentity>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let identity = match require_client_identity(identity) {
        Ok(identity) => identity,
        Err(error) => return error,
    };

    let request: AckRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(error) => return invalid_payload(format!("invalid ack request JSON: {error}")),
    };

    if let Err(error) = request.validate() {
        return invalid_payload(error.to_string());
    }

    let mut registry = state.registry.write().await;
    let source = match registry.lookup_agent_any(&request.from_ref) {
        Some(source) => source,
        None => return identity_mismatch(),
    };
    drop(registry);

    if source.node_id.to_ascii_lowercase() != identity.node_id {
        return identity_mismatch();
    }

    let message_id = request.message_id.clone();
    let from_ref = request.from_ref.clone();
    let mut router = state.router.write().await;
    let response = router.record_ack(request);
    info!(message_id = %message_id, from_ref = %from_ref, "ack_recorded");
    ok_json(StatusCode::OK, response)
}

fn require_client_identity(
    identity: Option<Extension<ClientIdentity>>,
) -> Result<ClientIdentity, (StatusCode, Json<Value>)> {
    match identity {
        Some(Extension(identity)) => Ok(identity),
        None => Err(mtls_required()),
    }
}

fn mtls_required() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::UNAUTHORIZED,
        "mtls_required",
        "Client certificate is required",
        false,
    ))
}

fn duplicate_message() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::CONFLICT,
        "duplicate_message",
        "message_id already processed recently",
        false,
    ))
}

fn replay_rejected() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::FORBIDDEN,
        "replay_rejected",
        "message timestamp is outside allowed replay window",
        false,
    ))
}

fn agent_not_found() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::NOT_FOUND,
        "agent_not_found",
        "Destination agent is not registered",
        false,
    ))
}

fn invalid_payload(message: String) -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::BAD_REQUEST,
        "invalid_payload",
        message,
        false,
    ))
}

fn lease_expired() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::GONE,
        "lease_expired",
        "Lease is expired or unknown",
        false,
    ))
}

fn identity_mismatch() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::FORBIDDEN,
        "identity_mismatch",
        "Client certificate identity does not match node identity",
        false,
    ))
}

fn destination_unreachable(status: Option<u16>, details: Option<String>) -> (StatusCode, Json<Value>) {
    let details = json!({
        "status": status,
        "details": details
    });
    to_value_response(errors::response_with_details(
        StatusCode::SERVICE_UNAVAILABLE,
        "destination_unreachable",
        "Failed to forward message to destination",
        true,
        details,
    ))
}

fn delivery_timeout() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::GATEWAY_TIMEOUT,
        "delivery_timeout",
        "Message TTL exceeded before delivery succeeded",
        true,
    ))
}

fn to_value_response(
    response: (StatusCode, Json<errors::ErrorResponse>),
) -> (StatusCode, Json<Value>) {
    let (status, Json(body)) = response;
    ok_json(status, body)
}

fn ok_json<T: serde::Serialize>(status: StatusCode, payload: T) -> (StatusCode, Json<Value>) {
    (status, Json(serde_json::to_value(payload).unwrap_or(json!({}))))
}
