use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;

#[derive(Debug, Clone)]
pub struct FsListReq {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct FsListEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct FsListResp {
    pub entries: Vec<FsListEntry>,
}

#[derive(Debug, Clone)]
pub struct FsReadReq {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct FsReadResp {
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FsWriteReq {
    pub path: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FsWriteResp {
    pub bytes_written: usize,
}

pub trait FsProvider: Send + Sync {
    fn list(&self, _ctx: &ExecutionContext, _req: FsListReq) -> Result<FsListResp, KernelError>;
    fn read(&self, _ctx: &ExecutionContext, _req: FsReadReq) -> Result<FsReadResp, KernelError>;
    fn write(&self, _ctx: &ExecutionContext, _req: FsWriteReq) -> Result<FsWriteResp, KernelError>;
}

#[derive(Debug, Clone)]
pub struct HostFsProvider {
    root: PathBuf,
}

impl HostFsProvider {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, KernelError> {
        let root = root.into();
        let root = fs::canonicalize(&root).map_err(|error| {
            KernelError::invalid(format!("invalid fs root '{}': {error}", root.display()))
        })?;

        if !root.is_dir() {
            return Err(KernelError::invalid(format!(
                "fs root is not a directory: {}",
                root.display()
            )));
        }

        Ok(Self { root })
    }

    fn resolve_existing_path(&self, raw_path: &str) -> Result<PathBuf, KernelError> {
        let candidate = self.to_candidate_path(raw_path)?;
        let canonical = fs::canonicalize(&candidate).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                KernelError::not_found(format!("path not found: {}", candidate.display()))
            } else {
                KernelError::internal(format!(
                    "failed to canonicalize path '{}': {error}",
                    candidate.display()
                ))
            }
        })?;

        self.ensure_within_root(&canonical)?;
        Ok(canonical)
    }

    fn resolve_writable_path(&self, raw_path: &str) -> Result<PathBuf, KernelError> {
        let candidate = self.to_candidate_path(raw_path)?;
        let parent = candidate
            .parent()
            .ok_or_else(|| KernelError::invalid("fs.write path must have a parent directory"))?;

        let canonical_parent = fs::canonicalize(parent).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                KernelError::not_found(format!("parent path not found: {}", parent.display()))
            } else {
                KernelError::internal(format!(
                    "failed to canonicalize parent path '{}': {error}",
                    parent.display()
                ))
            }
        })?;

        self.ensure_within_root(&canonical_parent)?;

        let file_name = candidate
            .file_name()
            .ok_or_else(|| KernelError::invalid("fs.write path must target a file"))?;
        Ok(canonical_parent.join(file_name))
    }

    fn to_candidate_path(&self, raw_path: &str) -> Result<PathBuf, KernelError> {
        if raw_path.trim().is_empty() {
            return Err(KernelError::invalid("path must not be empty"));
        }

        let requested = Path::new(raw_path);
        if requested.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::CurDir | Component::ParentDir
            )
        }) {
            return Err(KernelError::access_denied(
                "path traversal or unsupported component is not allowed",
            ));
        }

        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.root.join(requested)
        };

        Ok(candidate)
    }

    fn ensure_within_root(&self, candidate: &Path) -> Result<(), KernelError> {
        if candidate.starts_with(&self.root) {
            return Ok(());
        }

        Err(KernelError::access_denied(format!(
            "path escapes fs root: {}",
            candidate.display()
        )))
    }
}

impl FsProvider for HostFsProvider {
    fn list(&self, _ctx: &ExecutionContext, req: FsListReq) -> Result<FsListResp, KernelError> {
        let path = self.resolve_existing_path(&req.path)?;
        let mut entries = Vec::new();

        for entry in fs::read_dir(&path).map_err(|error| {
            KernelError::internal(format!("failed to list '{}': {error}", path.display()))
        })? {
            let entry = entry.map_err(|error| {
                KernelError::internal(format!("failed to access dir entry: {error}"))
            })?;
            let file_type = entry.file_type().map_err(|error| {
                KernelError::internal(format!("failed to inspect dir entry type: {error}"))
            })?;
            entries.push(FsListEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: file_type.is_dir(),
            });
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(FsListResp { entries })
    }

    fn read(&self, _ctx: &ExecutionContext, req: FsReadReq) -> Result<FsReadResp, KernelError> {
        let path = self.resolve_existing_path(&req.path)?;
        let content = fs::read(&path).map_err(|error| {
            KernelError::internal(format!("failed to read '{}': {error}", path.display()))
        })?;
        Ok(FsReadResp { content })
    }

    fn write(&self, _ctx: &ExecutionContext, req: FsWriteReq) -> Result<FsWriteResp, KernelError> {
        let path = self.resolve_writable_path(&req.path)?;
        fs::write(&path, &req.content).map_err(|error| {
            KernelError::internal(format!("failed to write '{}': {error}", path.display()))
        })?;
        Ok(FsWriteResp {
            bytes_written: req.content.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FsListReq, FsProvider, FsReadReq, FsWriteReq, HostFsProvider};
    use crate::kernel::context::ExecutionContext;
    use tempfile::TempDir;

    #[test]
    fn allows_read_and_write_within_root() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("create workspace");
        let provider = HostFsProvider::new(&root).expect("provider");
        let ctx = context();

        provider
            .write(
                &ctx,
                FsWriteReq {
                    path: root.join("notes.txt").display().to_string(),
                    content: b"hello".to_vec(),
                },
            )
            .expect("write ok");

        let read = provider
            .read(
                &ctx,
                FsReadReq {
                    path: root.join("notes.txt").display().to_string(),
                },
            )
            .expect("read ok");

        assert_eq!(read.content, b"hello");
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("create workspace");
        let provider = HostFsProvider::new(&root).expect("provider");
        let ctx = context();

        let err = provider
            .read(
                &ctx,
                FsReadReq {
                    path: "../secret.txt".to_string(),
                },
            )
            .expect_err("must reject traversal");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_on_read() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("create workspace");
        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, b"outside").expect("outside file");
        symlink(&outside, root.join("escape.txt")).expect("symlink");

        let provider = HostFsProvider::new(&root).expect("provider");
        let ctx = context();
        let err = provider
            .read(
                &ctx,
                FsReadReq {
                    path: root.join("escape.txt").display().to_string(),
                },
            )
            .expect_err("must reject symlink escape");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[test]
    fn lists_entries_within_root() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(root.join("dir_a")).expect("dir");
        std::fs::write(root.join("file_b.txt"), b"ok").expect("file");
        let provider = HostFsProvider::new(&root).expect("provider");
        let ctx = context();

        let list = provider
            .list(
                &ctx,
                FsListReq {
                    path: root.display().to_string(),
                },
            )
            .expect("list ok");

        let names = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["dir_a".to_string(), "file_b.txt".to_string()]);
    }

    fn context() -> ExecutionContext {
        ExecutionContext {
            trace_id: "trc_fs_1".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: Some("fs".to_string()),
        }
    }
}
