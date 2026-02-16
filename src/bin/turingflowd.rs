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
use tracing::{info, warn};
use turingflow::kernel::context::ExecutionContext;
use turingflow::kernel::errors::{KernelError, KernelErrorCode};
use turingflow::kernel::policy::{PolicyConfig, PolicyEngine};
use turingflow::kernel::syscalls::fs::{FsListReq, FsReadReq, FsWriteReq, HostFsProvider};
use turingflow::kernel::syscalls::net::NetHttpReq;
use turingflow::kernel::syscalls::process::ProcExecReq;
use turingflow::kernel::Kernel;
use turingflow::tfpv1::agent_ref::AgentRef;
use turingflow::tfpv1::errors;
use turingflow::tfpv1::mtls::{build_server_config, extract_node_id_from_cert};
use turingflow::tfpv1::router::{
    ClientTlsConfig, DestinationRoute, Router as TfpRouter, RouterError, RouterRetryPolicy,
};
use turingflow::tfpv1::storage::sqlite::initialize_database;
use turingflow::tfpv1::storage::sqlite_ack::SqliteAckStore;
use turingflow::tfpv1::storage::sqlite_dedupe::{within_replay_window, DedupeResult, SqliteDedupe};
use turingflow::tfpv1::storage::sqlite_registry::{RegistryError, SqliteRegistry};
use turingflow::tfpv1::system_config::{DaemonConfig, KingdomQuotas, KingdomsConfig};
use turingflow::tfpv1::types::{
    AckRequest, HeartbeatRequest, Meta, RegisterRequest, SendRequest, SendResponse, TFPV1_VERSION,
};

#[derive(Clone)]
struct AppState {
    registry: Arc<RwLock<SqliteRegistry>>,
    router: Arc<RwLock<TfpRouter>>,
    dedupe: Arc<RwLock<SqliteDedupe>>,
    replay_window_seconds: i64,
    max_payload_bytes: usize,
    max_message_ttl_ms: u64,
    kingdoms: Arc<KingdomsConfig>,
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
    #[arg(long = "config", default_value = "config/turingflowd.yaml")]
    config: PathBuf,
    #[arg(long = "kingdoms-config", default_value = "config/kingdoms.yaml")]
    kingdoms_config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args = Args::parse();
    let daemon_config = DaemonConfig::from_file(&args.config)?;
    let kingdoms_config = KingdomsConfig::from_file(&args.kingdoms_config)?;

    let sqlite_path = daemon_config.storage.sqlite.path.clone();
    initialize_database(&sqlite_path)?;

    let max_level = daemon_config.logging.level();
    let log_builder = tracing_subscriber::fmt().with_max_level(max_level);
    if daemon_config.logging.format == "json" {
        log_builder.json().init();
    } else {
        log_builder.init();
    }

    let addr: SocketAddr = daemon_config.listen_addr()?;

    let tls_config = build_server_config(
        &daemon_config.tls.server_cert,
        &daemon_config.tls.server_key,
        &daemon_config.tls.client_ca_cert,
    )?;
    let tls_acceptor = TlsAcceptor::from(tls_config);

    let router = TfpRouter::new_with_policy_and_ack_store(
        daemon_config.server.node_id.clone(),
        ClientTlsConfig {
            ca_cert_path: daemon_config.tls.upstream_ca_cert.clone(),
            client_cert_path: daemon_config.tls.upstream_client_cert.clone(),
            client_key_path: daemon_config.tls.upstream_client_key.clone(),
        },
        RouterRetryPolicy {
            retry_delays_ms: daemon_config.routing.retry_delays_ms.clone(),
        },
        SqliteAckStore::new(sqlite_path.clone()),
    )?;

    let state = AppState {
        registry: Arc::new(RwLock::new(SqliteRegistry::new(sqlite_path.clone()))),
        router: Arc::new(RwLock::new(router)),
        dedupe: Arc::new(RwLock::new(SqliteDedupe::new(sqlite_path))),
        replay_window_seconds: daemon_config.security.replay_window_seconds,
        max_payload_bytes: daemon_config.limits.max_payload_bytes,
        max_message_ttl_ms: daemon_config.limits.max_message_ttl_ms,
        kingdoms: Arc::new(kingdoms_config),
        metrics: Arc::new(Metrics::default()),
    };

