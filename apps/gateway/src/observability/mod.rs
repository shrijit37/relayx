use std::sync::Arc;

use axum::response::IntoResponse;
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
    /// Extension registry for compile-time kind resolution and runtime
    /// execution. Set at startup via [`Self::with_extensions`]; the
    /// compile path reads the current value so a hot-reload is possible
    /// without rebuilding `PublicationState`.
    extensions: Arc<arc_swap::ArcSwap<Option<Arc<workflow_runtime::ExtensionRegistry>>>>,
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
            extensions: Arc::new(arc_swap::ArcSwap::from(Arc::new(None))),
        }
    }

    /// Attach an extension registry for compile-time kind resolution and
    /// runtime execution. Can be called at any time; the compile path
    /// observes the latest value.
    pub fn with_extensions(&self, extensions: Arc<workflow_runtime::ExtensionRegistry>) {
        self.extensions.store(Arc::new(Some(extensions)));
    }

    /// Current extension registry, if one has been registered.
    pub fn current_extensions(&self) -> Option<Arc<workflow_runtime::ExtensionRegistry>> {
        (*self.extensions.load_full()).clone()
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
    // `relay_x` is this workspace's tracing target. There is no `tower_http`
    // directive: the gateway never installs a tower-http layer (request IDs are
    // generated explicitly in src/proxy/mod.rs), so that directive only ever
    // silenced nothing and referenced a crate that is no longer a dependency.
    let default_filter = "relay_x=info";

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

/// Increment the total timeout counter.
pub fn increment_timeout_count(lane: &str) {
    metrics::counter!("relayx_timeout_total", "lane" => lane.to_owned()).increment(1);
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
///
/// Accepts `api_key` for signature consistency with
/// [`admin_router_with_publication`]; this router exposes no mutating
/// endpoints, so the key is not enforced here.
pub fn admin_router(handle: PrometheusHandle, _api_key: Option<String>) -> axum::Router {
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
    /// Extension specs (kind + version) for observability. The actual
    /// validator/executor trait objects are not serialized — they live
    /// in the runtime `ExtensionRegistry`, not on the wire.
    #[serde(default)]
    pub extensions: Vec<WireExtension>,
}

/// An extension spec on the wire (kind + version only).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireExtension {
    /// The extension kind identifier.
    pub kind: String,
    /// Version of this extension.
    pub version: u64,
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
    /// The ACTIVE version this workflow was compiled from (run identity).
    #[serde(default)]
    pub version: u64,
}

impl PublicationState {
    /// Compile a set of workflows into a snapshot WITHOUT publishing it.
    ///
    /// Shared by `validate_workflows` and `publish_workflows`. On any
    /// per-workflow compile failure the whole set is rejected (the caller
    /// publishes nothing), so a bad plan never reaches the data plane.
    fn compile_snapshot(&self, wire: &WireSnapshot) -> Result<Arc<RuntimeSnapshot>, String> {
        let mut builder = workflow_runtime::RuntimeSnapshotBuilder::new(wire.snapshot_version);

        // Record extension specs (kind + version) as snapshot metadata.
        for ext in &wire.extensions {
            builder = builder.with_extension(ext.kind.clone(), ext.version);
        }

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

        let extensions = self.current_extensions();

        for wf in &wire.workflows {
            let plan = workflow_runtime::compile_workflow_with_lanes(
                &wf.workflow,
                &wf.lanes
                    .iter()
                    .map(|(k, v)| (k.clone(), v.base_url.clone()))
                    .collect::<Vec<_>>(),
                extensions.clone(),
            )
            .map_err(|e| format!("workflow '{}' failed to compile: {e}", wf.id))?;
            builder = builder
                .with_lanes(lane_registry.clone())
                .with_plan(wf.id.clone(), plan)
                .with_plan_version(wf.id.clone(), wf.version);
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
///
/// When `api_key` is `Some`, mutating endpoints (`/publish`, `/validate`,
/// `/run`) require `Authorization: Bearer <key>`. Read-only endpoints
/// (`/healthz`, `/ready`, `/metrics`) are always unauthenticated.
pub fn admin_router_with_publication(
    handle: PrometheusHandle,
    publication: Option<Arc<PublicationState>>,
    api_key: Option<String>,
) -> axum::Router {
    use axum::extract::State;
    use axum::routing::{get, post};
    use tokio_stream::StreamExt as _;

    /// Combined admin state: publication seam + auth key.
    #[derive(Clone)]
    struct AdminState {
        publication: Option<Arc<PublicationState>>,
        api_key: Option<String>,
    }

    /// Shared envelope builder for both /publish and /validate: renders the
    /// real versioned plan identity (workflow_id / plan_hash / snapshot
    /// version) so no handler re-implements the response shape (review).
    fn plan_response(
        status: &str,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> axum::Json<serde_json::Value> {
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
            "status": status,
            "snapshot_version": snapshot.version(),
            "workflows": plans,
        }))
    }

    /// Validate `Authorization: Bearer <key>` against the configured key.
    /// Returns `Err(StatusCode)` on failure; `Ok(())` if valid or unconfigured.
    fn check_auth(
        headers: &http::HeaderMap,
        expected: &Option<String>,
    ) -> Result<(), http::StatusCode> {
        if let Some(expected) = expected {
            let provided = headers
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "));
            if provided != Some(expected.as_str()) {
                return Err(http::StatusCode::UNAUTHORIZED);
            }
        }
        Ok(())
    }

    async fn publish(
        State(state): State<AdminState>,
        headers: http::HeaderMap,
        axum::Json(wire): axum::Json<WireSnapshot>,
    ) -> impl axum::response::IntoResponse {
        if let Err(status) = check_auth(&headers, &state.api_key) {
            return (
                status,
                axum::Json(serde_json::json!({
                    "status": "error",
                    "error": "invalid or missing API key"
                })),
            )
                .into_response();
        }

        let Some(publication) = state.publication else {
            return axum::Json(serde_json::json!({
                "status": "error",
                "error": "workflow execution not configured"
            }))
            .into_response();
        };

        match publication.publish_workflows(wire) {
            Ok(snapshot) => plan_response("published", &snapshot).into_response(),
            Err(e) => axum::Json(serde_json::json!({
                "status": "error",
                "error": e
            }))
            .into_response(),
        }
    }

    async fn validate(
        State(state): State<AdminState>,
        headers: http::HeaderMap,
        axum::Json(wire): axum::Json<WireSnapshot>,
    ) -> impl axum::response::IntoResponse {
        if let Err(status) = check_auth(&headers, &state.api_key) {
            return (
                status,
                axum::Json(serde_json::json!({
                    "status": "error",
                    "error": "invalid or missing API key"
                })),
            )
                .into_response();
        }

        let Some(publication) = state.publication else {
            return axum::Json(serde_json::json!({
                "status": "error",
                "error": "workflow execution not configured"
            }))
            .into_response();
        };

        // Compile-only: the active runtime is never mutated.
        match publication.validate_workflows(&wire) {
            Ok(snapshot) => plan_response("validated", &snapshot).into_response(),
            Err(e) => axum::Json(serde_json::json!({
                "status": "error",
                "error": e
            }))
            .into_response(),
        }
    }

    /// Run payload from the control plane: the workflow id + the raw JSON
    /// request body fed to the workflow's Input node.
    #[derive(serde::Deserialize)]
    struct RunRequest {
        workflow_id: String,
        body: serde_json::Value,
    }

    /// Optional query params on `/run`. `stream=true` requests SSE
    /// token-by-token streaming instead of a buffered JSON envelope.
    #[derive(serde::Deserialize)]
    struct RunParams {
        #[serde(default)]
        stream: bool,
    }

    /// Execute a workflow from the CURRENT published snapshot (compile-time
    /// validation, never a live draft). This is the frontend's Run contract:
    /// it reuses `execute_workflow` — the same path the proxy's workflow
    /// routes take — so results, per-lane pools, cancellation and typed
    /// errors are identical to production traffic. A request for a workflow
    /// that is not in the snapshot (unpublished or unknown) is a 404; a
    /// runtime failure (provider error, timeout, invalid plan) maps through
    /// the gateway's typed error → HTTP mapping to real 4xx/5xx.
    ///
    /// With `?stream=true` the response is `text/event-stream`: `token`
    /// events carry text deltas as they arrive from the upstream provider,
    /// followed by a final `done` event with the complete envelope (or an
    /// `error` event on failure).
    async fn run(
        State(state): State<AdminState>,
        headers: http::HeaderMap,
        req_uri: axum::http::Uri,
        axum::Json(req): axum::Json<RunRequest>,
    ) -> Result<axum::response::Response, crate::errors::GatewayError> {
        // Parse ?stream=true from the query string (URL-decode safe).
        let params = RunParams {
            stream: req_uri
                .query()
                .map(|q| {
                    url::form_urlencoded::parse(q.as_bytes())
                        .any(|(k, v)| k == "stream" && (v == "true" || v == "1"))
                })
                .unwrap_or(false),
        };
        if check_auth(&headers, &state.api_key).is_err() {
            return Err(crate::errors::GatewayError::InvalidRequest {
                status: http::StatusCode::UNAUTHORIZED,
                message: "invalid or missing API key".into(),
            });
        }

        let Some(publication) = state.publication else {
            return Err(crate::errors::GatewayError::Internal(
                "workflow execution not configured".into(),
            ));
        };
        let snapshot = publication
            .snapshot()
            .ok_or_else(|| crate::errors::GatewayError::Internal("no published snapshot".into()))?;

        let plan = snapshot.get_plan(&req.workflow_id).ok_or_else(|| {
            crate::errors::GatewayError::InvalidRequest {
                status: http::StatusCode::NOT_FOUND,
                message: format!("no published workflow named '{}'", req.workflow_id),
            }
        })?;

        let request_id = uuid::Uuid::new_v4().to_string();

        // Encode the JSON body exactly as `workflow_route_request` does so the
        // run path observes identical bytes.
        let body_bytes: bytes::Bytes = match serde_json::to_vec(&req.body) {
            Ok(b) => b.into(),
            Err(e) => {
                return Err(crate::errors::GatewayError::InvalidRequest {
                    status: http::StatusCode::BAD_REQUEST,
                    message: format!("invalid request body json: {e}"),
                });
            }
        };

        // ponytail: per-run Hyper client (human-paced UI runs). The shared
        // proxy client lives on AppState and isn't reachable from the admin
        // router's state type; thread it through if run throughput ever needs
        // connection reuse.
        let client = workflow_runtime::gateway_client(std::time::Duration::from_secs(90), 64);

        // 120s hard deadline for admin-triggered runs — prevents orphaned
        // workflow executions from running indefinitely.
        let deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(120));

        if params.stream {
            let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(64);
            // When the browser aborts, this receiver's channel goes away; the
            // stream then ends, and the run task cancels its token so the node
            // runtime aborts the upstream request instead of streaming it to
            // completion (token burn after cancel).
            let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

            let wf_id = req.workflow_id.clone();
            let wf_id2 = wf_id.clone();
            let snap = snapshot.clone();
            let pl = plan.clone();
            let rid = request_id.clone();
            let bb = body_bytes;
            let extension_registry = publication.current_extensions();

            tokio::spawn(async move {
                let cancel = tokio_util::sync::CancellationToken::new();

                // Not-yet-cancelled flag shared with the completion block:
                // once the browser aborts (receiver dropped), `tx.closed()`
                // fires and the token cancels the in-flight upstream request.
                let cancel_run = cancel.clone();

                let result = tokio::select! {
                    r = crate::execution::execute_workflow(
                        &snap,
                        &pl,
                        bb,
                        &wf_id,
                        &rid,
                        client,
                        None,
                        deadline,
                        Some(tx.clone()),
                        cancel_run,
                        extension_registry,
                    ) => r,
                    _ = tx.closed() => {
                        cancel.cancel();
                        return;
                    }
                };

                match result {
                    Ok(response) => {
                        let body_result =
                            http_body_util::BodyExt::collect(response.into_body()).await;
                        match body_result {
                            Ok(collected) => {
                                let bytes = collected.to_bytes();

                                // Fail closed on non-JSON output: an unexpected
                                // (non-JSON) body must not ship a successful done
                                // with null output and mask corrupted data.
                                let output: serde_json::Value = match serde_json::from_slice(&bytes)
                                {
                                    Ok(v) => v,
                                    Err(e) => {
                                        let err = serde_json::json!({
                                            "error": format!("workflow returned non-JSON output: {e}")
                                        });
                                        let wire = protocol_core::sse::format_sse_event(
                                            &err.to_string(),
                                            Some("error"),
                                        );
                                        let _ = tx.send(bytes::Bytes::from(wire)).await;
                                        return;
                                    }
                                };
                                let envelope = serde_json::json!({
                                    "request_id": rid,
                                    "workflow_id": wf_id2,
                                    "workflow_version": snap.workflow_version_for(&wf_id2).unwrap_or(0),
                                    "snapshot_version": snap.version(),
                                    "plan_hash": snap.plan_hash_for(&wf_id2).unwrap_or_default(),
                                    "output": output,
                                });
                                let wire = protocol_core::sse::format_sse_event(
                                    &envelope.to_string(),
                                    Some("done"),
                                );
                                let _ = tx.send(bytes::Bytes::from(wire)).await;
                            }
                            Err(e) => {
                                let err = serde_json::json!({
                                    "error": format!("failed to read response body: {e}")
                                });
                                let wire = protocol_core::sse::format_sse_event(
                                    &err.to_string(),
                                    Some("error"),
                                );
                                let _ = tx.send(bytes::Bytes::from(wire)).await;
                            }
                        }
                    }
                    Err(e) => {
                        let err = serde_json::json!({"error": e.to_string()});
                        let wire =
                            protocol_core::sse::format_sse_event(&err.to_string(), Some("error"));
                        let _ = tx.send(bytes::Bytes::from(wire)).await;
                    }
                }
                // tx drops here → stream ends
            });

            let sse_body = axum::body::Body::from_stream(stream.map(Ok::<_, std::io::Error>));
            let resp = axum::response::Response::builder()
                .status(200)
                .header(http::header::CONTENT_TYPE, "text/event-stream")
                .header(http::header::CACHE_CONTROL, "no-cache")
                .body(sse_body);
            return resp.map_err(|e| {
                crate::errors::GatewayError::Internal(format!("failed to build SSE response: {e}"))
            });
        }

        // Buffered (non-streaming) path — unchanged.
        let extension_registry = publication.current_extensions();
        let response = crate::execution::execute_workflow(
            &snapshot,
            plan,
            body_bytes,
            &req.workflow_id,
            &request_id,
            client,
            None,
            deadline,
            None,
            tokio_util::sync::CancellationToken::new(),
            extension_registry,
        )
        .await?;

        let body_bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .map_err(|e| {
                crate::errors::GatewayError::Internal(format!(
                    "failed to read workflow output: {e}"
                ))
            })?
            .to_bytes();
        let output: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Err(crate::errors::GatewayError::Internal(format!(
                    "workflow returned non-JSON output: {e}"
                )));
            }
        };

        Ok(axum::Json(serde_json::json!({
            "status": "ok",
            "request_id": request_id,
            "workflow_id": req.workflow_id,
            "workflow_version": snapshot.workflow_version_for(&req.workflow_id).unwrap_or(0),
            "snapshot_version": snapshot.version(),
            "plan_hash": snapshot.plan_hash_for(&req.workflow_id).unwrap_or_default(),
            "output": output,
        }))
        .into_response())
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
        .route("/run", post(run))
        .layer(axum::Extension(handle))
        .with_state(AdminState {
            publication,
            api_key,
        })
}
