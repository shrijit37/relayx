use std::fmt;

use http::StatusCode;
use thiserror::Error;

use protocol_core::error::ProtocolEngineError;

/// Typed gateway error hierarchy.
///
/// Each variant maps to a stable HTTP status code.
/// Upstream status codes are preserved when the gateway is proxying
/// an upstream HTTP error (see `UpstreamHttp`).
#[derive(Debug, Error)]
pub enum GatewayError {
    /// Client sent an invalid request (malformed, missing required fields, etc.)
    #[error("invalid request: {message}")]
    InvalidRequest { status: StatusCode, message: String },

    /// Failed to establish a TCP connection to the upstream.
    #[error("upstream connection failed for '{upstream}': {reason}")]
    UpstreamConnection { upstream: String, reason: String },

    /// Request timed out while waiting for the upstream.
    #[error("upstream timeout for '{upstream}' after {elapsed:?}")]
    UpstreamTimeout {
        upstream: String,
        elapsed: std::time::Duration,
    },

    /// Upstream returned a non-2xx HTTP status.
    /// The status code and optional body are forwarded to the client.
    #[error("upstream HTTP {status} from '{upstream}'")]
    UpstreamHttp {
        upstream: String,
        status: StatusCode,
    },

    /// Upstream sent malformed or unexpected protocol data.
    #[error("upstream protocol error for '{upstream}': {message}")]
    UpstreamProtocol { upstream: String, message: String },

    /// Client disconnected before the response was fully sent.
    #[error("client cancelled the request")]
    ClientCancelled,

