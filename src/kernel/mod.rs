//! Kernel entry-point for policy-gated syscalls.
//!
//! The kernel enforces authorization before delegating to syscall providers.
//! Every decision can be audited through an [`crate::kernel::audit::AuditSink`].

/// Auditing types and SQLite sink.
pub mod audit;
/// Request execution context and principal resolution.
pub mod context;
/// Kernel error model.
pub mod errors;
/// Policy configuration and evaluator.
pub mod policy;
/// Syscall request/response and provider traits.
pub mod syscalls;

use audit::{now_epoch_ms, AuditDecision, AuditRecord, AuditSink, NoopAuditSink};
use context::ExecutionContext;
use errors::KernelError;
use policy::{DecisionResult, PolicyEngine};
use serde_json::json;
use syscalls::fs::{
    FsListReq, FsListResp, FsProvider, FsReadReq, FsReadResp, FsWriteReq, FsWriteResp,
};
use syscalls::net::{NetHttpReq, NetHttpResp, NetworkProvider};
use syscalls::process::{ProcExecReq, ProcExecResp, ProcessProvider};
use syscalls::user::{
    UserCommsProvider, UserInboxReq, UserInboxResp, UserIngestReq, UserIngestResp, UserRecvReq,
    UserRecvResp, UserRouteResolveReq, UserRouteResolveResp, UserSendReq, UserSendResp,
};

/// Policy-enforcing syscall dispatcher.
///
/// # Thread safety
///
/// `Kernel` is `Clone` and internally shares providers with `Arc`, so it can be
/// used concurrently across async handlers or worker threads.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use turingflow::kernel::policy::{PolicyConfig, PolicyEngine};
/// use turingflow::kernel::syscalls::fs::HostFsProvider;
/// use turingflow::kernel::Kernel;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let policy: PolicyConfig = serde_yaml::from_str(
///     "version: 1\ndefaults:\n  decision: deny\nprincipals: []\n"
/// )?;
/// let fs = Arc::new(HostFsProvider::new(std::env::current_dir()?)?);
/// let kernel = Kernel::new(PolicyEngine::new(policy), fs);
/// let _ = kernel;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Kernel {
    policy: PolicyEngine,
    fs_provider: std::sync::Arc<dyn FsProvider>,
    process_provider: std::sync::Arc<dyn ProcessProvider>,
    network_provider: std::sync::Arc<dyn NetworkProvider>,
    user_provider: std::sync::Arc<dyn UserCommsProvider>,
    audit_sink: std::sync::Arc<dyn AuditSink>,
}

impl Kernel {
    /// Creates a kernel with filesystem provider only.
    ///
    /// Process, network, and user communication syscalls are denied by default.
    pub fn new(policy: PolicyEngine, fs_provider: std::sync::Arc<dyn FsProvider>) -> Self {
        Self::new_with_providers_and_audit(
            policy,
            fs_provider,
            std::sync::Arc::new(DenyProcessProvider),
            std::sync::Arc::new(DenyNetworkProvider),
            std::sync::Arc::new(DenyUserCommsProvider),
            std::sync::Arc::new(NoopAuditSink),
        )
    }

    /// Creates a kernel with filesystem, process, and network providers.
    pub fn new_with_providers(
        policy: PolicyEngine,
        fs_provider: std::sync::Arc<dyn FsProvider>,
        process_provider: std::sync::Arc<dyn ProcessProvider>,
        network_provider: std::sync::Arc<dyn NetworkProvider>,
    ) -> Self {
        Self::new_with_providers_and_audit(
            policy,
            fs_provider,
            process_provider,
            network_provider,
            std::sync::Arc::new(DenyUserCommsProvider),
            std::sync::Arc::new(NoopAuditSink),
        )
    }

    /// Creates a kernel with filesystem and user communication providers.
    pub fn new_with_user_provider(
        policy: PolicyEngine,
        fs_provider: std::sync::Arc<dyn FsProvider>,
        user_provider: std::sync::Arc<dyn UserCommsProvider>,
    ) -> Self {
        Self::new_with_providers_and_audit(
            policy,
            fs_provider,
            std::sync::Arc::new(DenyProcessProvider),
            std::sync::Arc::new(DenyNetworkProvider),
            user_provider,
            std::sync::Arc::new(NoopAuditSink),
        )
    }