    let gc_state = state.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(5)).await;
            let mut registry = gc_state.registry.write().await;
            if let Err(error) = registry.cleanup_expired_now() {
                warn!(error = ?error, "registry_cleanup_failed");
            }
            drop(registry);
            let mut dedupe = gc_state.dedupe.write().await;
            if let Err(error) = dedupe.cleanup_expired_now() {
                warn!(error = ?error, "dedupe_cleanup_failed");
            }
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

    let quotas = match kingdom_quotas(&state, &request.kingdom_id) {
        Ok(quotas) => quotas,
        Err(error) => return error,
    };

    if request.agents.len() > quotas.max_agents_per_node {
        return invalid_payload(format!(
            "agents count exceeds max_agents_per_node ({})",
            quotas.max_agents_per_node
        ));
    }
    if request.lease_ttl_ms > quotas.max_lease_ttl_ms {
        return invalid_payload(format!(
            "lease_ttl_ms exceeds max_lease_ttl_ms ({})",
            quotas.max_lease_ttl_ms
        ));
    }

    if request.node.node_id.to_ascii_lowercase() != identity.node_id {
        return identity_mismatch();
    }

    let mut registry = state.registry.write().await;
    let existing_count =
        match registry.count_agents_for_node(&request.kingdom_id, &request.node.node_id) {
            Ok(count) => count,
            Err(RegistryError::Storage(message)) => return internal_error(message),
            Err(RegistryError::Invalid(message)) => return invalid_payload(message.to_string()),
            Err(RegistryError::IdentityMismatch) => return identity_mismatch(),
            Err(RegistryError::LeaseExpired) => return lease_expired(),
        };

    let requested_agent_refs = request
        .agents
        .iter()
        .map(|agent| agent.agent_ref.clone())
        .collect::<Vec<_>>();
    let additional_agents = match registry.additional_agents_for_node_registration(
        &request.kingdom_id,
        &request.node.node_id,
        &requested_agent_refs,
    ) {
        Ok(count) => count,
        Err(RegistryError::Storage(message)) => return internal_error(message),
        Err(RegistryError::Invalid(message)) => return invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => return identity_mismatch(),
        Err(RegistryError::LeaseExpired) => return lease_expired(),
    };

    if existing_count.saturating_add(additional_agents) > quotas.max_agents_per_node {
        return invalid_payload(format!(
            "register would exceed max_agents_per_node ({})",
            quotas.max_agents_per_node
        ));
    }

    match registry.register(request) {
        Ok(response) => ok_json(StatusCode::OK, response),
        Err(RegistryError::Invalid(message)) => invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => identity_mismatch(),
        Err(RegistryError::LeaseExpired) => lease_expired(),
        Err(RegistryError::Storage(message)) => internal_error(message),
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

    if !state.kingdoms.is_allowed(&request.kingdom_id) {
        return kingdom_not_allowed();
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
        Err(RegistryError::Storage(message)) => internal_error(message),
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

    if !state.kingdoms.is_allowed(&query.kingdom_id) {
        return kingdom_not_allowed();
    }

    let mut registry = state.registry.write().await;
    match registry.resolve(&query.kingdom_id, &parsed.normalized()) {
        Ok(response) => ok_json(StatusCode::OK, response),
        Err(RegistryError::Storage(message)) => internal_error(message),
        Err(RegistryError::Invalid(message)) => invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => identity_mismatch(),
        Err(RegistryError::LeaseExpired) => lease_expired(),
    }
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

    let payload_bytes = payload_size_bytes(&payload);

    if payload_bytes > state.max_payload_bytes {
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
        return payload_too_large(state.max_payload_bytes);
    }

    let request: SendRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(error) => return invalid_payload(format!("invalid send request JSON: {error}")),
    };

    let quotas = match kingdom_quotas(&state, &request.kingdom_id) {
        Ok(quotas) => quotas,
        Err(error) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return error;
        }
    };

    let max_payload_bytes = state.max_payload_bytes.min(quotas.max_payload_bytes);
    if payload_bytes > max_payload_bytes {
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
        return payload_too_large(max_payload_bytes);
    }

    if let Err(error) = request.validate() {
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
        return invalid_payload(error.to_string());
    }

    let max_message_ttl_ms = state.max_message_ttl_ms.min(quotas.max_message_ttl_ms);
    if request.message.ttl_ms > max_message_ttl_ms {
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
        return invalid_payload(format!(
            "message.ttl_ms exceeds configured limit ({max_message_ttl_ms})"
        ));
    }

    state.metrics.messages_in.fetch_add(1, Ordering::Relaxed);

    let now = OffsetDateTime::now_utc();
    let message_timestamp = match OffsetDateTime::parse(&request.message.timestamp, &Rfc3339) {
        Ok(ts) => ts,
        Err(_) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
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
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
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
    match dedupe.check_and_insert(&message_id, dedupe_expiry) {
        Ok(DedupeResult::Duplicate) => {
            state.metrics.dedupe_hits.fetch_add(1, Ordering::Relaxed);
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_duplicate_rejected"
            );
            return duplicate_message();
        }
        Ok(DedupeResult::Inserted) => {}
        Err(error) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return internal_error(error.to_string());
        }
    }
    drop(dedupe);

    let mut registry = state.registry.write().await;
    let source = match registry.lookup_agent(&request.kingdom_id, &from_ref) {
        Ok(Some(source)) => source,
        Ok(None) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_source_not_registered"
            );
            return identity_mismatch();
        }
        Err(RegistryError::Storage(message)) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return internal_error(message);
        }
        Err(RegistryError::Invalid(message)) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return invalid_payload(message.to_string());
        }
        Err(RegistryError::IdentityMismatch) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return identity_mismatch();
        }
        Err(RegistryError::LeaseExpired) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return lease_expired();
        }
    };

    if source.node_id.to_ascii_lowercase() != identity.node_id {
        state
            .metrics
            .messages_failed
            .fetch_add(1, Ordering::Relaxed);
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
        Ok(Some(destination)) => destination,
        Ok(None) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                message_id = %message_id,
                trace_id = %trace_id,
                from_ref = %from_ref,
                to_ref = %to_ref,
                "message_destination_not_found"
            );
            return agent_not_found();
        }
        Err(RegistryError::Storage(message)) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return internal_error(message);
        }
        Err(RegistryError::Invalid(message)) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return invalid_payload(message.to_string());
        }
        Err(RegistryError::IdentityMismatch) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return identity_mismatch();
        }
        Err(RegistryError::LeaseExpired) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            return lease_expired();
        }
    };
    let destination_is_local = destination.node_id.eq_ignore_ascii_case(&source.node_id);
    drop(registry);

    let execution_ctx = build_send_execution_context(&request);
    if destination_is_local {
        match try_execute_local_agent_operation(&execution_ctx, &request.message, &to_ref) {
            Ok(Some(response)) => {
                info!(
                    message_id = %message_id,
                    trace_id = %trace_id,
                    from_ref = %from_ref,
                    to_ref = %to_ref,
                    tool_id = ?execution_ctx.tool_id,
                    "message_executed_locally"
                );
                return ok_json(StatusCode::OK, response);
            }
            Ok(None) => {}
            Err(error) => {
                state
                    .metrics
                    .messages_failed
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    message_id = %message_id,
                    trace_id = %trace_id,
                    from_ref = %from_ref,
                    to_ref = %to_ref,
                    error = %error,
                    "message_local_execution_failed"
                );
                return kernel_error_response(error);
            }
        }
    }

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
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
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
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
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
        Err(RouterError::AckStoreError(message)) => {
            state
                .metrics
                .messages_failed
                .fetch_add(1, Ordering::Relaxed);
            internal_error(message)
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
        Ok(Some(source)) => source,
        Ok(None) => return identity_mismatch(),
        Err(RegistryError::Storage(message)) => return internal_error(message),
        Err(RegistryError::Invalid(message)) => return invalid_payload(message.to_string()),
        Err(RegistryError::IdentityMismatch) => return identity_mismatch(),
        Err(RegistryError::LeaseExpired) => return lease_expired(),
    };
    drop(registry);

    if source.node_id.to_ascii_lowercase() != identity.node_id {
        return identity_mismatch();
    }

    if !state.kingdoms.is_allowed(&source.kingdom_id) {
        return kingdom_not_allowed();
    }

    let ack_ctx = build_ack_execution_context(&request);
    info!(
        trace_id = %ack_ctx.trace_id,
        from_ref = %ack_ctx.agent_ref,
        tool_id = ?ack_ctx.tool_id,
        "ack_execution_context_built"
    );

    let message_id = request.message_id.clone();
    let from_ref = request.from_ref.clone();
    let mut router = state.router.write().await;
    let response = match router.record_ack(request) {
        Ok(response) => response,
        Err(RouterError::AckStoreError(message)) => return internal_error(message),
        Err(RouterError::DeliveryTimeout) => return delivery_timeout(),
        Err(RouterError::DestinationUnreachable { status, details }) => {
            return destination_unreachable(status, details);
        }
    };
    info!(message_id = %message_id, from_ref = %from_ref, "ack_recorded");
    ok_json(StatusCode::OK, response)
}

