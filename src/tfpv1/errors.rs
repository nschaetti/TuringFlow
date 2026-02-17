//! Standardized TFPv1 HTTP error payload builders.

use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::tfpv1::types::TFPV1_VERSION;

/// Embedded error body.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable description.
    pub message: String,
    /// Whether clients may retry safely.
    pub retryable: bool,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Top-level error response envelope.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Protocol version.
    pub version: &'static str,
    /// Embedded error.
    pub error: ErrorBody,
}

/// Builds a standard error response without details.
pub fn response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            version: TFPV1_VERSION,
            error: ErrorBody {
                code,
                message: message.into(),
                retryable,
                details: None,
            },
        }),
    )
}

/// Builds a standard error response with structured details.
pub fn response_with_details(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
    details: Value,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            version: TFPV1_VERSION,
            error: ErrorBody {
                code,
                message: message.into(),
                retryable,
                details: Some(details),
            },
        }),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::response;

    #[test]
    fn error_response_has_contract_fields() {
        let (status, body) = response(StatusCode::BAD_REQUEST, "invalid_payload", "oops", false);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0.version, "TFPv1");
        assert_eq!(body.0.error.code, "invalid_payload");
        assert_eq!(body.0.error.message, "oops");
        assert!(!body.0.error.retryable);
    }
}
