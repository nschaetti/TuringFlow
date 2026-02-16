use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use turingflow::kernel::context::ExecutionContext;
use turingflow::kernel::policy::{PolicyConfig, PolicyEngine};
use turingflow::kernel::syscalls::fs::{FsReadReq, FsWriteReq, HostFsProvider};
use turingflow::kernel::Kernel;

#[derive(Clone)]
pub struct ToolRuntime {
    kernel: Arc<Kernel>,
    root: PathBuf,
    agent_ref: String,
}

impl ToolRuntime {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let root = std::env::current_dir()?;
        let root = std::fs::canonicalize(root)?;
        let agent_ref = "cli@local".to_string();

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
      - id: \"allow-fs-write\"
        effect: allow
        syscall: \"fs.write\"
        resource:
          path_prefix:
            - \"{}\"
      - id: \"allow-fs-list\"
        effect: allow
        syscall: \"fs.list\"
        resource:
          path_prefix:
            - \"{}\"
",
            agent_ref,
            root.display(),
            root.display(),
            root.display()
        );

        let config: PolicyConfig = serde_yaml::from_str(&policy_yaml)?;
        config.validate()?;

        let policy = PolicyEngine::new(config);
        let fs_provider = Arc::new(HostFsProvider::new(&root)?);
        let kernel = Arc::new(Kernel::new(policy, fs_provider));

        Ok(Self {
            kernel,
            root,
            agent_ref,
        })
    }

    pub fn read_bytes(
        &self,
        path: impl AsRef<Path>,
        tool_id: Option<&str>,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let normalized = self.normalize_path(path.as_ref())?;
        let response = self.kernel.fs_read(
            &self.context(tool_id),
            FsReadReq {
                path: normalized.display().to_string(),
            },
        )?;
        Ok(response.content)
    }

    pub fn write_bytes(
        &self,
        path: impl AsRef<Path>,
        content: Vec<u8>,
        tool_id: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        let normalized = self.normalize_path(path.as_ref())?;
        self.kernel.fs_write(
            &self.context(tool_id),
            FsWriteReq {
                path: normalized.display().to_string(),
                content,
            },
        )?;
        Ok(())
    }

    fn normalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(self.root.join(path))
        }
    }

    fn context(&self, tool_id: Option<&str>) -> ExecutionContext {
        ExecutionContext {
            trace_id: format!("cli_{}", tool_id.unwrap_or("runtime")),
            kingdom_id: "kingdom-local".to_string(),
            agent_ref: self.agent_ref.clone(),
            tool_id: tool_id.map(ToString::to_string),
        }
    }
}