fn build_send_execution_context(request: &SendRequest) -> ExecutionContext {
    ExecutionContext {
        trace_id: request.message.trace_id.clone(),
        kingdom_id: request.kingdom_id.clone(),
        agent_ref: request.message.from_ref.clone(),
        tool_id: extract_tool_id_from_meta(&request.message.meta),
    }
}

fn build_ack_execution_context(request: &AckRequest) -> ExecutionContext {
    let trace_id = request
        .result
        .as_ref()
        .and_then(|value| value.get("trace_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("ack:{}", request.message_id));

    let tool_id = request
        .result
        .as_ref()
        .and_then(|value| value.get("tool_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    ExecutionContext {
        trace_id,
        kingdom_id: "unknown".to_string(),
        agent_ref: request.from_ref.clone(),
        tool_id,
    }
}

fn extract_tool_id_from_meta(meta: &Option<Meta>) -> Option<String> {
    meta.as_ref()
        .and_then(|meta| meta.tags.as_ref())
        .and_then(|tags| {
            tags.iter()
                .find_map(|tag| tag.strip_prefix("tool:").map(ToString::to_string))
        })
}

fn try_execute_local_agent_operation(
    ctx: &ExecutionContext,
    message: &turingflow::tfpv1::types::Envelope,
    destination_ref: &str,
) -> Result<Option<SendResponse>, KernelError> {
    if message.payload.content_type != "application/vnd.turingflow.agent-op+json" {
        return Ok(None);
    }

    let operation = message
        .payload
        .body
        .as_object()
        .ok_or_else(|| KernelError::invalid("agent-op payload body must be an object"))?;

    let op = operation
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| KernelError::invalid("agent-op payload requires string field 'op'"))?;

    let kernel = build_local_agent_kernel(&ctx.agent_ref)?;
    match op {
        "fs.read" => {
            let path = operation
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("fs.read requires string field 'path'"))?;
            let _ = kernel.fs_read(
                ctx,
                FsReadReq {
                    path: path.to_string(),
                },
            )?;
        }
        "fs.list" => {
            let path = operation
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("fs.list requires string field 'path'"))?;
            let _ = kernel.fs_list(
                ctx,
                FsListReq {
                    path: path.to_string(),
                },
            )?;
        }
        "fs.write" => {
            let path = operation
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("fs.write requires string field 'path'"))?;
            let content = operation
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("fs.write requires string field 'content'"))?;
            let _ = kernel.fs_write(
                ctx,
                FsWriteReq {
                    path: path.to_string(),
                    content: content.as_bytes().to_vec(),
                },
            )?;
        }
        "proc.exec" => {
            let command = operation
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("proc.exec requires string field 'command'"))?;
            let args = operation
                .get("args")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let _ = kernel.proc_exec(
                ctx,
                ProcExecReq {
                    command: command.to_string(),
                    args,
                },
            )?;
        }
        "net.http" => {
            let method = operation
                .get("method")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("net.http requires string field 'method'"))?;
            let url = operation
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| KernelError::invalid("net.http requires string field 'url'"))?;
            let timeout_ms = operation.get("timeout_ms").and_then(Value::as_u64);
            let body = operation
                .get("body")
                .and_then(Value::as_str)
                .map(|body| body.as_bytes().to_vec());
            let _ = kernel.net_http(
                ctx,
                NetHttpReq {
                    method: method.to_string(),
                    url: url.to_string(),
                    body,
                    timeout_ms,
                },
            )?;
        }
        _ => {
            return Err(KernelError::invalid(format!(
                "unsupported agent operation '{}'",
                op
            )));
        }
    }

    Ok(Some(SendResponse {
        version: TFPV1_VERSION.to_string(),
        accepted: true,
        delivery_id: format!("local_{}_{}", now_epoch_millis(), message.message_id),
        status: "executed_locally".to_string(),
        destination: destination_ref.to_string(),
    }))
}

