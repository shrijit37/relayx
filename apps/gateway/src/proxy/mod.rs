use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use bytes::Bytes;
use futures_util::TryStreamExt;
use http::{HeaderMap, Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use tokio_stream::Stream;

use crate::errors::GatewayError;
use crate::server::AppState;
use crate::transport;
use workflow_runtime::RuntimeSnapshot;

/// Extension: read the current published runtime snapshot.
///
/// `publication` is the active snapshot store; None when the deployment is a
/// pure proxy (no workflow execution).
pub trait CurrentSnapshot {
    fn current_snapshot(&self) -> Option<Arc<RuntimeSnapshot>>;
}

impl CurrentSnapshot for AppState {
    fn current_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.publication.as_ref().and_then(|p| p.snapshot())
    }
}

/// Extension: per-lane client for a lane name.
///
/// The data plane knows which pool belongs to which lane; the runtime calls
/// this through `ExecutionContext::lane_clients` (via `AsLaneClient`).
impl workflow_runtime::AsLaneClient for AppState {
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<crate::execution::GatewayClient>> {
        let pools = self.publication.as_ref()?.pools();
        pools.get(lane_id).map(|snapshot| snapshot.client.clone())
    }
}

/// A stream wrapper that enforces a per-frame timeout.
///
/// If no frame arrives within `timeout`, the stream terminates with an error.
/// This prevents a stalled upstream from holding the connection indefinitely.
struct FrameTimeoutStream<S> {
    inner: S,
    timeout: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
}

impl<S: Unpin> Unpin for FrameTimeoutStream<S> {}

impl<S> FrameTimeoutStream<S> {
    fn new(inner: S, timeout: Duration) -> Self {
        let sleep = Box::pin(tokio::time::sleep(timeout));
        Self {
            inner,
            timeout,
            sleep,
        }
    }
}

impl<S> Stream for FrameTimeoutStream<S>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Self: Unpin (manual impl above), so get_mut() is safe.
        let this = self.get_mut();

        // Poll the inner stream first — do NOT reset the timer before this.
        // The timer measures time since the last frame was received.
        if let Poll::Ready(item) = Pin::new(&mut this.inner).poll_next(cx) {
            // Frame received — restart the per-frame timer.
            this.sleep
                .as_mut()
                .reset(tokio::time::Instant::now() + this.timeout);
            return Poll::Ready(item);
        }

        // No frame arrived. Check the timer without resetting it.
        if this.sleep.as_mut().poll(cx).is_ready() {
            Poll::Ready(Some(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "upstream body frame timeout: no data received within {:?}",
                    this.timeout
                ),
            ))))
        } else {
            Poll::Pending
        }
    }
}

/// Main proxy handler — captures all requests and forwards them to the
/// resolved upstream lane. This is the gateway's hot path.
///
/// Pipeline: parse → match route → acquire pool connection → forward → stream response.
#[tracing::instrument(
    skip(state, req),
    fields(
        method = %req.method(),
        path = %req.uri().path(),
    ),
    level = "debug"
)]
pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<axum::response::Response<Body>, GatewayError> {
    // Enforce the per-request timeout deadline. If the entire handler (including
    // upstream response headers) does not complete within the deadline, return 504.
    match tokio::time::timeout(state.timeout, proxy_handler_inner(state.clone(), req)).await {
        Ok(result) => result,
        Err(_elapsed) => {
            let lane_id = "unknown";
            crate::observability::increment_timeout_count(lane_id);
            tracing::warn!(
                timeout_ms = state.timeout.as_millis(),
                "request timeout exceeded"
            );
            Err(GatewayError::UpstreamTimeout {
                upstream: lane_id.to_owned(),
                elapsed: state.timeout,
            })
        }
    }
}

