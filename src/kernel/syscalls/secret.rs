use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;

#[derive(Debug, Clone)]
pub struct SecretGetReq {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SecretGetResp {
    pub value: String,
}

pub trait SecretProvider: Send + Sync {
    fn get(
        &self,
        _ctx: &ExecutionContext,
        _req: SecretGetReq,
    ) -> Result<SecretGetResp, KernelError>;
}
