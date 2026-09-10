//! SSE generator for the mock upstream.
//!
//! Emits OpenAI-style SSE chunks (`data: ...` lines) suitable for testing
//! streaming pass-through fidelity.

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use std::time::Duration;

/// Configuration for a single SSE response.
#[derive(Debug, Clone)]
pub struct SseConfig {
    /// Number of SSE `data:` chunks to emit.
    pub chunks: usize,
    /// Approximate bytes per `data:` chunk.
    pub chunk_size: usize,
    /// Delay between chunks.
    pub chunk_delay: Duration,
    /// If set, fail (drop the connection) after this many chunks.
    pub error_at: Option<usize>,
    /// Status code to use for a well-formed error response.
    pub error_status: u16,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            chunks: 10,
            chunk_size: 512,
            chunk_delay: Duration::ZERO,
            error_at: None,
            error_status: 500,
        }
    }
}

/// A single SSE event (data line + optional trailing newline).
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub data: String,
    pub final_newline: bool,
}

impl SseEvent {
    /// Render the event to wire format: `data: <payload>\n\n`.
    pub fn to_wire(&self) -> Vec<u8> {
        let mut out = format!("data: {}\n", self.data).into_bytes();
        if self.final_newline {
            out.extend_from_slice(b"\n");
        }
        out
    }
}

/// Build the SSE event body generator.
///
/// Returns a `Body` that streams `data:` chunks with the configured timing.
pub fn build_sse_events(config: &SseConfig) -> Vec<SseEvent> {
    // Build payload chunks of `chunk_size` bytes each over a filler payload.
    const FILLER: &[u8] = b"mock-stream-data-filler-";
    let filler: Vec<u8> = FILLER.iter().copied().cycle().take(config.chunk_size).collect();
    let mut events = Vec::with_capacity(config.chunks);
    for i in 0..config.chunks {
        let data = format!(
            "{{\"id\":\"chunk-{}\",\"chunk_bytes\":{},\"payload\":\"{}\"}}",
            i,
            config.chunk_size,
            String::from_utf8_lossy(&filler[..config.chunk_size.min(filler.len())])
        );
        events.push(SseEvent {
            data,
            final_newline: true,
        });
    }
    events
}

/// Produce the SSE response body.
///
/// Emits `data:` chunks until reaching `error_at`, which terminates the
/// stream. If the error fires before the first chunk (`error_at == Some(0)`),
/// the response is a well-formed error with `error_status`; otherwise the
/// stream starts as 200 `text/event-stream` and drops mid-stream (the status
/// cannot change after headers are sent).
pub fn sse_response(config: SseConfig) -> Response {
    if config.error_at == Some(0) {
        return (
            axum::http::StatusCode::from_u16(config.error_status)
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
            [(CONTENT_TYPE, "text/plain")],
            "mock error response",
        )
            .into_response();
    }

    let events = build_sse_events(&config);
    let chunk_delay = config.chunk_delay;
    let error_at = config.error_at;

    let stream = stream::unfold((0, events.into_iter()), move |(idx, mut it)| {
        let delay = chunk_delay;
        async move {
            if let Some(limit) = error_at
                && idx >= limit
            {
                return None; // connection dropped (no trailing data)
            }
            match it.next() {
                Some(event) => {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    Some((
                        Ok::<_, std::convert::Infallible>(event.to_wire()),
                        (idx + 1, it),
                    ))
                }
                None => None,
            }
        }
    });

    let body = Body::from_stream(stream);

    (
        axum::http::StatusCode::OK,
        [(CONTENT_TYPE, "text/event-stream")],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_event_wire_format() {
        let event = SseEvent {
            data: "hello".into(),
            final_newline: true,
        };
        assert_eq!(event.to_wire(), b"data: hello\n\n");
    }

    #[test]
    fn test_build_sse_events_preserves_shape() {
        let config = SseConfig {
            chunks: 5,
            chunk_size: 32,
            ..Default::default()
        };
        let events = build_sse_events(&config);
        assert_eq!(events.len(), 5);
        // Each event renders to wire format.
        for event in &events {
            assert!(!event.to_wire().is_empty());
            assert!(event.data.contains("chunk-"));
        }
    }

    #[test]
    fn test_sse_response_builds() {
        let response = sse_response(SseConfig::default());
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&axum::http::HeaderValue::from_static("text/event-stream"))
        );
    }

    #[test]
    fn test_sse_response_error_status() {
        let config = SseConfig {
            error_at: Some(0),
            error_status: 503,
            ..Default::default()
        };
        let response = sse_response(config);
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