fn build_local_agent_kernel(agent_ref: &str) -> Result<Kernel, KernelError> {
    let root = std::env::current_dir()
        .map_err(|error| KernelError::internal(format!("failed to resolve cwd: {error}")))?;
    let root = std::fs::canonicalize(root)
        .map_err(|error| KernelError::internal(format!("failed to canonicalize cwd: {error}")))?;

    let policy_yaml = format!(
        "version: 1
defaults:
  decision: deny
principals:
  - id: \"agent:{}\"
    rules:
      - id: \"allow-fs-read\"
        effect: allow
        syscall: \"fs.read\"
        resource:
          path_prefix:
            - \"{}\"
      - id: \"allow-fs-list\"
        effect: allow
        syscall: \"fs.list\"
        resource:
          path_prefix:
            - \"{}\"
      - id: \"allow-fs-write\"
        effect: allow
        syscall: \"fs.write\"
        resource:
          path_prefix:
            - \"{}\"
",
        agent_ref,
        root.display(),
        root.display(),
        root.display()
    );

    let config: PolicyConfig = serde_yaml::from_str(&policy_yaml)
        .map_err(|error| KernelError::internal(format!("failed to parse policy: {error}")))?;
    config
        .validate()
        .map_err(|error| KernelError::internal(format!("invalid policy: {error}")))?;

    let fs_provider = Arc::new(HostFsProvider::new(&root)?);
    Ok(Kernel::new(PolicyEngine::new(config), fs_provider))
}

