use std::sync::Arc;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::lanes::{LanePools, PoolBuilder};
use workflow_runtime::{InMemoryPublisher, RuntimeSnapshot, SnapshotReader};

/// A consistent (snapshot, pools) pair captured atomically per request.
///
/// One `ArcSwap` holds both, so a reader that sees snapshot v2 always resolves
/// v2's lane pools — never the pools of a snapshot it is not using. This is
/// the "immutable snapshot + atomic replacement" rule applied to the pool set
/// too.
pub struct PublishedBundle {
    /// The active snapshot, if one has been published.
    pub snapshot: Option<Arc<RuntimeSnapshot>>,
    /// Per-lane connection pools matching `snapshot`.
    pub pools: Arc<LanePools>,
}

/// Runtime snapshot publication state shared with the admin router.
///
/// The gateway is usually a pure proxy; this state exists only when a
/// deployment carries a workflow snapshot. The admin router's `/publish`
/// endpoint mutates it; data-plane workers read it through [`AppState`].
///
/// Snapshot and pools are swapped in ONE atomic store, so a request worker
/// can never observe (v1 snapshot, v2 pools) or the reverse.
pub struct PublicationState {
    /// Active snapshot + its per-lane pools, swapped atomically together.
    bundle: Arc<arc_swap::ArcSwap<PublishedBundle>>,
    pool_builder: Box<dyn PoolBuilder>,
}

impl PublicationState {
    /// Attach a publisher + pool holder for a run that has a snapshot.
    pub fn new(
        publisher: Arc<InMemoryPublisher>,
        lane_pools: LanePools,
        pool_builder: Box<dyn PoolBuilder>,
    ) -> Self {
        let snapshot = publisher.snapshot();
        Self {
            bundle: Arc::new(arc_swap::ArcSwap::from(Arc::new(PublishedBundle {
                snapshot,
                pools: Arc::new(lane_pools),
            }))),
            pool_builder,
        }
    }

    /// Atomically publish a new snapshot and rebuild the per-lane pools.
    ///
    /// The swap replaces snapshot + pools in ONE atomic store, so request
    /// workers can never observe (v1 snapshot, v2 pools) or the reverse.
    pub fn publish(&self, snapshot: Arc<RuntimeSnapshot>) {
        let pools = Arc::new(LanePools::build(&snapshot, &*self.pool_builder));
        self.bundle.store(Arc::new(PublishedBundle {
            snapshot: Some(snapshot),
            pools,
        }));
    }

    /// Capture the current (snapshot, pools) pair in one atomic load.
    pub fn load(&self) -> Arc<PublishedBundle> {
        self.bundle.load_full()
    }

    /// Current published snapshot, if any.
    pub fn snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.bundle.load_full().snapshot.clone()
    }

    /// Current lane pools (lock-free `Arc` clone).
    pub fn pools(&self) -> Arc<LanePools> {
        self.bundle.load_full().pools.clone()
    }
}

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

/// Wire format for a workflow publication: workflow JSON + the lanes it may
/// reference (name → base URL). The gateway compiles the workflow here — on
/// the publication/control-plane cadence, never on the request hot path.
/// Snapshot publication payload from the control plane.
///
/// Lanes are keyed by name; the optional `authorization` value (resolved
/// from a control-plane `credential_ref` at publish time) becomes the
/// lane's static `Authorization` header at runtime. Never put raw
/// credentials into workflow JSON — the wire snapshot is the seam.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireSnapshot {
    /// Monotonic snapshot version.
    pub snapshot_version: u64,
    /// Workflows (plans to compile and publish).
    pub workflows: Vec<WireWorkflow>,
}

/// A lane's runtime configuration within a `WireSnapshot`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireLane {
    /// Upstream base URL.
    pub base_url: String,
    /// Resolved `Authorization` header value, if the lane has credentials.
    #[serde(default)]
    pub authorization: Option<String>,
}

/// Modified `WireWorkflow`: `lanes` is now a map of lane name → `WireLane`
/// so credentials can be carried out of band from the workflow JSON.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireWorkflow {
    /// Workflow id (route `workflow_id`).
    pub id: String,
    /// The workflow graph as schema `Workflow` JSON.
    pub workflow: workflow_schema::Workflow,
    /// Lane name → runtime lane config (base URL + optional auth header).
    pub lanes: std::collections::HashMap<String, WireLane>,
}

