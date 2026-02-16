use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use turingflow::tfpv1::router::{
    ClientTlsConfig, DestinationRoute, Router as TfpRouter, RouterError, RouterRetryPolicy,
};
use turingflow::tfpv1::storage::sqlite::initialize_database;
use turingflow::tfpv1::storage::sqlite_ack::SqliteAckStore;
use turingflow::tfpv1::storage::sqlite_dedupe::{DedupeResult, SqliteDedupe};
use turingflow::tfpv1::storage::sqlite_registry::SqliteRegistry;
use turingflow::tfpv1::types::{
    AckRequest, AckStatus, AgentRegistration, Envelope, MessageKind, NodeRegistration, Payload,
    RegisterRequest, Routing, TFPV1_VERSION,
};

#[tokio::test]
async fn register_resolve_send_flow_works_with_sqlite_backend() {
    let delivered_count = Arc::new(AtomicUsize::new(0));
    let (deliver_url, server_task) = spawn_mock_agent(delivered_count.clone()).await;

    let db_path = temp_db_path("flow");
    initialize_database(&db_path).expect("db init");
    let db_path_string = db_path.to_string_lossy().to_string();

    let mut registry = SqliteRegistry::new(db_path_string.clone());
    registry
        .register(RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-a".to_string(),
                hostname: "node-a.local".to_string(),
                deliver_url: "http://127.0.0.1:9998".to_string(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "planner@node-a.local".to_string(),
                agent_id: "ag_01A".to_string(),
            }],
            lease_ttl_ms: 10_000,
        })
        .expect("register A");

    registry
        .register(RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-b".to_string(),
                hostname: "node-b.local".to_string(),
                deliver_url: deliver_url.clone(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "executor@node-b.local".to_string(),
                agent_id: "ag_01B".to_string(),
            }],
            lease_ttl_ms: 10_000,
        })
        .expect("register B");

    let resolved = registry
        .resolve("kingdom-main", "executor@node-b.local")
        .expect("resolve");
    assert!(resolved.found);

    let destination = registry
        .lookup_agent("kingdom-main", "executor@node-b.local")
        .expect("lookup")
        .expect("destination exists");

    let mut dedupe = SqliteDedupe::new(db_path_string.clone());
    assert_eq!(
        dedupe
            .check_and_insert(
                "msg_int_01",
                OffsetDateTime::now_utc() + time::Duration::seconds(30)
            )
            .expect("dedupe insert"),
        DedupeResult::Inserted
    );

    let message = build_message("msg_int_01", "trc_int_01", 10_000);
    let mut router = TfpRouter::new_with_policy_and_ack_store(
        "turingflowd",
        ClientTlsConfig::default(),
        RouterRetryPolicy::default(),
        SqliteAckStore::new(db_path_string),
    )
    .expect("router");

    let response = router
        .forward_message(
            message,
            &DestinationRoute {
                agent_ref: destination.agent_ref,
                deliver_url: destination.deliver_url,
            },
        )
        .await
        .expect("forwarded");

    assert!(response.accepted);
    assert_eq!(delivered_count.load(Ordering::Relaxed), 1);

    let ack = AckRequest {
        version: TFPV1_VERSION.to_string(),
        delivery_id: response.delivery_id.clone(),
        message_id: "msg_int_01".to_string(),
        from_ref: "executor@node-b.local".to_string(),
        status: AckStatus::Processed,
        timestamp: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("timestamp"),
        result: Some(json!({"ok": true})),
    };
    let ack_response = router.record_ack(ack).expect("ack store");
    assert!(ack_response.accepted);

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let stored_acks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM acks WHERE delivery_id = ?1",
            rusqlite::params![response.delivery_id],
            |row| row.get(0),
        )
        .expect("ack count");
    assert_eq!(stored_acks, 1);

    server_task.abort();
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn lease_expired_removes_destination_sqlite_registry() {
    let db_path = temp_db_path("lease_expired");
    initialize_database(&db_path).expect("db init");
    let mut registry = SqliteRegistry::new(db_path.to_string_lossy().to_string());

    registry
        .register(RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-b".to_string(),
                hostname: "node-b.local".to_string(),
                deliver_url: "http://127.0.0.1:9444".to_string(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "executor@node-b.local".to_string(),
                agent_id: "ag_01B".to_string(),
            }],
            lease_ttl_ms: 20,
        })
        .expect("register");

    std::thread::sleep(Duration::from_millis(40));
    let resolved = registry
        .resolve("kingdom-main", "executor@node-b.local")
        .expect("resolve");
    assert!(!resolved.found);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn destination_offline_returns_failure() {
    let db_path = temp_db_path("offline");
    initialize_database(&db_path).expect("db init");

    let message = build_message("msg_int_offline", "trc_int_offline", 5_000);
    let mut router = TfpRouter::new_with_policy_and_ack_store(
        "turingflowd",
        ClientTlsConfig::default(),
        RouterRetryPolicy::default(),
        SqliteAckStore::new(db_path.to_string_lossy().to_string()),
    )
    .expect("router");

    let err = router
        .forward_message(
            message,
            &DestinationRoute {
                agent_ref: "executor@node-b.local".to_string(),
                deliver_url: "http://127.0.0.1:9".to_string(),
            },
        )
        .await
        .expect_err("must fail when destination is offline");

    assert!(matches!(err, RouterError::DestinationUnreachable { .. }));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn dedupe_rejects_duplicate_message_sqlite() {
    let db_path = temp_db_path("dedupe");
    initialize_database(&db_path).expect("db init");
    let mut dedupe = SqliteDedupe::new(db_path.to_string_lossy().to_string());
    let now = OffsetDateTime::now_utc();

    assert_eq!(
        dedupe
            .check_and_insert("msg_dup_01", now + time::Duration::seconds(30))
            .expect("insert #1"),
        DedupeResult::Inserted
    );
    assert_eq!(
        dedupe
            .check_and_insert("msg_dup_01", now + time::Duration::seconds(30))
            .expect("insert #2"),
        DedupeResult::Duplicate
    );

    let _ = std::fs::remove_file(db_path);
}

async fn spawn_mock_agent(counter: Arc<AtomicUsize>) -> (String, JoinHandle<()>) {
    let app = Router::new()
        .route("/tfpv1/deliver", post(mock_deliver))
        .with_state(counter);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    (format!("http://{}", addr), task)
}

async fn mock_deliver(
    State(counter): State<Arc<AtomicUsize>>,
    Json(_payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    counter.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        Json(json!({
            "version": "TFPv1",
            "ack": "processed"
        })),
    )
}

fn build_message(message_id: &str, trace_id: &str, ttl_ms: u64) -> Envelope {
    Envelope {
        version: TFPV1_VERSION.to_string(),
        message_id: message_id.to_string(),
        trace_id: trace_id.to_string(),
        timestamp: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("rfc3339 timestamp"),
        from_ref: "planner@node-a.local".to_string(),
        to_ref: "executor@node-b.local".to_string(),
        kind: MessageKind::Request,
        ttl_ms,
        requires_ack: true,
        routing: Routing {
            hops_max: 8,
            path: vec![],
        },
        payload: Payload {
            content_type: "application/json".to_string(),
            body: json!({"cmd": "run"}),
        },
        meta: None,
    }
}

fn temp_db_path(suffix: &str) -> std::path::PathBuf {
    let file_name = format!(
        "turingflow_integration_{}_{}_{}.db",
        suffix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    );
    std::env::temp_dir().join(file_name)
}
