use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::tfpv1::types::TFPV1_VERSION;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub version: &'static str,
    pub error: ErrorBody,
}

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
