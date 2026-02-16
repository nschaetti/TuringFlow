use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, SanType,
};
use reqwest::{Certificate as ReqwestCert, Client, Identity};
use serde_json::{json, Value};
use tempfile::TempDir;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::net::TcpListener as TokioTcpListener;

#[tokio::test]
async fn https_mtls_endpoints_work_with_sqlite_backend() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let temp_dir = TempDir::new().expect("tempdir");
    let certs = generate_test_certs(temp_dir.path()).expect("cert generation");
    let listen = random_listen_addr().expect("listen addr");
    let db_path = temp_dir.path().join("turingflow.db");

    let config_path = write_turingflowd_config(temp_dir.path(), &listen, &certs, &db_path);
    let kingdoms_path = write_kingdoms_config(temp_dir.path(), 16, 120_000, 30_000, 4096);

    let mut daemon = start_daemon(&config_path, &kingdoms_path).expect("start daemon");
    let client = make_mtls_client(&certs).expect("mtls client");
    wait_for_health(&client, &listen, &mut daemon).await;

    let delivered = Arc::new(AtomicUsize::new(0));
    let (deliver_url, deliver_task) = spawn_mock_deliver_https(delivered.clone(), &certs).await;

    let register = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "node": {
            "node_id": "node-a",
            "hostname": "node-a.local",
            "deliver_url": deliver_url
        },
        "agents": [
            { "agent_ref": "planner@node-a.local", "agent_id": "ag_planner" },
            { "agent_ref": "executor@node-a.local", "agent_id": "ag_executor" }
        ],
        "lease_ttl_ms": 60_000
    });

    let register_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/agents/register"),
        register,
    )
    .await;
    assert_eq!(
        register_resp.0, 200,
        "register response: {}",
        register_resp.1
    );
    let lease_id = register_resp.1["lease_id"]
        .as_str()
        .expect("lease_id")
        .to_string();

    let heartbeat = json!({
        "version": "TFPv1",
        "lease_id": lease_id,
        "kingdom_id": "kingdom-main",
        "node_id": "node-a",
        "agents": ["planner@node-a.local", "executor@node-a.local"]
    });
    let heartbeat_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/agents/heartbeat"),
        heartbeat,
    )
    .await;
    assert_eq!(
        heartbeat_resp.0, 200,
        "heartbeat response: {}",
        heartbeat_resp.1
    );

    let resolve_resp = get_json(
        &client,
        &format!(
            "https://{listen}/tfpv1/agents/resolve/executor@node-a.local?kingdom_id=kingdom-main"
        ),
    )
    .await;
    assert_eq!(resolve_resp.0, 200, "resolve response: {}", resolve_resp.1);
    assert_eq!(resolve_resp.1["found"], Value::Bool(true));

    let send = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "message": {
            "version": "TFPv1",
            "message_id": "msg_http_01",
            "trace_id": "trc_http_01",
            "timestamp": OffsetDateTime::now_utc().format(&Rfc3339).expect("ts"),
            "from_ref": "planner@node-a.local",
            "to_ref": "executor@node-a.local",
            "kind": "request",
            "ttl_ms": 10_000,
            "requires_ack": true,
            "routing": { "hops_max": 8, "path": [] },
            "payload": {
                "content_type": "application/json",
                "body": { "task": "ping" }
            },
            "meta": { "priority": "normal", "tags": ["http-test"] }
        }
    });

    let send_resp = match client
        .post(format!("https://{listen}/tfpv1/messages/send"))
        .json(&send)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response
                .json::<Value>()
                .await
                .unwrap_or_else(|_| json!({"error": "invalid json"}));
            (status, body)
        }
        Err(error) => {
            let logs = daemon.collect_logs();
            panic!("send request failed: {error}. daemon logs:\n{logs}");
        }
    };
    assert_eq!(send_resp.0, 200, "send response: {}", send_resp.1);
    assert_eq!(delivered.load(Ordering::Relaxed), 1);

    let ack = json!({
        "version": "TFPv1",
        "delivery_id": send_resp.1["delivery_id"].as_str().unwrap_or("dlv_unknown"),
        "message_id": "msg_http_01",
        "from_ref": "executor@node-a.local",
        "status": "processed",
        "timestamp": OffsetDateTime::now_utc().format(&Rfc3339).expect("ts"),
        "result": { "ok": true }
    });
    let ack_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/messages/ack"),
        ack,
    )
    .await;
    assert_eq!(ack_resp.0, 200, "ack response: {}", ack_resp.1);

    daemon.stop();
    deliver_task.abort();
}

