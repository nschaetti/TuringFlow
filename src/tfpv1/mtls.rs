//! mTLS helpers for the daemon.

use std::error::Error;
use std::fs;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pemfile::Item;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

/// Builds rustls server config with mandatory client certificate verification.
pub fn build_server_config(
    server_cert_path: &str,
    server_key_path: &str,
    client_ca_cert_path: &str,
) -> Result<Arc<ServerConfig>, Box<dyn Error>> {
    let cert_bytes = fs::read(server_cert_path)?;
    let key_bytes = fs::read(server_key_path)?;
    let ca_bytes = fs::read(client_ca_cert_path)?;

    let cert_chain: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_bytes.as_ref()).collect::<Result<Vec<_>, _>>()?;
    if cert_chain.is_empty() {
        return Err("server certificate chain is empty".into());
    }

    let mut keys: Vec<PrivateKeyDer<'static>> = rustls_pemfile::read_all(&mut key_bytes.as_ref())
        .filter_map(|item| match item.ok()? {
            Item::Pkcs8Key(key) => Some(PrivateKeyDer::Pkcs8(key)),
            Item::Pkcs1Key(key) => Some(PrivateKeyDer::Pkcs1(key)),
            Item::Sec1Key(key) => Some(PrivateKeyDer::Sec1(key)),
            _ => None,
        })
        .collect();
    if keys.len() != 1 {
        return Err("server key file must contain exactly one private key".into());
    }
    let private_key = keys.remove(0);

    let client_ca_certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut ca_bytes.as_ref()).collect::<Result<Vec<_>, _>>()?;
    if client_ca_certs.is_empty() {
        return Err("client CA certificate file is empty".into());
    }

    let mut roots = RootCertStore::empty();
    for cert in client_ca_certs {
        roots.add(cert)?;
    }

    let verifier = WebPkiClientVerifier::builder(Arc::new(roots)).build()?;

    let mut config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(cert_chain, private_key)?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(Arc::new(config))
}

/// Extracts normalized node identity from client certificate.
///
/// The function prefers SAN DNS names and falls back to subject CN.
pub fn extract_node_id_from_cert(cert_der: &[u8]) -> Option<String> {
    let (_, cert) = X509Certificate::from_der(cert_der).ok()?;

    if let Ok(Some(san)) = cert.subject_alternative_name() {
        for name in &san.value.general_names {
            if let GeneralName::DNSName(dns) = name {
                let value = dns.trim().to_ascii_lowercase();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }

    for cn in cert.subject().iter_common_name() {
        if let Ok(value) = cn.attr_value().as_str() {
            let normalized = value.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::extract_node_id_from_cert;

    #[test]
    fn invalid_certificate_returns_none() {
        assert!(extract_node_id_from_cert(b"not-a-certificate").is_none());
    }
}