fn now_epoch_millis() -> i64 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
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

fn kingdom_not_allowed() -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::FORBIDDEN,
        "kingdom_not_allowed",
        "kingdom_id is not allowed by configuration",
        false,
    ))
}

fn payload_too_large(max_payload_bytes: usize) -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
        format!("payload exceeds max_payload_bytes ({max_payload_bytes})"),
        false,
    ))
}

fn kernel_error_response(error: KernelError) -> (StatusCode, Json<Value>) {
    let (status, code): (StatusCode, &'static str) = match error.code {
        KernelErrorCode::Eacces => (StatusCode::FORBIDDEN, "EACCES"),
        KernelErrorCode::Enoent => (StatusCode::NOT_FOUND, "ENOENT"),
        KernelErrorCode::Einval => (StatusCode::BAD_REQUEST, "EINVAL"),
        KernelErrorCode::Etimeout => (StatusCode::GATEWAY_TIMEOUT, "ETIMEOUT"),
        KernelErrorCode::Eratelimit => (StatusCode::TOO_MANY_REQUESTS, "ERATELIMIT"),
        KernelErrorCode::Einternal => (StatusCode::INTERNAL_SERVER_ERROR, "EINTERNAL"),
    };
    to_value_response(errors::response(
        status,
        code,
        error.message,
        error.retryable,
    ))
}

fn internal_error(message: String) -> (StatusCode, Json<Value>) {
    to_value_response(errors::response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        message,
        true,
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

fn destination_unreachable(
    status: Option<u16>,
    details: Option<String>,
) -> (StatusCode, Json<Value>) {
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
    (
        status,
        Json(serde_json::to_value(payload).unwrap_or(json!({}))),
    )
}

fn payload_size_bytes(payload: &Value) -> usize {
    serde_json::to_vec(payload).map_or(usize::MAX, |bytes| bytes.len())
}

fn kingdom_quotas<'a>(
    state: &'a AppState,
    kingdom_id: &str,
) -> Result<&'a KingdomQuotas, (StatusCode, Json<Value>)> {
    state
        .kingdoms
        .quotas_for(kingdom_id)
        .ok_or_else(kingdom_not_allowed)
}