#[tokio::test]
async fn https_mtls_enforces_kingdom_and_quota_rules() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let temp_dir = TempDir::new().expect("tempdir");
    let certs = generate_test_certs(temp_dir.path()).expect("cert generation");
    let listen = random_listen_addr().expect("listen addr");
    let db_path = temp_dir.path().join("turingflow.db");

    let config_path = write_turingflowd_config(temp_dir.path(), &listen, &certs, &db_path);
    let kingdoms_path = write_kingdoms_config(temp_dir.path(), 8, 5_000, 800, 2_000);

    let mut daemon = start_daemon(&config_path, &kingdoms_path).expect("start daemon");
    let client = make_mtls_client(&certs).expect("mtls client");
    wait_for_health(&client, &listen, &mut daemon).await;

    let forbidden_register = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-forbidden",
        "node": {
            "node_id": "node-a",
            "hostname": "node-a.local",
            "deliver_url": "https://127.0.0.1:9555"
        },
        "agents": [{ "agent_ref": "planner@node-a.local", "agent_id": "ag_1" }],
        "lease_ttl_ms": 1000
    });

    let forbidden_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/agents/register"),
        forbidden_register,
    )
    .await;
    assert_eq!(
        forbidden_resp.0, 403,
        "forbidden response: {}",
        forbidden_resp.1
    );
    assert_eq!(
        forbidden_resp.1["error"]["code"],
        Value::String("kingdom_not_allowed".to_string())
    );

    let delivered = Arc::new(AtomicUsize::new(0));
    let (deliver_url, deliver_task) = spawn_mock_deliver_https(delivered, &certs).await;

    let ok_register = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "node": {
            "node_id": "node-a",
            "hostname": "node-a.local",
            "deliver_url": deliver_url
        },
        "agents": [
            { "agent_ref": "planner@node-a.local", "agent_id": "ag_planner" },
            { "agent_ref": "executor@node-a.local", "agent_id": "ag_executor" }
        ],
        "lease_ttl_ms": 3000
    });
    let ok_register_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/agents/register"),
        ok_register,
    )
    .await;
    assert_eq!(
        ok_register_resp.0, 200,
        "register response: {}",
        ok_register_resp.1
    );

    let ttl_too_high_send = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "message": {
            "version": "TFPv1",
            "message_id": "msg_quota_ttl",
            "trace_id": "trc_quota_ttl",
            "timestamp": OffsetDateTime::now_utc().format(&Rfc3339).expect("ts"),
            "from_ref": "planner@node-a.local",
            "to_ref": "executor@node-a.local",
            "kind": "request",
            "ttl_ms": 3000,
            "requires_ack": true,
            "routing": { "hops_max": 8, "path": [] },
            "payload": {
                "content_type": "application/json",
                "body": { "x": 1 }
            },
            "meta": { "priority": "normal", "tags": ["quota"] }
        }
    });
    let ttl_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/messages/send"),
        ttl_too_high_send,
    )
    .await;
    assert_eq!(ttl_resp.0, 400, "ttl response: {}", ttl_resp.1);
    assert_eq!(
        ttl_resp.1["error"]["code"],
        Value::String("invalid_payload".to_string())
    );

    let oversized_blob = "x".repeat(4_000);
    let payload_too_large_send = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "message": {
            "version": "TFPv1",
            "message_id": "msg_quota_payload",
            "trace_id": "trc_quota_payload",
            "timestamp": OffsetDateTime::now_utc().format(&Rfc3339).expect("ts"),
            "from_ref": "planner@node-a.local",
            "to_ref": "executor@node-a.local",
            "kind": "request",
            "ttl_ms": 500,
            "requires_ack": true,
            "routing": { "hops_max": 8, "path": [] },
            "payload": {
                "content_type": "application/json",
                "body": { "blob": oversized_blob }
            },
            "meta": { "priority": "normal", "tags": ["quota"] }
        }
    });
    let payload_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/messages/send"),
        payload_too_large_send,
    )
    .await;
    assert_eq!(payload_resp.0, 413, "payload response: {}", payload_resp.1);
    assert_eq!(
        payload_resp.1["error"]["code"],
        Value::String("payload_too_large".to_string())
    );

    daemon.stop();
    deliver_task.abort();
}

