use std::collections::{HashMap, HashSet};
use std::process::Command;

use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;

/// Request payload for `proc.exec`.
#[derive(Debug, Clone)]
pub struct ProcExecReq {
    /// Binary name (not a path).
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
}

/// Response payload for `proc.exec`.
#[derive(Debug, Clone)]
pub struct ProcExecResp {
    /// Process exit code (`-1` when unavailable).
    pub exit_code: i32,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
}

/// Process syscall provider.
pub trait ProcessProvider: Send + Sync {
    /// Executes a command.
    fn exec(&self, _ctx: &ExecutionContext, _req: ProcExecReq)
        -> Result<ProcExecResp, KernelError>;
}

/// Allowlist entry for one executable.
#[derive(Debug, Clone)]
pub struct AllowedCommand {
    /// Binary name (`cargo`, `echo`, ...).
    pub binary: String,
    /// Optional explicit argument allowlist.
    pub allowed_args: Option<HashSet<String>>,
}

/// Host-backed process provider with strict command/arg validation.
#[derive(Debug, Clone)]
pub struct HostProcessProvider {
    allowed: HashMap<String, Option<HashSet<String>>>,
    max_args: usize,
    max_arg_len: usize,
}

impl HostProcessProvider {
    /// Creates a provider from an executable allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError`] when the allowlist is empty or contains invalid
    /// / shell binaries.
    pub fn new(allowed_commands: Vec<AllowedCommand>) -> Result<Self, KernelError> {
        if allowed_commands.is_empty() {
            return Err(KernelError::invalid(
                "process allowlist must include at least one binary",
            ));
        }

        let mut allowed = HashMap::new();
        for cmd in allowed_commands {
            validate_binary_name(&cmd.binary)?;
            if is_shell_binary(&cmd.binary) {
                return Err(KernelError::invalid(format!(
                    "shell binary '{}' cannot be allowlisted",
                    cmd.binary
                )));
            }
            allowed.insert(cmd.binary, cmd.allowed_args);
        }

        Ok(Self {
            allowed,
            max_args: 32,
            max_arg_len: 1024,
        })
    }

    fn validate_request(&self, req: &ProcExecReq) -> Result<(), KernelError> {
        validate_binary_name(&req.command)?;
        if is_shell_binary(&req.command) {
            return Err(KernelError::access_denied(
                "shell execution is not allowed in proc.exec",
            ));
        }

        let allowed_args = self.allowed.get(&req.command).ok_or_else(|| {
            KernelError::access_denied(format!("command '{}' is not allowlisted", req.command))
        })?;

        if req.args.len() > self.max_args {
            return Err(KernelError::invalid(format!(
                "too many args: {} > {}",
                req.args.len(),
                self.max_args
            )));
        }

        for arg in &req.args {
            if arg.len() > self.max_arg_len {
                return Err(KernelError::invalid(format!(
                    "arg too long: {} > {}",
                    arg.len(),
                    self.max_arg_len
                )));
            }
            if arg.contains('\n') || arg.contains('\r') {
                return Err(KernelError::invalid("args cannot contain newlines"));
            }
            if let Some(allowed_args) = allowed_args {
                if !allowed_args.contains(arg) {
                    return Err(KernelError::access_denied(format!(
                        "arg '{}' is not allowlisted for command '{}'",
                        arg, req.command
                    )));
                }
            }
        }

        Ok(())
    }
}

impl ProcessProvider for HostProcessProvider {
    fn exec(&self, _ctx: &ExecutionContext, req: ProcExecReq) -> Result<ProcExecResp, KernelError> {
        self.validate_request(&req)?;

        let output = Command::new(&req.command)
            .args(&req.args)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    KernelError::not_found(format!("binary not found: {}", req.command))
                } else {
                    KernelError::internal(format!(
                        "failed to execute command '{}': {error}",
                        req.command
                    ))
                }
            })?;

        Ok(ProcExecResp {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn validate_binary_name(binary: &str) -> Result<(), KernelError> {
    if binary.trim().is_empty() {
        return Err(KernelError::invalid("command must not be empty"));
    }
    if binary.contains('/') || binary.contains('\\') {
        return Err(KernelError::access_denied(
            "command must be a binary name, not a path",
        ));
    }
    if binary.contains(char::is_whitespace) {
        return Err(KernelError::access_denied(
            "command must not contain whitespace",
        ));
    }
    Ok(())
}

fn is_shell_binary(binary: &str) -> bool {
    matches!(
        binary,
        "sh" | "bash" | "zsh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{AllowedCommand, HostProcessProvider, ProcExecReq, ProcessProvider};
    use crate::kernel::context::ExecutionContext;

    #[test]
    fn rejects_forbidden_command() {
        let provider = HostProcessProvider::new(vec![AllowedCommand {
            binary: "echo".to_string(),
            allowed_args: None,
        }])
        .expect("provider");

        let err = provider
            .exec(
                &ctx(),
                ProcExecReq {
                    command: "cat".to_string(),
                    args: vec!["/etc/passwd".to_string()],
                },
            )
            .expect_err("must reject command not in allowlist");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[test]
    fn rejects_shell_execution() {
        let provider = HostProcessProvider::new(vec![AllowedCommand {
            binary: "echo".to_string(),
            allowed_args: None,
        }])
        .expect("provider");

        let err = provider
            .exec(
                &ctx(),
                ProcExecReq {
                    command: "sh".to_string(),
                    args: vec!["-c".to_string(), "id".to_string()],
                },
            )
            .expect_err("must reject shell");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[test]
    fn rejects_non_allowlisted_arg() {
        let mut args = HashSet::new();
        args.insert("--version".to_string());
        let provider = HostProcessProvider::new(vec![AllowedCommand {
            binary: "cargo".to_string(),
            allowed_args: Some(args),
        }])
        .expect("provider");

        let err = provider
            .exec(
                &ctx(),
                ProcExecReq {
                    command: "cargo".to_string(),
                    args: vec!["build".to_string()],
                },
            )
            .expect_err("must reject arg");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    fn ctx() -> ExecutionContext {
        ExecutionContext {
            trace_id: "trc_proc_1".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: Some("exec".to_string()),
        }
    }
}
