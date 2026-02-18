use std::collections::HashSet;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::Method;

use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;

/// Request payload for `net.http`.
#[derive(Debug, Clone)]
pub struct NetHttpReq {
    /// HTTP method (for example `GET`, `POST`).
    pub method: String,
    /// Absolute URL.
    pub url: String,
    /// Optional raw body.
    pub body: Option<Vec<u8>>,
    /// Optional request timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// Response payload for `net.http`.
#[derive(Debug, Clone)]
pub struct NetHttpResp {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Raw response body bytes.
    pub body: Vec<u8>,
}

/// Network syscall provider.
pub trait NetworkProvider: Send + Sync {
    /// Executes an HTTP request.
    fn http(&self, _ctx: &ExecutionContext, _req: NetHttpReq) -> Result<NetHttpResp, KernelError>;
}

/// Host-backed HTTP provider with explicit method/host allowlists.
#[derive(Debug, Clone)]
pub struct HostNetworkProvider {
    client: Client,
    allowed_hosts: HashSet<String>,
    allowed_methods: HashSet<String>,
    timeout_ms_max: u64,
    default_timeout_ms: u64,
}

impl HostNetworkProvider {
    /// Builds a provider constrained by allowed hosts/methods.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError`] when the allowlists are empty, timeout bounds are
    /// invalid, or the underlying HTTP client cannot be built.
    pub fn new(
        allowed_hosts: HashSet<String>,
        allowed_methods: HashSet<String>,
        timeout_ms_max: u64,
    ) -> Result<Self, KernelError> {
        if allowed_hosts.is_empty() {
            return Err(KernelError::invalid(
                "network host allowlist must not be empty",
            ));
        }
        if allowed_methods.is_empty() {
            return Err(KernelError::invalid(
                "network method allowlist must not be empty",
            ));
        }
        if timeout_ms_max == 0 {
            return Err(KernelError::invalid("timeout_ms_max must be > 0"));
        }

        let client = Client::builder().build().map_err(|error| {
            KernelError::internal(format!("failed to build HTTP client: {error}"))
        })?;

        Ok(Self {
            client,
            allowed_hosts,
            allowed_methods: allowed_methods
                .into_iter()
                .map(|method| method.to_uppercase())
                .collect(),
            timeout_ms_max,
            default_timeout_ms: timeout_ms_max.min(3_000),
        })
    }

    fn validate(&self, req: &NetHttpReq) -> Result<(Method, reqwest::Url, Duration), KernelError> {
        let method_name = req.method.to_uppercase();
        if !self.allowed_methods.contains(&method_name) {
            return Err(KernelError::access_denied(format!(
                "HTTP method '{}' is not allowlisted",
                req.method
            )));
        }

        let url = reqwest::Url::parse(&req.url)
            .map_err(|error| KernelError::invalid(format!("invalid URL '{}': {error}", req.url)))?;
        let host = url
            .host_str()
            .ok_or_else(|| KernelError::invalid("URL host is required"))?;

        if !self.allowed_hosts.contains(host) {
            return Err(KernelError::access_denied(format!(
                "host '{}' is not allowlisted",
                host
            )));
        }

        let timeout_ms = req.timeout_ms.unwrap_or(self.default_timeout_ms);
        if timeout_ms == 0 || timeout_ms > self.timeout_ms_max {
            return Err(KernelError::invalid(format!(
                "timeout_ms must be in range 1..={} ms",
                self.timeout_ms_max
            )));
        }

        let method = Method::from_bytes(method_name.as_bytes()).map_err(|_| {
            KernelError::invalid(format!(
                "invalid HTTP method '{}': {}",
                req.method, method_name
            ))
        })?;

        Ok((method, url, Duration::from_millis(timeout_ms)))
    }
}

impl NetworkProvider for HostNetworkProvider {
    fn http(&self, _ctx: &ExecutionContext, req: NetHttpReq) -> Result<NetHttpResp, KernelError> {
        let (method, url, timeout) = self.validate(&req)?;

        let mut request = self.client.request(method, url).timeout(timeout);
        if let Some(body) = req.body {
            request = request.body(body);
        }

        let response = request.send().map_err(|error| {
            if error.is_timeout() {
                KernelError {
                    code: crate::kernel::errors::KernelErrorCode::Etimeout,
                    message: format!("HTTP request timeout: {error}"),
                    retryable: true,
                }
            } else {
                KernelError::internal(format!("HTTP request failed: {error}"))
            }
        })?;

        let status = response.status().as_u16();
        let body = response
            .bytes()
            .map_err(|error| {
                KernelError::internal(format!("failed to read response body: {error}"))
            })?
            .to_vec();

        Ok(NetHttpResp { status, body })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{HostNetworkProvider, NetHttpReq, NetworkProvider};
    use crate::kernel::context::ExecutionContext;

    #[test]
    fn rejects_forbidden_host() {
        let provider = HostNetworkProvider::new(
            HashSet::from(["example.com".to_string()]),
            HashSet::from(["GET".to_string()]),
            5_000,
        )
        .expect("provider");

        let err = provider
            .http(
                &ctx(),
                NetHttpReq {
                    method: "GET".to_string(),
                    url: "https://not-allowed.example/api".to_string(),
                    body: None,
                    timeout_ms: Some(500),
                },
            )
            .expect_err("must reject host");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    #[test]
    fn rejects_forbidden_method() {
        let provider = HostNetworkProvider::new(
            HashSet::from(["example.com".to_string()]),
            HashSet::from(["GET".to_string()]),
            5_000,
        )
        .expect("provider");

        let err = provider
            .http(
                &ctx(),
                NetHttpReq {
                    method: "POST".to_string(),
                    url: "https://example.com/api".to_string(),
                    body: Some(vec![1, 2, 3]),
                    timeout_ms: Some(500),
                },
            )
            .expect_err("must reject method");

        assert_eq!(err.code.as_str(), "EACCES");
    }

    fn ctx() -> ExecutionContext {
        ExecutionContext {
            trace_id: "trc_net_1".to_string(),
            span_id: None,
            parent_span_id: None,
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: Some("http".to_string()),
        }
    }
}
