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

/// Execute a workflow plan in-process and return the result as an HTTP response.
///
/// The snapshot supplies the lane registry and the pre-compiled plan. The
/// request body is decoded as JSON and wrapped in a `RuntimeValue::Json`.
/// The plan's output is serialized back as `application/json`.
pub async fn execute_workflow(
    snapshot: &Arc<RuntimeSnapshot>,
    plan: &ExecutionPlan,
    request_body: Bytes,
    workflow_id: &str,
    request_id: &str,
) -> Result<axum::response::Response<Body>, GatewayError> {
    // Decode request body.
    let input_json: serde_json::Value =
        serde_json::from_slice(&request_body).map_err(|e| GatewayError::InvalidRequest {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid request body: {e}"),
        })?;

    let input = NodeInput::message(RuntimeValue::Json(input_json));

    // Build execution context.
    let ctx = ExecutionContext::new(
        workflow_id.to_string(),
        request_id.to_string(),
        snapshot.lanes_arc(),
    );

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

use bytes::Bytes;