#[tokio::test]
async fn https_mtls_local_agent_op_denies_forbidden_file_with_eacces() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let temp_dir = TempDir::new().expect("tempdir");
    let certs = generate_test_certs(temp_dir.path()).expect("cert generation");
    let listen = random_listen_addr().expect("listen addr");
    let db_path = temp_dir.path().join("turingflow.db");

    let config_path = write_turingflowd_config(temp_dir.path(), &listen, &certs, &db_path);
    let kingdoms_path = write_kingdoms_config(temp_dir.path(), 8, 30_000, 10_000, 16_384);

    let mut daemon = start_daemon(&config_path, &kingdoms_path).expect("start daemon");
    let client = make_mtls_client(&certs).expect("mtls client");
    wait_for_health(&client, &listen, &mut daemon).await;

    let register = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "node": {
            "node_id": "node-a",
            "hostname": "node-a.local",
            "deliver_url": "https://127.0.0.1:9555"
        },
        "agents": [
            { "agent_ref": "planner@node-a.local", "agent_id": "ag_planner" },
            { "agent_ref": "executor@node-a.local", "agent_id": "ag_executor" }
        ],
        "lease_ttl_ms": 10_000
    });

    let register_resp = post_json(
        &client,
        &format!("https://{listen}/tfpv1/agents/register"),
        register,
    )
    .await;
    assert_eq!(
        register_resp.0, 200,
        "register response: {}",
        register_resp.1
    );

    let send = json!({
        "version": "TFPv1",
        "kingdom_id": "kingdom-main",
        "message": {
            "version": "TFPv1",
            "message_id": "msg_local_op_1",
            "trace_id": "trc_local_op_1",
            "timestamp": OffsetDateTime::now_utc().format(&Rfc3339).expect("ts"),
            "from_ref": "planner@node-a.local",
            "to_ref": "executor@node-a.local",
            "kind": "request",
            "ttl_ms": 5_000,
            "requires_ack": false,
            "routing": { "hops_max": 8, "path": [] },
            "payload": {
                "content_type": "application/vnd.turingflow.agent-op+json",
                "body": {
                    "op": "fs.read",
                    "path": "/etc/passwd"
                }
            },
            "meta": { "priority": "normal", "tags": ["tool:fs"] }
        }
    });

    let send_resp = match client
        .post(format!("https://{listen}/tfpv1/messages/send"))
        .json(&send)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response
                .json::<Value>()
                .await
                .unwrap_or_else(|_| json!({"error": "invalid json"}));
            (status, body)
        }
        Err(error) => {
            let logs = daemon.collect_logs();
            panic!("send request failed: {error}. daemon logs:\n{logs}");
        }
    };

    assert_eq!(send_resp.0, 403, "send response: {}", send_resp.1);
    assert_eq!(
        send_resp.1["error"]["code"],
        Value::String("EACCES".to_string())
    );

    daemon.stop();
}

struct DaemonProcess {
    child: Option<Child>,
}

impl DaemonProcess {
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn collect_logs(&mut self) -> String {
        let Some(mut child) = self.child.take() else {
            return String::new();
        };

        let _ = child.kill();
        match child.wait_with_output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!("stdout:\n{}\nstderr:\n{}", stdout, stderr)
            }
            Err(_) => String::new(),
        }
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_daemon(config_path: &Path, kingdoms_path: &Path) -> io::Result<DaemonProcess> {
    let binary = env!("CARGO_BIN_EXE_turingflowd");
    let child = Command::new(binary)
        .arg("--config")
        .arg(config_path)
        .arg("--kingdoms-config")
        .arg(kingdoms_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(DaemonProcess { child: Some(child) })
}

async fn wait_for_health(client: &Client, listen: &str, daemon: &mut DaemonProcess) {
    let url = format!("https://{listen}/tfpv1/health");
    for _ in 0..80 {
        if let Some(child) = daemon.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                let logs = daemon.collect_logs();
                panic!("daemon exited early with status {status}. logs:\n{logs}");
            }
        }
        let result = client.get(&url).send().await;
        if let Ok(response) = result {
            if response.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let logs = daemon.collect_logs();
    panic!("health endpoint did not become ready. logs:\n{logs}");
}

async fn post_json(client: &Client, url: &str, body: Value) -> (u16, Value) {
    let response = client
        .post(url)
        .json(&body)
        .send()
        .await
        .expect("http post");
    let status = response.status().as_u16();
    let value = response.json::<Value>().await.expect("json response");
    (status, value)
}

async fn get_json(client: &Client, url: &str) -> (u16, Value) {
    let response = client.get(url).send().await.expect("http get");
    let status = response.status().as_u16();
    let value = response.json::<Value>().await.expect("json response");
    (status, value)
}

async fn spawn_mock_deliver_https(
    counter: Arc<AtomicUsize>,
    certs: &CertPaths,
) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/tfpv1/deliver", post(mock_deliver))
        .with_state(counter);

    let listener = TokioTcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock deliver");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    let tls_config = RustlsConfig::from_pem_file(&certs.server_cert, &certs.server_key)
        .await
        .expect("mock deliver tls config");

    let handle = tokio::spawn(async move {
        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await
            .expect("serve mock deliver");
    });

    (format!("https://{}", addr), handle)
}

