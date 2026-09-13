//! Gateway ↔ workflow-runtime bridge.
//!
//! Executes a compiled workflow plan from within the gateway's proxy path.
//! Translates the inbound HTTP request into a `RuntimeValue`, feeds it
//! through the plan, and returns the result as JSON.

use std::sync::Arc;

use axum::body::Body;
use http::StatusCode;

use crate::errors::GatewayError;
use workflow_runtime::execution::{ExecutionPlan, PlanClassification};
use workflow_runtime::nodes::{NodeInput, RuntimeValue};
use workflow_runtime::{ExecutionContext, RuntimeSnapshot};

/// Type alias for the gateway's shared HTTP client.
pub type GatewayClient =
    hyper_util::client::legacy::Client<hyper_util::client::legacy::connect::HttpConnector, Body>;

/// Execute a workflow plan in-process and return the result as an HTTP response.
///
/// The snapshot supplies the lane registry, the pre-compiled plan, and the
/// execution metadata (snapshot version + plan hash). The request body is
/// decoded as JSON and wrapped in a `RuntimeValue::Json`. The provided HTTP
/// client is injected so LLM nodes can reach their upstream providers;
/// `lane_clients` (when present) resolves a per-lane pool for each lane.
// ponytail: 8 params is at the ceiling; group into a RequestSpec struct if another is added.
#[allow(clippy::too_many_arguments)]
pub async fn execute_workflow(
    snapshot: &Arc<RuntimeSnapshot>,
    plan: &ExecutionPlan,
    request_body: Bytes,
    workflow_id: &str,
    request_id: &str,
    client: Arc<GatewayClient>,
    lane_clients: Option<Arc<dyn workflow_runtime::AsLaneClient>>,
    deadline: Option<tokio::time::Instant>,
) -> Result<axum::response::Response<Body>, GatewayError> {
    // Decode request body.
    let input_json: serde_json::Value =
        serde_json::from_slice(&request_body).map_err(|e| GatewayError::InvalidRequest {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid request body: {e}"),
        })?;

    let input = NodeInput::message(RuntimeValue::Json(input_json));

    // Build execution context with the real HTTP client and snapshot identity.
    let mut ctx = ExecutionContext::new(
        workflow_id.to_string(),
        request_id.to_string(),
        snapshot.lanes_arc(),
    );
    ctx.upstream_client = Some(client);
    ctx.lane_clients = lane_clients;
    ctx.snapshot = Some(snapshot.clone());
    ctx.deadline = deadline; // NEW: propagate execution deadline
    ctx.metadata = workflow_runtime::ExecutionMetadata::from_snapshot(snapshot, workflow_id);
    ctx.reporter = Arc::new(GatewayMilestones);

    // Dispatch to the appropriate execution path.
    let output = match plan.classification() {
        PlanClassification::FastPathSimple | PlanClassification::FastPathTranslated => {
            workflow_runtime::fast_path::execute_fast_path(plan, &ctx, input).await
        }
        PlanClassification::WorkflowExecution => {
            let rt = workflow_runtime::NodeRuntime::new(plan.clone());
            rt.execute(&ctx, input).await
        }
    }
    .map_err(|e| GatewayError::Internal(format!("workflow execution failed: {e}")))?;

    // Serialize response.
    let response_json = output.value.to_json();
    let body: Vec<u8> = serde_json::to_vec(&response_json)
        .map_err(|e| GatewayError::Internal(format!("failed to serialize response: {e}")))?;

    let resp: Result<_, http::Error> = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body));
    resp.map_err(|e| GatewayError::Internal(format!("failed to build response: {e}")))
}

/// Gateway-side milestone reporter: records per-node execution outcomes as
/// Prometheus counters so workflow runs are observable without logging
/// prompts or completions.
pub struct GatewayMilestones;

impl workflow_runtime::MilestoneReporter for GatewayMilestones {
    fn node_completed(&self, node_id: &str, output_port: Option<&str>) {
        metrics::counter!(
            "relayx_node_completed_total",
            "node" => node_id.to_owned(),
            "port" => output_port.unwrap_or("out").to_owned(),
        )
        .increment(1);
    }

    fn node_failed(&self, node_id: &str, error: &str) {
        // Cap the error label to its first 96 chars so a high-cardinality
        // error stream (per-429 body text, etc.) cannot grow the metric
        // cardinality without bound or leak upstream response bodies.
        let truncated: String = error.chars().take(96).collect();
        metrics::counter!(
            "relayx_node_failed_total",
            "node" => node_id.to_owned(),
            "error" => truncated,
        )
        .increment(1);
    }
}

use bytes::Bytes;
