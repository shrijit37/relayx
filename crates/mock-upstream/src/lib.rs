//! Reusable mock LLM upstream for integration tests and benchmarks.
//!
//! Provides a configurable HTTP server that simulates an LLM provider:
//! - streaming (SSE) and non-streaming (JSON) responses
//! - configurable latency and chunk sizes
//! - deterministic error points
//! - connection resets
//! - request/response counters for assertions

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower::ServiceExt;

use crate::app::build_app;

mod app;
mod sse;

pub use crate::sse::SseEvent;

/// Mode of a mock endpoint.
#[derive(Debug, Clone, Copy)]
pub enum MockMode {
    /// Single-shot JSON response with Content-Length.
    Json,
    /// Streaming SSE response with chunked transfer encoding.
    Sse,
}

/// Configuration for the mock upstream.
#[derive(Debug, Clone)]
pub struct MockConfig {
    pub mode: MockMode,
    /// Number of SSE chunks to emit (SSE mode only, when `raw_sse` is None).
    pub chunks: usize,
    /// Bytes per chunk (SSE mode only, when `raw_sse` is None).
    pub chunk_size: usize,
    /// Artificial delay before the response headers (TTFB).
    pub ttfb: std::time::Duration,
    /// Delay between chunks (SSE mode only).
    pub chunk_delay: std::time::Duration,
    /// JSON response body for non-streaming mode.
    pub json_body: String,
    /// Raw SSE wire body for streaming mode (takes precedence over `chunks`).
    /// Each entry is one SSE data line (e.g. `{"type":"..."}`); the mock emits
    /// them in order as `data: <line>\n\n`.
    pub raw_sse: Option<Vec<String>>,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            mode: MockMode::Sse,
            chunks: 10,
            chunk_size: 512,
            ttfb: std::time::Duration::ZERO,
            chunk_delay: std::time::Duration::ZERO,
            json_body: r#"{"ok":true,"result":"hello"}"#.into(),
            raw_sse: None,
        }
    }
}

/// Observable state of the mock server.
///
/// Counters allow tests and benchmarks to assert behavior
/// (e.g., "connections were reused"). `last_request_body` captures the
/// most recently received request body so protocol-translation tests can
/// verify that the gateway emitted the correct wire format.
#[derive(Debug, Default)]
pub struct MockState {
    pub requests_served: std::sync::atomic::AtomicU64,
    pub connections_accepted: std::sync::atomic::AtomicU64,
    pub bytes_sent: std::sync::atomic::AtomicU64,
    /// Body of the most recently received LLM request, for test assertions.
    pub last_request_body: std::sync::Mutex<Option<String>>,
}

impl MockState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

/// A running mock upstream: state handle + task.
pub struct MockUpstream {
    pub state: Arc<MockState>,
    pub addr: SocketAddr,
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    pub handle: JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        // Send shutdown signal and best-effort wait for the task.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Note: we don't await the handle in Drop (it's sync), but the
        // shutdown signal will cause the mock loop to exit on its own.
    }
}

/// Spawn a mock upstream on an ephemeral port.
pub async fn spawn_mock(config: MockConfig) -> std::io::Result<MockUpstream> {
    spawn_mock_on(config, SocketAddr::from(([127, 0, 0, 1], 0))).await
}

/// Spawn a mock upstream on a specific address.
pub async fn spawn_mock_on(
    config: MockConfig,
    listen: SocketAddr,
) -> std::io::Result<MockUpstream> {
    let state = MockState::new();
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let run_state = state.clone();
    let handle = tokio::spawn(async move {
        run_mock(listener, config, run_state, shutdown_rx).await;
    });

    Ok(MockUpstream {
        state,
        addr,
        shutdown_tx: Some(shutdown_tx),
        handle,
    })
}

async fn run_mock(
    listener: tokio::net::TcpListener,
    config: MockConfig,
    state: Arc<MockState>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let router = build_app(config, state.clone());

    loop {
        let router = router.clone();
        let state = state.clone();

        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::debug!("mock upstream shutdown signal received");
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _peer)) => {
                        state.connections_accepted.fetch_add(1, Ordering::Relaxed);

                        let svc = hyper::service::service_fn(move |req| {
                            let router = router.clone();
                            async move {
                                let (parts, incoming_body) = req.into_parts();
                                // Map hyper's body into axum's Body (axum Body
                                // is a wrapper around the same frame types).
                                let body = axum::body::Body::new(incoming_body);
                                let req = hyper::Request::from_parts(parts, body);
                                let resp = router.oneshot(req).await?;
                                Ok::<_, std::convert::Infallible>(resp)
                            }
                        });

                        let io = hyper_util::rt::TokioIo::new(stream);
                        tokio::spawn(async move {
                            let _ = hyper::server::conn::http1::Builder::new()
                                .serve_connection(io, svc)
                                .await;
                        });
                    }
                    Err(e) => {
                        // Transient errors (EMFILE, ENOTCONN) are common under
                        // load. Log and continue rather than killing the server.
                        tracing::warn!(error = %e, "mock accept error, retrying");
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
            }
        }
    }
}