    /// Creates a kernel with full provider and audit sink customization.
    pub fn new_with_providers_and_audit(
        policy: PolicyEngine,
        fs_provider: std::sync::Arc<dyn FsProvider>,
        process_provider: std::sync::Arc<dyn ProcessProvider>,
        network_provider: std::sync::Arc<dyn NetworkProvider>,
        user_provider: std::sync::Arc<dyn UserCommsProvider>,
        audit_sink: std::sync::Arc<dyn AuditSink>,
    ) -> Self {
        Self {
            policy,
            fs_provider,
            process_provider,
            network_provider,
            user_provider,
            audit_sink,
        }
    }

    /// Executes `fs.list` after policy evaluation.
    pub fn fs_list(
        &self,
        ctx: &ExecutionContext,
        req: FsListReq,
    ) -> Result<FsListResp, KernelError> {
        self.assert_allowed_with_resource(ctx, "fs.list", json!({ "path": req.path.clone() }))?;
        self.fs_provider.list(ctx, req)
    }

    /// Executes `fs.read` after policy evaluation.
    pub fn fs_read(
        &self,
        ctx: &ExecutionContext,
        req: FsReadReq,
    ) -> Result<FsReadResp, KernelError> {
        self.assert_allowed_with_resource(ctx, "fs.read", json!({ "path": req.path.clone() }))?;
        self.fs_provider.read(ctx, req)
    }

    /// Executes `fs.write` after policy evaluation.
    pub fn fs_write(
        &self,
        ctx: &ExecutionContext,
        req: FsWriteReq,
    ) -> Result<FsWriteResp, KernelError> {
        self.assert_allowed_with_resource(ctx, "fs.write", json!({ "path": req.path.clone() }))?;
        self.fs_provider.write(ctx, req)
    }