async fn proxy_handler_inner(
    state: Arc<AppState>,
    req: Request<Body>,
) -> Result<axum::response::Response<Body>, GatewayError> {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // ── 0. Request ID ──────────────────────────────────────────────────────
    let request_id = uuid::Uuid::new_v4().to_string();
    tracing::Span::current().record("request_id", request_id.as_str());

    // ── 1. Match route ──────────────────────────────────────────────────────
    let (route_id, lane_id, src_proto, tgt_proto, workflow_id) = {
        let (route, lane) = state.config.match_route(&method, &path).ok_or_else(|| {
            GatewayError::InvalidRequest {
                status: StatusCode::NOT_FOUND,
                message: format!("no route matches {method} {path}"),
            }
        })?;

        // A workflow route carries no lane; a normal route always does.
        let lane_id = match lane {
            Some(l) => l.id.clone(),
            None => String::new(),
        };
        (
            route.id.clone(),
            lane_id,
            route.source_protocol,
            route.target_protocol,
            route.workflow_id.clone(),
        )
    };

    // If the route is a workflow route, execute the compiled plan instead of
    // proxying to a lane.
    if let Some(ref wf_id) = workflow_id {
        return workflow_route_request(state, req, wf_id, &request_id, start).await;
    }

    tracing::debug!(
        route_id = %route_id,
        lane_id = %lane_id,
        request_id = %request_id,
        "matched route"
    );

    crate::observability::increment_route_selected(&route_id);
    crate::observability::increment_lane_selected(&lane_id);

    let _active_guard = crate::observability::track_active_request(&lane_id);

    match (src_proto, tgt_proto) {
        (Some(src), Some(tgt)) => {
            let engine = crate::protocol::ProtocolEngine::from_pair(src, tgt)?;
            translate_proxy_request(state, &engine, req, &lane_id, &route_id, &request_id, start)
                .await
        }
        _ => passthrough_proxy_request(state, req, &lane_id, &route_id, &request_id, start).await,
    }
}

/// Phase 1 fast-path passthrough: forward the request body verbatim and
/// stream the upstream response back without translation.
async fn passthrough_proxy_request(
    state: Arc<AppState>,
    req: Request<Body>,
    lane_id: &str,
    route_id: &str,
    request_id: &str,
    start: Instant,
) -> Result<axum::response::Response<Body>, GatewayError> {
    let path = req.uri().path().to_string();
    let lane = state
        .config
        .lookup_lane(lane_id)
        .ok_or_else(|| GatewayError::Internal(format!("unknown lane: {lane_id}")))?;

    let upstream_req = build_upstream_request(&lane.base_url, req, &path)?;
    let frame_timeout = lane.frame_timeout;

    forward_upstream(
        state,
        upstream_req,
        lane_id,
        route_id,
        request_id,
        start,
        None,
        frame_timeout,
    )
    .await
}

/// Translation path: decode the client request, translate through the
/// canonical model, forward the translated request upstream, then translate
/// the upstream response back to the client's protocol.
///
/// Streaming responses are translated event-by-event via the SSE parser.
async fn translate_proxy_request(
    state: Arc<AppState>,
    engine: &crate::protocol::ProtocolEngine,
    req: Request<Body>,
    lane_id: &str,
    _route_id: &str,
    request_id: &str,
    start: Instant,
) -> Result<axum::response::Response<Body>, GatewayError> {
    let _path = req.uri().path().to_string();
    let lane = state
        .config
        .lookup_lane(lane_id)
        .ok_or_else(|| GatewayError::Internal(format!("unknown lane: {lane_id}")))?;

    // ── Buffer client request body and decode ────────────────────────────────
    let (_parts, body) = req.into_parts();
    let body_bytes = http_body_util::BodyExt::collect(body)
        .await
        .map_err(|e| GatewayError::Internal(format!("failed to buffer request body: {e}")))?
        .to_bytes();

    let canonical_request = engine.decode_request(&body_bytes)?;

    // ── Enforce translation-loss policy on the ACTUAL request ───────────────
    // A `Reject`/`Drop` loss is surfaced as a client error rather than
    // silently degrading the request. Loss detection compares the features
    // this request actually uses against the target's capabilities — the
    // capability matrices may differ without the request being lossy.
    engine.check_request_losses(&canonical_request)?;

    // ── Encode for the target protocol ───────────────────────────────────────
    let target_body = engine.encode_request(&canonical_request)?;
    let is_stream = canonical_request.stream;

    // ── Forward translated request ───────────────────────────────────────────
    let lane_url = &lane.base_url;
    let method = http::Method::POST; // all LLM protocol routes are POST
    let upstream_url = {
        let mut u = lane_url.clone();
        // Use the target protocol's canonical upstream path, not the
        // client's original path (which is specific to the source protocol).
        let target_path = crate::config::protocol_upstream_path(engine.target_protocol());
        u.set_path(target_path);
        u
    };

    let mut upstream_req = http::Request::builder()
        .method(method)
        .uri(upstream_url.as_str())
        .header(http::header::CONTENT_TYPE, "application/json");

    // Copy Host.
    let host_value = transport::build_host_header(
        lane_url.scheme(),
        lane_url.host_str().unwrap_or("localhost"),
        lane_url.port(),
    );
    upstream_req = upstream_req.header(http::header::HOST, host_value);

    let upstream_req = upstream_req
        .body(Body::from(target_body))
        .map_err(|e| GatewayError::Internal(format!("failed to build upstream request: {e}")))?;

    let connect_start = Instant::now();
    let upstream_response = state.client.request(upstream_req).await.map_err(|e| {
        tracing::warn!(error = %e, request_id = %request_id, "upstream request failed");
        GatewayError::UpstreamConnection {
            upstream: lane_id.to_owned(),
            reason: e.to_string(),
        }
    })?;
    let connect_duration = connect_start.elapsed();
    crate::observability::record_upstream_connect_duration(lane_id, connect_duration);

    let status = upstream_response.status();
    let status_str = status.as_str().to_string();
    let ttfb = start.elapsed();
    crate::observability::record_upstream_ttfb(lane_id, ttfb);
    crate::observability::record_request_duration(&status_str, lane_id, ttfb);
    crate::observability::increment_request_count(&status_str, lane_id);

    tracing::debug!(
        status = %status,
        upstream_ttfb_ms = ttfb.as_millis(),
        request_id = %request_id,
        "translated upstream responded"
    );

    // ── Translate response ───────────────────────────────────────────────────
    if is_stream {
        let response_body =
            engine.stream_response(Body::new(upstream_response.into_body()), lane.frame_timeout)?;

        axum::response::Response::builder()
            .status(status)
            .header(http::header::CONTENT_TYPE, "text/event-stream")
            .body(response_body)
            .map_err(|e| GatewayError::Internal(format!("failed to build response: {e}")))
    } else {
        // Non-streaming: buffer, decode upstream response, encode for client.
        let upstream_bytes = http_body_util::BodyExt::collect(upstream_response.into_body())
            .await
            .map_err(|e| {
                GatewayError::Internal(format!("failed to buffer upstream response: {e}"))
            })?
            .to_bytes();

        let canonical_response = engine.decode_response(&upstream_bytes)?;
        let client_body = engine.encode_response(&canonical_response)?;

        axum::response::Response::builder()
            .status(status)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(client_body))
            .map_err(|e| GatewayError::Internal(format!("failed to build response: {e}")))
    }
}

