use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrackerError {
    #[error("Invalid info hash: {0}")]
    InvalidInfoHash(String),

    #[error("Invalid peer ID: {0}")]
    InvalidPeerId(String),

    #[error("Invalid public key: {0}")]
    InvalidPubkey(String),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Insufficient stake: required {required}, got {actual}")]
    InsufficientStake { required: u64, actual: u64 },

    #[error("Low reputation: {0}")]
    LowReputation(i32),

    #[error("Hex decode error: {0}")]
    HexDecode(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for TrackerError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            TrackerError::InvalidInfoHash(_)
            | TrackerError::InvalidPeerId(_)
            | TrackerError::InvalidPubkey(_)
            | TrackerError::HexDecode(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            TrackerError::InvalidSignature => (StatusCode::UNAUTHORIZED, self.to_string()),

            TrackerError::InsufficientStake { .. } | TrackerError::LowReputation(_) => {
                (StatusCode::FORBIDDEN, self.to_string())
            }

            TrackerError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}
