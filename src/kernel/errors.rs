use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelErrorCode {
    Eacces,
    Enoent,
    Einval,
    Etimeout,
    Eratelimit,
    Einternal,
}

impl KernelErrorCode {
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

#[derive(Debug, Clone)]
pub struct KernelError {
    pub code: KernelErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl KernelError {
    pub fn access_denied(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Eacces,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Einval,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: KernelErrorCode::Enoent,
            message: message.into(),
            retryable: false,
        }
    }

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