/// Forward an upstream request and stream the response back.
/// When `override_body` is Some, that body replaces the upstream response.
// ponytail: 8 params; group into a ForwardSpec struct if another is added.
#[allow(clippy::too_many_arguments)]
async fn forward_upstream(
    state: Arc<AppState>,
    upstream_req: HttpRequest<Body>,
    lane_id: &str,
    _route_id: &str,
    request_id: &str,
    start: Instant,
    override_body: Option<Body>,
    frame_timeout: Duration,
) -> Result<axum::response::Response<Body>, GatewayError> {
    let connect_start = Instant::now();
    let upstream_response = state.client.request(upstream_req).await.map_err(|e| {
        tracing::warn!(error = %e, request_id = %request_id, "upstream request failed");
        GatewayError::UpstreamConnection {
            upstream: lane_id.to_owned(),
            reason: e.to_string(),
        }
    })?;
    let connect_duration = connect_start.elapsed();
    crate::observability::record_upstream_connect_duration(lane_id, connect_duration);

    let status = upstream_response.status();
    let status_str = status.as_str().to_string();

    let ttfb = start.elapsed();
    crate::observability::record_upstream_ttfb(lane_id, ttfb);
    crate::observability::record_request_duration(&status_str, lane_id, ttfb);
    crate::observability::increment_request_count(&status_str, lane_id);

    tracing::debug!(
        status = %status,
        upstream_ttfb_ms = ttfb.as_millis(),
        request_id = %request_id,
        "upstream responded"
    );

    let response_body = if let Some(body) = override_body {
        body
    } else {
        let request_id_for_stream = request_id.to_owned();
        let upstream_body = upstream_response.into_body();
        let mapped_body = upstream_body.into_data_stream().map_err(move |e| {
            tracing::error!(
                error = %e,
                request_id = %request_id_for_stream,
                "upstream body read error during streaming"
            );
            std::io::Error::other(e.to_string())
        });
        Body::from_stream(FrameTimeoutStream::new(mapped_body, frame_timeout))
    };

    axum::response::Response::builder()
        .status(status)
        .body(response_body)
        .map_err(|e| GatewayError::Internal(format!("failed to build response: {e}")))
}

