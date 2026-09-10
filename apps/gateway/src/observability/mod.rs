use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the tracing subscriber.
///
/// Uses `RUST_LOG` env-filter with a sensible default for relay-x.
pub fn init_tracing() {
    let default_filter = "relay_x=info,tower_http=info";

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

/// Install the global metrics recorder exactly once and return its handle.
///
/// `install_recorder` errors if a global recorder is already set, so in
/// tests/benchmarks that spawn several gateway instances the recorder is
/// installed only on the first call; later calls reuse the same handle.
/// The handle is what `/metrics` parses into Prometheus text format.
pub fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    static HANDLE: std::sync::OnceLock<std::result::Result<PrometheusHandle, String>> =
        std::sync::OnceLock::new();

    let result = HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .map_err(|e| format!("failed to install metrics recorder: {e}"))
    });

    result.clone().map_err(|msg| anyhow::anyhow!("{msg}"))
}

/// RAII guard that increments a gauge on creation and decrements on drop.
pub struct GaugeGuard {
    name: &'static str,
    labels: Vec<(&'static str, String)>,
}

impl GaugeGuard {
    pub fn new(name: &'static str, labels: Vec<(&'static str, String)>) -> Self {
        for (key, value) in &labels {
            metrics::gauge!(name, *key => value.clone()).increment(1.0);
        }
        Self { name, labels }
    }
}

impl Drop for GaugeGuard {
    fn drop(&mut self) {
        for (key, value) in &self.labels {
            metrics::gauge!(self.name, *key => value.clone()).decrement(1.0);
        }
    }
}

/// Track active requests.
pub fn track_active_request(lane: &str) -> GaugeGuard {
    GaugeGuard::new("relayx_active_requests", vec![("lane", lane.to_owned())])
}

/// Record a request duration in milliseconds.
pub fn record_request_duration(status: &str, lane: &str, duration: std::time::Duration) {
    metrics::histogram!(
        "relayx_request_duration_ms",
        "status" => status.to_owned(),
        "lane" => lane.to_owned()
    )
    .record(duration.as_secs_f64() * 1000.0);
}

/// Record upstream connection duration.
pub fn record_upstream_connect_duration(lane: &str, duration: std::time::Duration) {
    metrics::histogram!(
        "relayx_upstream_connect_ms",
        "lane" => lane.to_owned()
    )
    .record(duration.as_secs_f64() * 1000.0);
}

/// Record upstream TTFB (time to first byte).
pub fn record_upstream_ttfb(lane: &str, duration: std::time::Duration) {
    metrics::histogram!("relayx_upstream_ttfb_ms", "lane" => lane.to_owned())
        .record(duration.as_secs_f64() * 1000.0);
}

/// Increment the total request counter.
pub fn increment_request_count(status: &str, lane: &str) {
    metrics::counter!(
        "relayx_request_total",
        "status" => status.to_owned(),
        "lane" => lane.to_owned()
    )
    .increment(1);
}

/// Increment bytes received from client.
pub fn record_bytes_in(n: u64) {
    metrics::counter!("relayx_bytes_in").increment(n);
}

/// Increment bytes sent to client.
pub fn record_bytes_out(n: u64) {
    metrics::counter!("relayx_bytes_out").increment(n);
}

/// Increment the total timeout counter.
pub fn increment_timeout_count(lane: &str) {
    metrics::counter!("relayx_timeout_total", "lane" => lane.to_owned()).increment(1);
}

/// Track active connections via an RAII gauge guard.
pub fn track_active_connection(lane: &str) -> GaugeGuard {
    GaugeGuard::new("relayx_active_connections", vec![("lane", lane.to_owned())])
}

/// Record upstream body stream duration (total time reading frames).
pub fn record_upstream_body_duration(lane: &str, duration: std::time::Duration) {
    metrics::histogram!(
        "relayx_upstream_body_duration_ms",
        "lane" => lane.to_owned()
    )
    .record(duration.as_secs_f64() * 1000.0);
}

/// Increment route selected counter.
pub fn increment_route_selected(route_id: &str) {
    metrics::counter!("relayx_route_selected_total", "route" => route_id.to_owned()).increment(1);
}

/// Increment lane selected counter.
pub fn increment_lane_selected(lane_id: &str) {
    metrics::counter!("relayx_lane_selected_total", "lane" => lane_id.to_owned()).increment(1);
}

/// Build an axum router for the admin listener (health + metrics).
pub fn admin_router(handle: PrometheusHandle) -> axum::Router {
    use axum::routing::get;

    axum::Router::new()
        .route(
            "/healthz",
            get(|| async {
                axum::Json(serde_json::json!({
                    "status": "ok",
                    "service": "relay-gateway"
                }))
            }),
        )
        .route(
            "/ready",
            get(|| async {
                // Ready means the gateway can accept traffic.
                // In Phase 1, this is always true after startup.
                axum::Json(serde_json::json!({ "status": "ready" }))
            }),
        )
        .route(
            "/metrics",
            get(
                |axum::Extension(handle): axum::Extension<PrometheusHandle>| async move {
                    (
                        [(http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                        handle.render(),
                    )
                },
            ),
        )
        .layer(axum::Extension(handle))
}
