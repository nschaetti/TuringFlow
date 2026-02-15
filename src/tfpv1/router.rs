use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::collections::HashMap;

use reqwest::{Client, StatusCode};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::time::{sleep, Duration, Instant};

use crate::tfpv1::types::{
    AckRequest, AckResponse, Envelope, RouteHop, SendResponse, TFPV1_VERSION,
};

#[derive(Debug, Clone)]
pub struct DestinationRoute {
    pub agent_ref: String,
    pub deliver_url: String,
}

#[derive(Debug)]
pub struct Router {
    client: Client,
    daemon_node: String,
    acks: HashMap<String, AckRequest>,
    seq: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ClientTlsConfig {
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
}

impl Router {
    pub fn new(
        daemon_node: impl Into<String>,
        tls_config: ClientTlsConfig,
    ) -> Result<Self, RouterConfigError> {
        let client = build_http_client(&tls_config)?;
        Ok(Self {
            client,
            daemon_node: daemon_node.into(),
            acks: HashMap::new(),
            seq: 0,
        })
    }

    pub async fn forward_message(
        &mut self,
        mut message: Envelope,
        destination: &DestinationRoute,
    ) -> Result<SendResponse, RouterError> {
        let now = OffsetDateTime::now_utc();
        message.routing.path.push(RouteHop {
            node: self.daemon_node.clone(),
            at: format_rfc3339(now),
        });

        let delivery_id = self.next_delivery_id(now);
        let body = serde_json::json!({
            "version": TFPV1_VERSION,
            "delivery_id": delivery_id,
            "message": message,
        });

        let url = deliver_endpoint(&destination.deliver_url);
        let deadline = Instant::now() + Duration::from_millis(message.ttl_ms);
        let delays = [Duration::from_millis(0), Duration::from_millis(250), Duration::from_secs(1), Duration::from_secs(3)];

        let mut last_status: Option<StatusCode> = None;
        let mut last_error: Option<String> = None;

        for delay in delays {
            if delay > Duration::from_millis(0) {
                if Instant::now() >= deadline {
                    return Err(RouterError::DeliveryTimeout);
                }
                sleep(delay).await;
            }

            if Instant::now() >= deadline {
                return Err(RouterError::DeliveryTimeout);
            }

            match self.client.post(&url).json(&body).send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(SendResponse {
                        version: TFPV1_VERSION.to_string(),
                        accepted: true,
                        delivery_id: body["delivery_id"].as_str().unwrap_or_default().to_string(),
                        status: "forwarded".to_string(),
                        destination: destination.agent_ref.clone(),
                    });
                }
                Ok(response) => {
                    last_status = Some(response.status());
                    last_error = response.text().await.ok();
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                }
            }
        }

        Err(RouterError::DestinationUnreachable {
            status: last_status.map(|s| s.as_u16()),
            details: last_error,
        })
    }

    pub fn record_ack(&mut self, ack: AckRequest) -> AckResponse {
        self.acks.insert(ack.delivery_id.clone(), ack);
        AckResponse {
            version: TFPV1_VERSION.to_string(),
            accepted: true,
        }
    }

    fn next_delivery_id(&mut self, now: OffsetDateTime) -> String {
        self.seq = self.seq.saturating_add(1);
        format!("dlv_{}_{}", now.unix_timestamp_nanos(), self.seq)
    }
}

#[derive(Debug)]
pub enum RouterConfigError {
    MissingClientKey,
    MissingClientCert,
    InvalidCert(String),
    InvalidKey(String),
    BuildClient(String),
    Io(String),
}

impl Display for RouterConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterConfigError::MissingClientKey => {
                write!(f, "upstream-client-key is required when upstream-client-cert is set")
            }
            RouterConfigError::MissingClientCert => {
                write!(f, "upstream-client-cert is required when upstream-client-key is set")
            }
            RouterConfigError::InvalidCert(msg) => write!(f, "invalid certificate: {msg}"),
            RouterConfigError::InvalidKey(msg) => write!(f, "invalid private key: {msg}"),
            RouterConfigError::BuildClient(msg) => write!(f, "failed to build HTTP client: {msg}"),
            RouterConfigError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl Error for RouterConfigError {}

fn build_http_client(tls: &ClientTlsConfig) -> Result<Client, RouterConfigError> {
    let mut builder = Client::builder();

    if let Some(ca_path) = &tls.ca_cert_path {
        let bytes = fs::read(ca_path).map_err(|e| RouterConfigError::Io(e.to_string()))?;
        let cert = reqwest::Certificate::from_pem(&bytes)
            .map_err(|e| RouterConfigError::InvalidCert(e.to_string()))?;
        builder = builder.add_root_certificate(cert);
    }

    match (&tls.client_cert_path, &tls.client_key_path) {
        (Some(cert_path), Some(key_path)) => {
            let cert_bytes = fs::read(cert_path).map_err(|e| RouterConfigError::Io(e.to_string()))?;
            let key_bytes = fs::read(key_path).map_err(|e| RouterConfigError::Io(e.to_string()))?;
            let mut identity_pem = cert_bytes;
            identity_pem.push(b'\n');
            identity_pem.extend_from_slice(&key_bytes);

            let identity = reqwest::Identity::from_pem(&identity_pem)
                .map_err(|e| RouterConfigError::InvalidKey(e.to_string()))?;
            builder = builder.identity(identity);
        }
        (Some(_), None) => return Err(RouterConfigError::MissingClientKey),
        (None, Some(_)) => return Err(RouterConfigError::MissingClientCert),
        (None, None) => {}
    }

    builder
        .build()
        .map_err(|e| RouterConfigError::BuildClient(e.to_string()))
}

#[derive(Debug)]
pub enum RouterError {
    DeliveryTimeout,
    DestinationUnreachable {
        status: Option<u16>,
        details: Option<String>,
    },
}

fn deliver_endpoint(base: &str) -> String {
    if base.ends_with("/tfpv1/deliver") {
        return base.to_string();
    }

    if base.ends_with('/') {
        format!("{}tfpv1/deliver", base)
    } else {
        format!("{}/tfpv1/deliver", base)
    }
}

fn format_rfc3339(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::{ClientTlsConfig, Router, RouterConfigError};

    #[test]
    fn router_rejects_missing_key_when_cert_is_set() {
        let result = Router::new(
            "turingflowd",
            ClientTlsConfig {
                ca_cert_path: None,
                client_cert_path: Some("/tmp/client.crt".to_string()),
                client_key_path: None,
            },
        );

        assert!(matches!(result, Err(RouterConfigError::MissingClientKey)));
    }

    #[test]
    fn router_rejects_missing_cert_when_key_is_set() {
        let result = Router::new(
            "turingflowd",
            ClientTlsConfig {
                ca_cert_path: None,
                client_cert_path: None,
                client_key_path: Some("/tmp/client.key".to_string()),
            },
        );

        assert!(matches!(result, Err(RouterConfigError::MissingClientCert)));
    }
}