/// Build the upstream HTTP request from the incoming client request.
///
/// Strips hop-by-hop headers, rewrites Host to the lane's upstream URL,
/// and preserves the request body (streaming, not buffered).
fn build_upstream_request(
    base_url: &url::Url,
    client_req: HttpRequest<Body>,
    original_path: &str,
) -> Result<HttpRequest<Body>, GatewayError> {
    let method = client_req.method().clone();

    // Build the full upstream URL.
    let mut upstream_url = base_url.clone();
    upstream_url.set_path(original_path);
    // Preserve query string if present.
    if let Some(query) = client_req.uri().query() {
        upstream_url.set_query(Some(query));
    }

    let mut upstream_req = HttpRequest::builder()
        .method(method)
        .uri(upstream_url.as_str());

    // Copy headers, stripping hop-by-hop.
    let mut headers = HeaderMap::new();
    transport::copy_headers(client_req.headers(), &mut headers);

    // Rewrite Host to the lane's upstream host.
    let host_value = transport::build_host_header(
        base_url.scheme(),
        base_url.host_str().unwrap_or("localhost"),
        base_url.port(),
    );
    headers.insert(http::header::HOST, host_value);

    // Apply headers to the builder.
    for (name, value) in &headers {
        upstream_req = upstream_req.header(name, value);
    }

    // Split request into parts + body. The body is passed through — NOT buffered.
    let (_parts, body) = client_req.into_parts();

    upstream_req
        .body(body)
        .map_err(|e| GatewayError::Internal(format!("failed to build upstream request: {e}")))
}

/// Workflow route: execute a compiled plan instead of proxying.
///
/// Decodes the request body as JSON, runs it through the execution plan
/// (fast path or full interpreter as classified at compile time), and
/// returns the result as `application/json`.
async fn workflow_route_request(
    state: Arc<AppState>,
    req: Request<Body>,
    workflow_id: &str,
    request_id: &str,
    _start: Instant,
) -> Result<axum::response::Response<Body>, GatewayError> {
    let snapshot = state
        .current_snapshot()
        .ok_or_else(|| GatewayError::Internal("workflow execution not configured".into()))?;

    let plan = snapshot
        .get_plan(workflow_id)
        .ok_or_else(|| GatewayError::InvalidRequest {
            status: StatusCode::NOT_FOUND,
            message: format!("workflow not found: {workflow_id}"),
        })?;

    let (_parts, body) = req.into_parts();
    let body_bytes = http_body_util::BodyExt::collect(body)
        .await
        .map_err(|e| {
            GatewayError::Internal(format!("failed to buffer workflow request body: {e}"))
        })?
        .to_bytes();

    // The outer `proxy_handler` wraps this in `tokio::time::timeout(state.timeout, ...)`;
    // mirror that deadline into the context so workflow-level deadline checks fire.
    let deadline = Some(tokio::time::Instant::now() + state.timeout);

    crate::execution::execute_workflow(
        &snapshot,
        plan,
        body_bytes,
        workflow_id,
        request_id,
        state.client.clone(),
        Some(state.clone()),
        deadline,
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GatewayConfig;
    use http::Method;

    fn test_config() -> anyhow::Result<GatewayConfig> {
        let toml_str = r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "echo"
path_prefix = "/v1/echo"
methods = ["POST"]
lane = "mock"

[[lanes]]
id = "mock"
base_url = "http://127.0.0.1:9000"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        Ok(config)
    }

    #[test]
    fn test_build_upstream_request_rewrites_host() -> anyhow::Result<()> {
        let config = test_config()?;
        let snapshot = config.compile()?;
        let lane = snapshot
            .lookup_lane("mock")
            .ok_or_else(|| anyhow::anyhow!("mock lane missing from compiled snapshot"))?;

        let client_req = http::Request::builder()
            .method(Method::POST)
            .uri("http://localhost:8080/v1/echo?foo=bar")
            .header("content-type", "application/json")
            .header("host", "gateway.local")
            .header("proxy-authorization", "secret")
            .body(Body::empty())?;

        let upstream_req = build_upstream_request(&lane.base_url, client_req, "/v1/echo")?;

        assert_eq!(upstream_req.uri(), "http://127.0.0.1:9000/v1/echo?foo=bar");
        let host = upstream_req
            .headers()
            .get("host")
            .ok_or_else(|| anyhow::anyhow!("upstream request missing host header"))?;
        assert_eq!(host, "127.0.0.1:9000");
        assert!(upstream_req.headers().get("proxy-authorization").is_none());
        let content_type = upstream_req
            .headers()
            .get("content-type")
            .ok_or_else(|| anyhow::anyhow!("upstream request missing content-type header"))?;
        assert_eq!(content_type, "application/json");
        Ok(())
    }
}