impl PublicationState {
    /// Compile a set of workflows into a snapshot WITHOUT publishing it.
    ///
    /// Shared by `validate_workflows` and `publish_workflows`. On any
    /// per-workflow compile failure the whole set is rejected (the caller
    /// publishes nothing), so a bad plan never reaches the data plane.
    fn compile_snapshot(&self, wire: &WireSnapshot) -> Result<Arc<RuntimeSnapshot>, String> {
        let mut builder = workflow_runtime::RuntimeSnapshotBuilder::new(wire.snapshot_version);

        // Union of lane name → runtime lane config across every workflow.
        let mut lane_registry = workflow_runtime::context::LaneRegistry::new();
        for wf in &wire.workflows {
            for (name, lane_cfg) in &wf.lanes {
                if lane_registry.get(name).is_none() {
                    let base_url = url::Url::parse(&lane_cfg.base_url).map_err(|e| {
                        format!(
                            "workflow '{}': lane '{name}' has invalid base_url: {e}",
                            wf.id
                        )
                    })?;
                    lane_registry.register(workflow_runtime::context::LaneEntry {
                        id: name.clone(),
                        base_url,
                        authorization: lane_cfg.authorization.clone(),
                    });
                }
            }
        }
        let lane_registry = Arc::new(lane_registry);

        for wf in &wire.workflows {
            let plan = workflow_runtime::compile_workflow_with_lanes(
                &wf.workflow,
                &wf.lanes
                    .iter()
                    .map(|(k, v)| (k.clone(), v.base_url.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| format!("workflow '{}' failed to compile: {e}", wf.id))?;
            builder = builder
                .with_lanes(lane_registry.clone())
                .with_plan(wf.id.clone(), plan);
        }

        Ok(Arc::new(builder.build()))
    }

    /// Validate + compile a set of workflows without publishing anything.
    ///
    /// The control plane calls this during the publish pipeline so a workflow
    /// version records its deterministic plan hash *before* commit. The
    /// gateway stays the only compiler; this endpoint never changes the
    /// active runtime.
    pub fn validate_workflows(&self, wire: &WireSnapshot) -> Result<Arc<RuntimeSnapshot>, String> {
        self.compile_snapshot(wire)
    }

    /// Compile + publish a set of workflows atomically.
    ///
    /// Returns per-workflow compile failures WITHOUT publishing anything
    /// (atomicity: a bad plan never sees traffic). Lanes referenced by any
    /// workflow are registered in the snapshot's lane registry (union across
    /// workflows), so LLM/Fallback/Retry nodes resolve them at run time.
    pub fn publish_workflows(&self, wire: WireSnapshot) -> Result<Arc<RuntimeSnapshot>, String> {
        let snapshot = self.compile_snapshot(&wire)?;
        self.publish(snapshot.clone());
        Ok(snapshot)
    }
}

/// Admin router that also exposes the snapshot publication endpoint.
///
/// `publication` is the shared publication state — the admin handler
/// publishes directly into it, and data-plane workers read from the same
/// `Arc`. Pure-proxy deployments pass `None`.
pub fn admin_router_with_publication(
    handle: PrometheusHandle,
    publication: Option<Arc<PublicationState>>,
) -> axum::Router {
    use axum::extract::State;
    use axum::routing::{get, post};

    async fn publish(
        State(publication): State<Option<Arc<PublicationState>>>,
        axum::Json(wire): axum::Json<WireSnapshot>,
    ) -> axum::Json<serde_json::Value> {
        let Some(publication) = publication else {
            return axum::Json(serde_json::json!({
                "status": "error",
                "error": "workflow execution not configured"
            }));
        };

        match publication.publish_workflows(wire) {
            Ok(snapshot) => {
                // Surface the real versioned plan identity so the client can
                // cache server truth, not a fabricated value.
                let plans: serde_json::Value = snapshot
                    .workflow_ids()
                    .map(|id| {
                        serde_json::json!({
                            "workflow_id": id,
                            "plan_hash": snapshot.plan_hash_for(id).unwrap_or_default(),
                            "version": snapshot.version(),
                        })
                    })
                    .collect();
                axum::Json(serde_json::json!({
                    "status": "published",
                    "snapshot_version": snapshot.version(),
                    "workflows": plans,
                }))
            }
            Err(e) => axum::Json(serde_json::json!({
                "status": "error",
                "error": e
            })),
        }
    }

    async fn validate(
        State(publication): State<Option<Arc<PublicationState>>>,
        axum::Json(wire): axum::Json<WireSnapshot>,
    ) -> axum::Json<serde_json::Value> {
        let Some(publication) = publication else {
            return axum::Json(serde_json::json!({
                "status": "error",
                "error": "workflow execution not configured"
            }));
        };

        // Compile-only: the active runtime is never mutated.
        match publication.validate_workflows(&wire) {
            Ok(snapshot) => {
                let plans: serde_json::Value = snapshot
                    .workflow_ids()
                    .map(|id| {
                        serde_json::json!({
                            "workflow_id": id,
                            "plan_hash": snapshot.plan_hash_for(id).unwrap_or_default(),
                            "version": snapshot.version(),
                        })
                    })
                    .collect();
                axum::Json(serde_json::json!({
                    "status": "validated",
                    "snapshot_version": snapshot.version(),
                    "workflows": plans,
                }))
            }
            Err(e) => axum::Json(serde_json::json!({
                "status": "error",
                "error": e
            })),
        }
    }

    axum::Router::new()
        .route(
            "/healthz",
            get(|| async {
                axum::Json(serde_json::json!({ "status": "ok", "service": "relay-gateway" }))
            }),
        )
        .route(
            "/ready",
            get(|| async { axum::Json(serde_json::json!({ "status": "ready" })) }),
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
        .route("/validate", post(validate))
        .route("/publish", post(publish))
        .layer(axum::Extension(handle))
        .with_state(publication)
}