    /// Executes `proc.exec` after policy evaluation.
    pub fn proc_exec(
        &self,
        ctx: &ExecutionContext,
        req: ProcExecReq,
    ) -> Result<ProcExecResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "proc.exec",
            json!({ "command": req.command.clone(), "args": req.args.clone() }),
        )?;
        self.process_provider.exec(ctx, req)
    }

    /// Executes `net.http` after policy evaluation.
    pub fn net_http(
        &self,
        ctx: &ExecutionContext,
        req: NetHttpReq,
    ) -> Result<NetHttpResp, KernelError> {
        let host = reqwest::Url::parse(&req.url)
            .ok()
            .and_then(|url| url.host_str().map(ToString::to_string));
        self.assert_allowed_with_resource(
            ctx,
            "net.http",
            json!({
                "method": req.method.clone(),
                "url": req.url.clone(),
                "host": host
            }),
        )?;
        self.network_provider.http(ctx, req)
    }

    /// Executes `user.ingest` after policy evaluation.
    pub fn user_ingest(
        &self,
        ctx: &ExecutionContext,
        req: UserIngestReq,
    ) -> Result<UserIngestResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "user.ingest",
            json!({
                "channel": req.channel.clone(),
                "thread_id": req.thread_id.clone(),
            }),
        )?;
        self.user_provider.ingest(ctx, req)
    }

    /// Executes `user.recv` after policy evaluation.
    pub fn user_recv(
        &self,
        ctx: &ExecutionContext,
        req: UserRecvReq,
    ) -> Result<UserRecvResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "user.recv",
            json!({
                "limit": req.limit,
                "consume": req.consume,
            }),
        )?;
        self.user_provider.recv(ctx, req)
    }

    /// Executes `user.send` after policy evaluation.
    pub fn user_send(
        &self,
        ctx: &ExecutionContext,
        req: UserSendReq,
    ) -> Result<UserSendResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "user.send",
            json!({
                "channel": req.channel.clone(),
                "thread_id": req.thread_id.clone(),
            }),
        )?;
        self.user_provider.send(ctx, req)
    }

    /// Executes `user.inbox` after policy evaluation.
    pub fn user_inbox(
        &self,
        ctx: &ExecutionContext,
        req: UserInboxReq,
    ) -> Result<UserInboxResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "user.inbox",
            json!({
                "limit": req.limit,
                "include_delivered": req.include_delivered,
            }),
        )?;
        self.user_provider.inbox(ctx, req)
    }

    /// Executes `user.route.resolve` after policy evaluation.
    pub fn user_route_resolve(
        &self,
        ctx: &ExecutionContext,
        req: UserRouteResolveReq,
    ) -> Result<UserRouteResolveResp, KernelError> {
        self.assert_allowed_with_resource(
            ctx,
            "user.route.resolve",
            json!({
                "thread_id": req.thread_id.clone(),
                "preferred_channel": req.preferred_channel.clone(),
            }),
        )?;
        self.user_provider.route_resolve(ctx, req)
    }

    /// Checks permission for a syscall without resource attributes.
    pub fn assert_allowed(&self, ctx: &ExecutionContext, syscall: &str) -> Result<(), KernelError> {
        let decision = self.policy.evaluate(ctx, syscall);
        self.audit_decision(ctx, syscall, None, &decision, 0);
        if decision.allowed {
            return Ok(());
        }

        Err(KernelError::access_denied(format!(
            "syscall '{}' denied for agent '{}'",
            syscall, ctx.agent_ref
        )))
    }

    /// Checks permission for a syscall with resource attributes.
    pub fn assert_allowed_with_resource(
        &self,
        ctx: &ExecutionContext,
        syscall: &str,
        resource: serde_json::Value,
    ) -> Result<(), KernelError> {
        let resource_json = serde_json::to_string(&resource).ok();
        let decision = self
            .policy
            .evaluate_with_resource(ctx, syscall, Some(&resource));
        self.audit_decision(ctx, syscall, resource_json, &decision, 0);
        if decision.allowed {
            return Ok(());
        }

        Err(KernelError::access_denied(format!(
            "syscall '{}' denied for agent '{}'",
            syscall, ctx.agent_ref
        )))
    }

    fn audit_decision(
        &self,
        ctx: &ExecutionContext,
        syscall: &str,
        resource_json: Option<String>,
        decision: &DecisionResult,
        latency_ms: i64,
    ) {
        let (decision_kind, error_code, error_message) = if decision.allowed {
            (AuditDecision::Allow, None, None)
        } else {
            (
                AuditDecision::Deny,
                Some(crate::kernel::errors::KernelErrorCode::Eacces),
                Some(format!("syscall '{}' denied by policy", syscall)),
            )
        };

        let record = AuditRecord {
            ts_ms: now_epoch_ms(),
            trace_id: ctx.trace_id.clone(),
            kingdom_id: ctx.kingdom_id.clone(),
            principal_id: decision.principal_id.clone(),
            agent_ref: ctx.agent_ref.clone(),
            tool_id: ctx.tool_id.clone(),
            syscall: syscall.to_string(),
            resource_json,
            decision: decision_kind,
            rule_id: decision.rule_id.clone(),
            error_code,
            error_message,
            latency_ms,
        };

        self.audit_sink.record(&record);
    }
}

#[derive(Debug)]
struct DenyProcessProvider;

impl ProcessProvider for DenyProcessProvider {
    fn exec(
        &self,
        _ctx: &ExecutionContext,
        _req: ProcExecReq,
    ) -> Result<ProcExecResp, KernelError> {
        Err(KernelError::access_denied(
            "process provider is not configured",
        ))
    }
}

#[derive(Debug)]
struct DenyNetworkProvider;

impl NetworkProvider for DenyNetworkProvider {
    fn http(&self, _ctx: &ExecutionContext, _req: NetHttpReq) -> Result<NetHttpResp, KernelError> {
        Err(KernelError::access_denied(
            "network provider is not configured",
        ))
    }
}

#[derive(Debug)]
struct DenyUserCommsProvider;

impl UserCommsProvider for DenyUserCommsProvider {
    fn ingest(
        &self,
        _ctx: &ExecutionContext,
        _req: UserIngestReq,
    ) -> Result<UserIngestResp, KernelError> {
        Err(KernelError::access_denied(
            "user comms provider is not configured",
        ))
    }

    fn recv(
        &self,
        _ctx: &ExecutionContext,
        _req: UserRecvReq,
    ) -> Result<UserRecvResp, KernelError> {
        Err(KernelError::access_denied(
            "user comms provider is not configured",
        ))
    }

    fn send(
        &self,
        _ctx: &ExecutionContext,
        _req: UserSendReq,
    ) -> Result<UserSendResp, KernelError> {
        Err(KernelError::access_denied(
            "user comms provider is not configured",
        ))
    }

