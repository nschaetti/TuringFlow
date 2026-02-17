use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable error codes returned by kernel syscalls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelErrorCode {
    /// Permission denied.
    Eacces,
    /// Resource not found.
    Enoent,
    /// Invalid input.
    Einval,
    /// Timeout.
    Etimeout,
    /// Rate-limit reached.
    Eratelimit,
    /// Internal/unknown error.
    Einternal,
}

impl KernelErrorCode {
    /// Converts the error code to an OS-like uppercase token.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Eacces => "EACCES",
            Self::Enoent => "ENOENT",
            Self::Einval => "EINVAL",
            Self::Etimeout => "ETIMEOUT",
            Self::Eratelimit => "ERATELIMIT",
            Self::Einternal => "EINTERNAL",
        }
    }
}

/// Kernel syscall error with retry semantics.
#[derive(Debug, Clone)]
pub struct KernelError {
    /// Machine-readable error code.
    pub code: KernelErrorCode,
    /// Human-readable explanation.
    pub message: String,
    /// Whether retrying might succeed without changing inputs.
    pub retryable: bool,
}

impl KernelError {
    /// Creates an access denied (`EACCES`) error.
    pub fn access_denied(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Eacces,
            message: message.into(),
            retryable: false,
        }
    }

    /// Creates an invalid-input (`EINVAL`) error.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Einval,
            message: message.into(),
            retryable: false,
        }
    }

    /// Creates a not-found (`ENOENT`) error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Enoent,
            message: message.into(),
            retryable: false,
        }
    }

    /// Creates an internal (`EINTERNAL`) error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Einternal,
            message: message.into(),
            retryable: true,
        }
    }
}

impl Display for KernelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl Error for KernelError {}