async fn mock_deliver(
    State(counter): State<Arc<AtomicUsize>>,
    Json(_payload): Json<Value>,
) -> Json<Value> {
    counter.fetch_add(1, Ordering::Relaxed);
    Json(json!({ "version": "TFPv1", "ack": "processed" }))
}

struct CertPaths {
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
}

fn generate_test_certs(dir: &Path) -> Result<CertPaths, Box<dyn std::error::Error>> {
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "turingflow-test-ca");
    let ca_key = KeyPair::generate()?;
    let ca_cert = ca_params.self_signed(&ca_key)?;

    let mut server_params = CertificateParams::new(vec!["localhost".to_string()])?;
    server_params
        .subject_alt_names
        .push(SanType::IpAddress("127.0.0.1".parse()?));
    server_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    server_params
        .distinguished_name
        .push(DnType::CommonName, "turingflowd");
    let server_key = KeyPair::generate()?;
    let server_cert = server_params.signed_by(&server_key, &ca_cert, &ca_key)?;

    let mut client_params = CertificateParams::new(vec!["node-a".to_string()])?;
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    client_params
        .distinguished_name
        .push(DnType::CommonName, "node-a");
    let client_key = KeyPair::generate()?;
    let client_cert = client_params.signed_by(&client_key, &ca_cert, &ca_key)?;

    let ca_cert_path = dir.join("ca.crt");
    let server_cert_path = dir.join("server.crt");
    let server_key_path = dir.join("server.key");
    let client_cert_path = dir.join("node-a.crt");
    let client_key_path = dir.join("node-a.key");

    std::fs::write(&ca_cert_path, ca_cert.pem())?;
    std::fs::write(&server_cert_path, server_cert.pem())?;
    std::fs::write(&server_key_path, server_key.serialize_pem())?;
    std::fs::write(&client_cert_path, client_cert.pem())?;
    std::fs::write(&client_key_path, client_key.serialize_pem())?;

    Ok(CertPaths {
        ca_cert: ca_cert_path,
        server_cert: server_cert_path,
        server_key: server_key_path,
        client_cert: client_cert_path,
        client_key: client_key_path,
    })
}

fn make_mtls_client(certs: &CertPaths) -> Result<Client, Box<dyn std::error::Error>> {
    let ca_pem = std::fs::read(&certs.ca_cert)?;
    let cert = ReqwestCert::from_pem(&ca_pem)?;

    let mut identity_pem = std::fs::read(&certs.client_cert)?;
    identity_pem.push(b'\n');
    identity_pem.extend_from_slice(&std::fs::read(&certs.client_key)?);
    let identity = Identity::from_pem(&identity_pem)?;

    let client = Client::builder()
        .add_root_certificate(cert)
        .identity(identity)
        .build()?;

    Ok(client)
}

fn write_turingflowd_config(
    dir: &Path,
    listen: &str,
    certs: &CertPaths,
    db_path: &Path,
) -> PathBuf {
    let path = dir.join("turingflowd.yaml");
    let content = format!(
        "version: 1
server:
  listen: {listen}
  node_id: turingflowd
tls:
  server_cert: {}
  server_key: {}
  client_ca_cert: {}
  upstream_ca_cert: {}
  upstream_client_cert: null
  upstream_client_key: null
security:
  replay_window_seconds: 60
routing:
  retry_delays_ms: [0, 50, 150]
storage:
  backend: sqlite
  sqlite:
    path: {}
limits:
  max_payload_bytes: 65536
  max_message_ttl_ms: 60000
logging:
  format: json
  level: info
",
        certs.server_cert.display(),
        certs.server_key.display(),
        certs.ca_cert.display(),
        certs.ca_cert.display(),
        db_path.display()
    );
    std::fs::write(&path, content).expect("write daemon config");
    path
}

fn write_kingdoms_config(
    dir: &Path,
    max_agents_per_node: usize,
    max_lease_ttl_ms: u64,
    max_message_ttl_ms: u64,
    max_payload_bytes: usize,
) -> PathBuf {
    let path = dir.join("kingdoms.yaml");
    let content = format!(
        "version: 1
kingdoms:
  - id: kingdom-main
    enabled: true
    quotas:
      max_agents_per_node: {max_agents_per_node}
      max_lease_ttl_ms: {max_lease_ttl_ms}
      max_message_ttl_ms: {max_message_ttl_ms}
      max_payload_bytes: {max_payload_bytes}
"
    );
    std::fs::write(&path, content).expect("write kingdoms config");
    path
}

fn random_listen_addr() -> io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    Ok(addr.to_string())
}
