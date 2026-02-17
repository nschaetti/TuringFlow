use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;

/// Request payload for secret retrieval.
#[derive(Debug, Clone)]
pub struct SecretGetReq {
    /// Logical secret name.
    pub name: String,
}

/// Response payload for secret retrieval.
#[derive(Debug, Clone)]
pub struct SecretGetResp {
    /// Secret value.
    pub value: String,
}

/// Secret provider abstraction.
///
/// Implementations must be thread-safe (`Send + Sync`) because providers are
/// shared between concurrent kernel requests.
pub trait SecretProvider: Send + Sync {
    /// Reads a secret by name.
    fn get(
        &self,
        _ctx: &ExecutionContext,
        _req: SecretGetReq,
    ) -> Result<SecretGetResp, KernelError>;
}