    /// An internal gateway error that should never happen.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convert a protocol engine error into a gateway error.
///
/// The mapping preserves HTTP status semantics and avoids leaking internal
/// details: only the user-facing message is forwarded.
impl From<ProtocolEngineError> for GatewayError {
    fn from(e: ProtocolEngineError) -> Self {
        match e {
            ProtocolEngineError::UnsupportedProtocol { protocol } => GatewayError::InvalidRequest {
                status: StatusCode::BAD_REQUEST,
                message: format!("unsupported protocol: {protocol}"),
            },
            ProtocolEngineError::InvalidPayload { message } => GatewayError::InvalidRequest {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            ProtocolEngineError::UnsupportedFeature { feature, reason } => {
                GatewayError::InvalidRequest {
                    status: StatusCode::NOT_IMPLEMENTED,
                    message: format!("unsupported feature '{feature}': {reason}"),
                }
            }
            ProtocolEngineError::InvalidStreamEvent { message } => GatewayError::UpstreamProtocol {
                upstream: "upstream".into(),
                message,
            },
            ProtocolEngineError::TranslationFailure { message } => GatewayError::Internal(message),
            ProtocolEngineError::LossyTranslation {
                feature,
                reason,
                policy,
            } => GatewayError::InvalidRequest {
                status: StatusCode::BAD_REQUEST,
                message: format!(
                    "translation rejected for '{feature}' (policy: {policy:?}): {reason}"
                ),
            },
            ProtocolEngineError::ProviderError { message, .. } => GatewayError::UpstreamProtocol {
                upstream: "upstream".into(),
                message,
            },
            ProtocolEngineError::Internal(msg) => GatewayError::Internal(msg),
        }
    }
}

impl GatewayError {
    /// HTTP status code to return to the client.
    pub fn status_code(&self) -> StatusCode {
        match self {
            GatewayError::InvalidRequest { status, .. } => *status,
            GatewayError::UpstreamConnection { .. } => StatusCode::BAD_GATEWAY,
            GatewayError::UpstreamTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            GatewayError::UpstreamHttp { status, .. } => *status,
            GatewayError::UpstreamProtocol { .. } => StatusCode::BAD_GATEWAY,
            GatewayError::ClientCancelled => {
                // 499 Client Closed Request (nginx convention).
                StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
            }
            GatewayError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Classification for metrics and structured logging.
    pub fn category(&self) -> ErrorCategory {
        match self {
            GatewayError::InvalidRequest { .. } => ErrorCategory::ClientError,
            GatewayError::UpstreamConnection { .. } => ErrorCategory::NetworkError,
            GatewayError::UpstreamTimeout { .. } => ErrorCategory::Timeout,
            GatewayError::UpstreamHttp { .. } => ErrorCategory::ProviderRejection,
            GatewayError::UpstreamProtocol { .. } => ErrorCategory::NetworkError,
            GatewayError::ClientCancelled => ErrorCategory::Cancelled,
            GatewayError::Internal(_) => ErrorCategory::Internal,
        }
    }
}

/// High-level error classification for metrics labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    ClientError,
    ProviderRejection,
    NetworkError,
    Timeout,
    Cancelled,
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            ErrorCategory::ClientError => "client_error",
            ErrorCategory::ProviderRejection => "provider_rejection",
            ErrorCategory::NetworkError => "network_error",
            ErrorCategory::Timeout => "timeout",
            ErrorCategory::Cancelled => "cancelled",
            ErrorCategory::Internal => "internal",
        };
        f.write_str(label)
    }
}

/// JSON error response body returned to the client.
#[derive(serde::Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(serde::Serialize)]
pub struct ErrorBody {
    pub status: u16,
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

impl GatewayError {
    /// Build a JSON response body for this error.
    pub fn to_json_body(&self) -> String {
        let status = self.status_code();
        let (error_type, message, upstream) = match self {
            GatewayError::InvalidRequest { message, .. } => {
                ("invalid_request", message.clone(), None)
            }
            GatewayError::UpstreamConnection { upstream, reason } => (
                "upstream_connection",
                reason.clone(),
                Some(upstream.clone()),
            ),
            GatewayError::UpstreamTimeout {
                upstream, elapsed, ..
            } => (
                "upstream_timeout",
                format!("timeout after {elapsed:?}"),
                Some(upstream.clone()),
            ),
            GatewayError::UpstreamHttp {
                upstream, status, ..
            } => (
                "upstream_http_error",
                format!("upstream returned HTTP {status}"),
                Some(upstream.clone()),
            ),
            GatewayError::UpstreamProtocol { upstream, message } => {
                ("upstream_protocol", message.clone(), Some(upstream.clone()))
            }
            GatewayError::ClientCancelled => {
                ("client_cancelled", "client disconnected".into(), None)
            }
            GatewayError::Internal(msg) => ("internal", msg.clone(), None),
        };

        let body = ErrorResponse {
            error: ErrorBody {
                status: status.as_u16(),
                error: error_type.into(),
                message,
                upstream,
            },
        };
        match serde_json::to_string(&body) {
            Ok(json) => json,
            Err(err) => {
                tracing::error!(error = %err, "failed to serialize error response");
                String::from(
                    r#"{"error":{"status":500,"error":"internal","message":"failed to serialize error response"}}"#,
                )
            }
        }
    }
}

/// Implement IntoResponse so GatewayError can be returned directly from handlers.
impl axum::response::IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = self.to_json_body();

        tracing::warn!(
            error = %self,
            status = %status,
            category = %self.category(),
            "gateway error"
        );

        metrics::counter!("relayx_errors_total", "category" => self.category().to_string())
            .increment(1);

        (
            status,
            [(http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_codes() {
        let cases = vec![
            (
                GatewayError::InvalidRequest {
                    status: StatusCode::BAD_REQUEST,
                    message: "bad".into(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                GatewayError::UpstreamConnection {
                    upstream: "x".into(),
                    reason: "refused".into(),
                },
                StatusCode::BAD_GATEWAY,
            ),
            (
                GatewayError::UpstreamTimeout {
                    upstream: "x".into(),
                    elapsed: std::time::Duration::from_secs(30),
                },
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                GatewayError::Internal("oops".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(err.status_code(), expected);
        }
    }

    #[test]
    fn test_json_body_contains_fields() -> anyhow::Result<()> {
        let err = GatewayError::UpstreamTimeout {
            upstream: "test-provider".into(),
            elapsed: std::time::Duration::from_millis(5000),
        };
        let body = err.to_json_body();
        let parsed: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(parsed["error"]["status"], 504);
        assert_eq!(parsed["error"]["error"], "upstream_timeout");
        assert_eq!(
            parsed["error"]["upstream"],
            serde_json::Value::String("test-provider".into())
        );
        Ok(())
    }
}
