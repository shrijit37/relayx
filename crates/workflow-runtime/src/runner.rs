//! Public runner functions for the control-plane / frontend boundary.
//!
//! Kept deliberately small: schema + runtime already do the work; these
//! functions only assemble it into the exact operations the frontend API
//! boundary and gateway publication need. No new machinery.

use std::sync::Arc;

use crate::context::{LaneEntry, LaneRegistry};
use crate::error::WorkflowError;
use crate::execution::ExecutionPlan;
use crate::snapshot::{RuntimeSnapshot, RuntimeSnapshotBuilder};

/// Compile a workflow against explicit lane URLs.
///
/// Registration of `lane_name → base_url` pairs is the only edition this
/// path adds; the compile itself delegates to `crate::compile_workflow`.
pub fn compile_workflow_with_lanes(
    workflow: &workflow_schema::Workflow,
    lanes: &[(String, String)],
) -> Result<ExecutionPlan, WorkflowError> {
    let mut lane_registry = LaneRegistry::new();
    for (id, url_str) in lanes {
        let base_url = url::Url::parse(url_str).map_err(|e| {
            WorkflowError::NodeConfig(format!("lane '{id}' has invalid base_url: {e}"))
        })?;
        lane_registry.register(LaneEntry {
            id: id.clone(),
            base_url,
            authorization: None,
        });
    }

    let ctx = crate::CompileContext {
        lanes: Arc::new(lane_registry),
    };
    crate::compile_workflow(workflow, &ctx).map_err(Into::into)
}

/// Build a runtime snapshot from one workflow and its lanes.
pub fn build_snapshot(
    workflow_id: &str,
    plan: ExecutionPlan,
    lanes: &[(String, String)],
) -> RuntimeSnapshot {
    let mut lane_registry = LaneRegistry::new();
    for (id, url_str) in lanes {
        if let Ok(base_url) = url::Url::parse(url_str) {
            lane_registry.register(LaneEntry {
                id: id.clone(),
                base_url,
                authorization: None,
            });
        }
    }

    RuntimeSnapshotBuilder::new(1)
        .with_lanes(Arc::new(lane_registry))
        .with_plan(workflow_id.to_owned(), plan)
        .build()
}
