//! Mock upstream application: builds the Axum router for the mock server.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use crate::MockConfig;
use crate::sse::{SseConfig, raw_sse_response, sse_response};

/// App state: config + counters.
pub struct MockAppState {
    pub config: MockConfig,
    pub state: Arc<crate::MockState>,
}

/// Build the mock upstream router.
pub fn build_app(config: MockConfig, state: Arc<crate::MockState>) -> Router {
    let app_state = Arc::new(MockAppState { config, state });

    Router::new()
        .route("/health", get(health))
        .route("/v1/echo", post(echo))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/messages", post(messages))
        .route("/stats", get(stats))
        .with_state(app_state)
}

/// GET /health — simple health check.
async fn health() -> Response {
    axum::Json(serde_json::json!({ "status": "ok" })).into_response()
}

/// POST /v1/echo — echo the request body back as JSON.
async fn echo(
    State(app): State<Arc<MockAppState>>,
    _headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    // Apply configurable TTFB latency.
    if !app.config.ttfb.is_zero() {
        tokio::time::sleep(app.config.ttfb).await;
    }

    let bytes = match http_body_util::BodyExt::collect(body).await {
        Ok(buf) => buf.to_bytes(),
        Err(e) => {
            tracing::warn!(error = %e, "failed to collect echo request body");
            return (
                StatusCode::BAD_REQUEST,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                "body collection failed",
            )
                .into_response();
        }
    };

    app.state
        .requests_served
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Echo the request body as the response body.
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bytes,
    )
        .into_response()
}

/// POST /v1/chat/completions — SSE or JSON based on config mode.
async fn chat_completions(
    State(app): State<Arc<MockAppState>>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    // Apply configurable TTFB latency.
    if !app.config.ttfb.is_zero() {
        tokio::time::sleep(app.config.ttfb).await;
    }

    // Capture the request body for protocol assertions.
    let body_bytes = http_body_util::BodyExt::collect(body)
        .await
        .map(|buf| buf.to_bytes())
        .unwrap_or_default();
    let body_str = String::from_utf8_lossy(&body_bytes).into_owned();
    *app.state
        .last_request_body
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(body_str);

    app.state
        .requests_served
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    match app.config.mode {
        crate::MockMode::Json => {
            let json_body = app.config.json_body.clone();
            let content_length = json_body.len().to_string();
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, "application/json"),
                    (axum::http::header::CONTENT_LENGTH, content_length.as_str()),
                ],
                json_body,
            )
                .into_response()
        }
        crate::MockMode::Sse => {
            // Raw SSE injection: tests provide exact wire chunks.
            if let Some(raw) = &app.config.raw_sse {
                return raw_sse_response(raw.clone()).into_response();
            }
            // Check for X-Mock-Error headers.
            let error_at = headers
                .get("x-mock-error-at")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());
            let error_status = headers
                .get("x-mock-error-status")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(500);

            let sse_config = SseConfig {
                chunks: app.config.chunks,
                chunk_size: app.config.chunk_size,
                chunk_delay: app.config.chunk_delay,
                error_at,
                error_status,
            };
            sse_response(sse_config).into_response()
        }
    }
}

/// GET /stats — expose internal counters for assertions.
async fn stats(State(app): State<Arc<MockAppState>>) -> Response {
    axum::Json(serde_json::json!({
        "requests_served": app.state.requests_served.load(std::sync::atomic::Ordering::Relaxed),
        "connections_accepted": app.state.connections_accepted.load(std::sync::atomic::Ordering::Relaxed),
        "bytes_sent": app.state.bytes_sent.load(std::sync::atomic::Ordering::Relaxed),
    }))
    .into_response()
}

/// POST /v1/messages — Anthropic Messages API mock.
///
/// Returns a deterministic Anthropic Messages response. The gateway
/// decodes this and translates it back to the client-facing protocol.
async fn messages(
    State(app): State<Arc<MockAppState>>,
    _headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !app.config.ttfb.is_zero() {
        tokio::time::sleep(app.config.ttfb).await;
    }

    // Capture the request body (in the target protocol) for assertions.
    let bytes = match http_body_util::BodyExt::collect(body).await {
        Ok(buf) => buf.to_bytes(),
        Err(e) => {
            tracing::warn!(error = %e, "failed to collect messages request body");
            return (
                StatusCode::BAD_REQUEST,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                "body collection failed",
            )
                .into_response();
        }
    };

    let body_str = String::from_utf8_lossy(&bytes).into_owned();
    *app.state
        .last_request_body
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(body_str);

    app.state
        .requests_served
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Raw SSE injection: tests provide exact wire chunks.
    if let Some(raw) = &app.config.raw_sse {
        return raw_sse_response(raw.clone()).into_response();
    }

    let response_body = if !app.config.json_body.is_empty() {
        // When tests configure an explicit response body, use it.
        app.config.json_body.clone()
    } else {
        // Default fixed Anthropic Messages response.
        serde_json::json!({
            "id": "msg_mock_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-mock",
            "content": [{"type": "text", "text": "Hello from Anthropic mock!"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })
        .to_string()
    };

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        response_body,
    )
        .into_response()
}
