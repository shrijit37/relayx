//! Public runner functions for the control-plane / frontend boundary.
//!
//! Kept deliberately small: schema + runtime already do the work; these
//! functions only assemble it into the exact operations the frontend API
//! boundary and gateway publication need. No new machinery.

use std::sync::Arc;

use crate::context::{LaneEntry, LaneRegistry};
use crate::error::WorkflowError;
use crate::execution::ExecutionPlan;
use crate::extension::ExtensionRegistry;
use crate::snapshot::{RuntimeSnapshot, RuntimeSnapshotBuilder};

/// Compile a workflow against explicit lane URLs.
///
/// Registration of `lane_name → base_url` pairs is the only edition this
/// path adds; the compile itself delegates to `crate::compile_workflow`.
///
/// `extensions` is forwarded to the compile context so Custom nodes get
/// compile-time kind resolution (warnings only; runtime is the enforcement
/// point). Pass `None` to skip extension resolution.
pub fn compile_workflow_with_lanes(
    workflow: &workflow_schema::Workflow,
    lanes: &[(String, String)],
    extensions: Option<Arc<ExtensionRegistry>>,
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
            egress: "direct".into(),
            proxy_url: None,
        });
    }

    let ctx = crate::CompileContext {
        lanes: Arc::new(lane_registry),
        extensions,
    };
    crate::compile_workflow(workflow, &ctx).map_err(Into::into)
}

/// Build a runtime snapshot from one workflow and its lanes.
///
/// `extensions` is threaded for symmetry with [`compile_workflow_with_lanes`]
/// and so a snapshot carries the same extension metadata a compile saw.
pub fn build_snapshot(
    workflow_id: &str,
    plan: ExecutionPlan,
    lanes: &[(String, String)],
    extensions: Option<Arc<ExtensionRegistry>>,
) -> RuntimeSnapshot {
    let mut lane_registry = LaneRegistry::new();
    for (id, url_str) in lanes {
        if let Ok(base_url) = url::Url::parse(url_str) {
            lane_registry.register(LaneEntry {
                id: id.clone(),
                base_url,
                authorization: None,
                egress: "direct".into(),
                proxy_url: None,
            });
        }
    }

    let mut builder = RuntimeSnapshotBuilder::new(1)
        .with_lanes(Arc::new(lane_registry))
        .with_plan(workflow_id.to_owned(), plan);

    // Record extension metadata (kind + version) from the registry so the
    // snapshot observes the same extension set the compile path resolved.
    if let Some(registry) = extensions {
        for spec in registry.specs_snapshots() {
            builder = builder.with_extension(spec.kind, spec.version);
        }
    }

    builder.build()
}
