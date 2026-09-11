//! Protocol-core error types.
//!
//! Typed errors for the protocol engine. Each variant maps to an HTTP status
//! code and provides structured information for clients and logging.
//!
//! The `IntoResponse` implementation lives in the gateway crate to keep
//! protocol-core free of axum/metrics dependencies.

use http::StatusCode;
use thiserror::Error;

use crate::canonical::LossPolicy;

/// Protocol engine errors, distinct from gateway-level GatewayError.
#[derive(Debug, Error)]
pub enum ProtocolEngineError {
    /// The detected protocol is not supported.
    #[error("unsupported protocol: {protocol}")]
    UnsupportedProtocol { protocol: String },

    /// The request payload could not be decoded.
    #[error("invalid payload: {message}")]
    InvalidPayload { message: String },

    /// A required feature is not supported by the target adapter.
    #[error("unsupported feature '{feature}': {reason}")]
    UnsupportedFeature { feature: String, reason: String },

    /// A stream event could not be parsed or translated.
    #[error("invalid stream event: {message}")]
    InvalidStreamEvent { message: String },

    /// Translation between protocols failed.
    #[error("translation failure: {message}")]
    TranslationFailure { message: String },

    /// A feature was lost during translation.
    #[error("lossy translation for '{feature}': {reason}")]
    LossyTranslation {
        feature: String,
        reason: String,
        policy: LossPolicy,
    },

    /// The upstream provider returned an error.
    #[error("provider error: {message}")]
    ProviderError { message: String },

    /// An internal error that should never happen.
    #[error("internal protocol error: {0}")]
    Internal(String),
}

impl ProtocolEngineError {
    /// HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            ProtocolEngineError::UnsupportedProtocol { .. } => StatusCode::BAD_REQUEST,
            ProtocolEngineError::InvalidPayload { .. } => StatusCode::BAD_REQUEST,
            ProtocolEngineError::UnsupportedFeature { .. } => StatusCode::NOT_IMPLEMENTED,
            ProtocolEngineError::InvalidStreamEvent { .. } => StatusCode::BAD_GATEWAY,
            ProtocolEngineError::TranslationFailure { .. } => StatusCode::BAD_GATEWAY,
            ProtocolEngineError::LossyTranslation {
                policy: LossPolicy::Reject,
                ..
            } => StatusCode::BAD_REQUEST,
            ProtocolEngineError::LossyTranslation { .. } => StatusCode::OK,
            ProtocolEngineError::ProviderError { .. } => StatusCode::BAD_GATEWAY,
            ProtocolEngineError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Classification for metrics labels.
    pub fn category(&self) -> &'static str {
        match self {
            ProtocolEngineError::UnsupportedProtocol { .. } => "unsupported_protocol",
            ProtocolEngineError::InvalidPayload { .. } => "invalid_payload",
            ProtocolEngineError::UnsupportedFeature { .. } => "unsupported_feature",
            ProtocolEngineError::InvalidStreamEvent { .. } => "invalid_stream_event",
            ProtocolEngineError::TranslationFailure { .. } => "translation_failure",
            ProtocolEngineError::LossyTranslation { .. } => "lossy_translation",
            ProtocolEngineError::ProviderError { .. } => "provider_error",
            ProtocolEngineError::Internal(_) => "internal",
        }
    }
}

/// JSON error response body for protocol errors.
#[derive(serde::Serialize)]
pub struct ProtocolErrorResponse {
    pub error: ProtocolErrorBody,
}

#[derive(serde::Serialize)]
pub struct ProtocolErrorBody {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    pub status: u16,
}

impl ProtocolEngineError {
    /// Build a JSON response body for this error.
    pub fn to_json_body(&self) -> String {
        let status = self.status_code();
        let error_type = match self {
            ProtocolEngineError::UnsupportedProtocol { .. } => "unsupported_protocol",
            ProtocolEngineError::InvalidPayload { .. } => "invalid_payload",
            ProtocolEngineError::UnsupportedFeature { .. } => "unsupported_feature",
            ProtocolEngineError::InvalidStreamEvent { .. } => "invalid_stream_event",
            ProtocolEngineError::TranslationFailure { .. } => "translation_failure",
            ProtocolEngineError::LossyTranslation { .. } => "lossy_translation",
            ProtocolEngineError::ProviderError { .. } => "provider_error",
            ProtocolEngineError::Internal(_) => "internal",
        };

        let body = ProtocolErrorResponse {
            error: ProtocolErrorBody {
                error_type: error_type.into(),
                message: self.to_string(),
                status: status.as_u16(),
            },
        };

        serde_json::to_string(&body).unwrap_or_else(|_| {
            r#"{"error":{"type":"internal","message":"failed to serialize error response","status":500}}"#
                .into()
        })
    }
}