    fn inbox(
        &self,
        _ctx: &ExecutionContext,
        _req: UserInboxReq,
    ) -> Result<UserInboxResp, KernelError> {
        Err(KernelError::access_denied(
            "user comms provider is not configured",
        ))
    }

    fn route_resolve(
        &self,
        _ctx: &ExecutionContext,
        _req: UserRouteResolveReq,
    ) -> Result<UserRouteResolveResp, KernelError> {
        Err(KernelError::access_denied(
            "user comms provider is not configured",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    use rusqlite::params;
    use tempfile::TempDir;

    use crate::kernel::audit::SqliteAuditSink;
    use crate::kernel::context::ExecutionContext;
    use crate::kernel::policy::{PolicyConfig, PolicyEngine};
    use crate::kernel::syscalls::fs::{FsReadReq, FsWriteReq, HostFsProvider};
    use crate::kernel::syscalls::net::{HostNetworkProvider, NetHttpReq};
    use crate::kernel::syscalls::process::{AllowedCommand, HostProcessProvider, ProcExecReq};
    use crate::tfpv1::storage::sqlite::{initialize_database, open_connection};

    use super::Kernel;

    #[test]
    fn kernel_allows_fs_read_for_matching_path_prefix() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("root dir");
        let file_path = root.join("allowed.txt");
        std::fs::write(&file_path, b"ok").expect("seed file");

        let policy = load_policy(
            r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "allow-fs-read-root"
        effect: allow
        syscall: "fs.read"
        resource:
          path_prefix:
            - "ROOT"
"#,
        )
        .replace("ROOT", &root.display().to_string());

        let engine =
            PolicyEngine::new(serde_yaml::from_str::<PolicyConfig>(&policy).expect("yaml"));
        let provider = HostFsProvider::new(&root).expect("provider");
        let kernel = Kernel::new(engine, Arc::new(provider));

        let read = kernel
            .fs_read(
                &ctx(),
                FsReadReq {
                    path: file_path.display().to_string(),
                },
            )
            .expect("read allowed");

        assert_eq!(read.content, b"ok");
    }

    #[test]
    fn kernel_denies_fs_write_when_policy_does_not_match_path() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("root dir");

        let policy = load_policy(
            r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "allow-fs-write-subdir"
        effect: allow
        syscall: "fs.write"
        resource:
          path_prefix:
            - "ROOT/safe"
"#,
        )
        .replace("ROOT", &root.display().to_string());

        let engine =
            PolicyEngine::new(serde_yaml::from_str::<PolicyConfig>(&policy).expect("yaml"));
        let provider = HostFsProvider::new(&root).expect("provider");
        let kernel = Kernel::new(engine, Arc::new(provider));

        let err = kernel
            .fs_write(
                &ctx(),
                FsWriteReq {
                    path: root.join("forbidden.txt").display().to_string(),
                    content: b"nope".to_vec(),
                },
            )
            .expect_err("write denied");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[test]
    fn writes_allow_and_deny_decisions_to_sqlite_audit_log() {
        let temp = TempDir::new().expect("tempdir");
        let db_path = temp.path().join("kernel_audit.db");
        initialize_database(&db_path).expect("db init");

        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("root dir");
        std::fs::write(root.join("allowed.txt"), b"ok").expect("seed file");

        let policy = load_policy(
            r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "allow-fs-read-root"
        effect: allow
        syscall: "fs.read"
        resource:
          path_prefix:
            - "ROOT"
"#,
        )
        .replace("ROOT", &root.display().to_string());

        let engine =
            PolicyEngine::new(serde_yaml::from_str::<PolicyConfig>(&policy).expect("yaml"));
        let provider = HostFsProvider::new(&root).expect("provider");
        let audit_sink = SqliteAuditSink::new(db_path.display().to_string(), 86_400_000, 100);
        let kernel = Kernel::new_with_providers_and_audit(
            engine,
            Arc::new(provider),
            Arc::new(super::DenyProcessProvider),
            Arc::new(super::DenyNetworkProvider),
            Arc::new(super::DenyUserCommsProvider),
            Arc::new(audit_sink),
        );

        kernel
            .fs_read(
                &ctx(),
                FsReadReq {
                    path: root.join("allowed.txt").display().to_string(),
                },
            )
            .expect("read allowed");

        let _ = kernel.fs_read(
            &ctx(),
            FsReadReq {
                path: temp.path().join("outside_denied.txt").display().to_string(),
            },
        );

        let conn = open_connection(&db_path).expect("open db");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM syscall_audit_log", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 2);

        let decisions = {
            let mut stmt = conn
                .prepare("SELECT decision FROM syscall_audit_log ORDER BY id")
                .expect("prepare");
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query");
            let mut out = Vec::new();
            for row in rows {
                out.push(row.expect("row"));
            }
            out
        };
        assert_eq!(decisions, vec!["allow".to_string(), "deny".to_string()]);

        let denied_code: Option<String> = conn
            .query_row(
                "SELECT error_code FROM syscall_audit_log WHERE decision = 'deny' LIMIT 1",
                params![],
                |row| row.get(0),
            )
            .expect("deny code");
        assert_eq!(denied_code.as_deref(), Some("EACCES"));
    }

    #[test]
    fn kernel_allows_and_denies_proc_exec_by_policy() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("root dir");

        let policy = r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "allow-proc-echo"
        effect: allow
        syscall: "proc.exec"
        resource:
          command_allowlist:
            - "echo"
"#;

        let engine = PolicyEngine::new(serde_yaml::from_str::<PolicyConfig>(policy).expect("yaml"));
        let provider = HostFsProvider::new(&root).expect("provider");
        let process_provider = HostProcessProvider::new(vec![AllowedCommand {
            binary: "echo".to_string(),
            allowed_args: None,
        }])
        .expect("process provider");

        let kernel = Kernel::new_with_providers(
            engine,
            Arc::new(provider),
            Arc::new(process_provider),
            Arc::new(super::DenyNetworkProvider),
        );

        let allowed = kernel
            .proc_exec(
                &ctx(),
                ProcExecReq {
                    command: "echo".to_string(),
                    args: vec!["hello".to_string()],
                },
            )
            .expect("proc allowed");
        assert_eq!(allowed.exit_code, 0);

        let denied = kernel
            .proc_exec(
                &ctx(),
                ProcExecReq {
                    command: "cat".to_string(),
                    args: vec!["/etc/passwd".to_string()],
                },
            )
            .expect_err("proc denied");
        assert_eq!(denied.code.as_str(), "EACCES");
    }

    #[test]
    fn kernel_allows_and_denies_net_http_by_policy() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("root dir");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let host = addr.ip().to_string();
        let url = format!("http://{}/", addr);

        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                let _ = stream.flush();
            }
        });

        let policy = format!(
            "version: 1
defaults:
  decision: deny
principals:
  - id: \"agent:planner@node-a.local\"
    rules:
      - id: \"allow-net-local\"
        effect: allow
        syscall: \"net.http\"
        resource:
          host_allowlist:
            - \"{}\"
          methods:
            - \"GET\"
",
            host
        );

        let engine =
            PolicyEngine::new(serde_yaml::from_str::<PolicyConfig>(&policy).expect("yaml"));
        let fs_provider = HostFsProvider::new(&root).expect("provider");
        let network_provider = HostNetworkProvider::new(
            HashSet::from([host.clone()]),
            HashSet::from(["GET".to_string()]),
            2_000,
        )
        .expect("network provider");

        let kernel = Kernel::new_with_providers(
            engine,
            Arc::new(fs_provider),
            Arc::new(super::DenyProcessProvider),
            Arc::new(network_provider),
        );

        let allowed = kernel
            .net_http(
                &ctx(),
                NetHttpReq {
                    method: "GET".to_string(),
                    url,
                    body: None,
                    timeout_ms: Some(1_000),
                },
            )
            .expect("http allowed");
        assert_eq!(allowed.status, 200);

        let denied = kernel
            .net_http(
                &ctx(),
                NetHttpReq {
                    method: "GET".to_string(),
                    url: "http://example.com/".to_string(),
                    body: None,
                    timeout_ms: Some(500),
                },
            )
            .expect_err("http denied");
        assert_eq!(denied.code.as_str(), "EACCES");

        let _ = server.join();
    }

    fn load_policy(raw: &str) -> String {
        raw.to_string()
    }

    fn ctx() -> ExecutionContext {
        ExecutionContext {
            trace_id: "trc_kernel_fs_1".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: None,
        }
    }
}
