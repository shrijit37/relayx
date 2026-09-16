//! Immutable runtime snapshot.
//!
//! Bundles all data-plane state needed to serve requests. Published atomically
//! via `Arc` swap. The gateway reads a single snapshot per request; no database
//! round-trips.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::context::LaneRegistry;
use crate::execution::ExecutionPlan;

/// A compiled plan plus the workflow version it was compiled from (the
/// ACTIVE version executed, carried out-of-band from the wire). One map
/// entry, one version — the snapshot never stores the version twice.
#[derive(Debug)]
struct PlanEntry {
    plan: Arc<ExecutionPlan>,
    workflow_version: u64,
}

/// Immutable snapshot of all runtime state.
///
/// Constructed by the control plane and atomically published to the data
/// plane. A snapshot is never mutated after construction — a new version
/// replaces the old via `Arc` swap.
#[derive(Debug)]
pub struct RuntimeSnapshot {
    /// Monotonically increasing version for cache invalidation.
    wall_version: u64,
    /// Pre-compiled workflow plans keyed by workflow id, each carrying the
    /// ACTIVE workflow version it was compiled from.
    plans: HashMap<String, PlanEntry>,
    /// Registered lanes.
    lanes: Arc<LaneRegistry>,
    /// When this snapshot was published.
    published_at: Instant,
}

impl RuntimeSnapshot {
    /// Create an empty snapshot (no workflows, lanes, or providers).
    pub fn empty(wall_version: u64) -> Self {
        Self {
            wall_version,
            plans: HashMap::new(),
            lanes: Arc::new(LaneRegistry::new()),
            published_at: Instant::now(),
        }
    }

    /// Wall version.
    pub fn version(&self) -> u64 {
        self.wall_version
    }

    /// Look up a pre-compiled plan by workflow id.
    pub fn get_plan(&self, workflow_id: &str) -> Option<&Arc<ExecutionPlan>> {
        self.plans.get(workflow_id).map(|e| &e.plan)
    }

    /// Get the plan hash for a workflow id (for observability).
    pub fn plan_hash_for(&self, workflow_id: &str) -> Option<&str> {
        self.plans.get(workflow_id).map(|e| e.plan.plan_hash())
    }

    /// The workflow version a plan was compiled from (the ACTIVE version
    /// executed); 0 when the publish path did not carry a version.
    pub fn workflow_version_for(&self, workflow_id: &str) -> Option<u64> {
        self.plans.get(workflow_id).map(|e| e.workflow_version)
    }

    /// All compiled workflow ids in this snapshot.
    pub fn workflow_ids(&self) -> impl Iterator<Item = &String> {
        self.plans.keys()
    }

    /// The lane registry.
    pub fn lanes(&self) -> &LaneRegistry {
        &self.lanes
    }

    /// The lane registry as an `Arc` (for execution contexts).
    pub fn lanes_arc(&self) -> Arc<LaneRegistry> {
        self.lanes.clone()
    }

    /// Time since publication.
    pub fn age(&self) -> std::time::Duration {
        self.published_at.elapsed()
    }

    /// Number of compiled plans in the snapshot.
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
}

/// Builder for constructing a snapshot before publication.
///
/// The gateway's `/publish` path (control-plane → data-plane seam) builds a
/// coherent snapshot from a wire bundle; tests build snapshots the same way.
pub struct RuntimeSnapshotBuilder {
    wall_version: u64,
    plans: HashMap<String, PlanEntry>,
    lanes: Arc<LaneRegistry>,
}

impl RuntimeSnapshotBuilder {
    /// Start building a snapshot with a version.
    pub fn new(wall_version: u64) -> Self {
        Self {
            wall_version,
            plans: HashMap::new(),
            lanes: Arc::new(LaneRegistry::new()),
        }
    }

    /// Insert a lane registry.
    pub fn with_lanes(mut self, lanes: Arc<LaneRegistry>) -> Self {
        self.lanes = lanes;
        self
    }

    /// Register a compiled plan under a workflow id. The plan is considered
    /// version 0 (no ACTIVE version) unless [`Self::with_plan_version`]
    /// records one afterwards.
    pub fn with_plan(mut self, workflow_id: impl Into<String>, plan: ExecutionPlan) -> Self {
        self.plans.insert(
            workflow_id.into(),
            PlanEntry {
                plan: Arc::new(plan),
                workflow_version: 0,
            },
        );
        self
    }

    /// Record the workflow version a plan was compiled from (the ACTIVE
    /// version), so runs report the exact version executed. Must be called
    /// after [`Self::with_plan`] for the same `workflow_id`; a version
    /// without a plan is meaningless and is ignored.
    pub fn with_plan_version(
        mut self,
        workflow_id: impl Into<String>,
        workflow_version: u64,
    ) -> Self {
        let id = workflow_id.into();
        debug_assert!(
            self.plans.contains_key(&id),
            "with_plan_version called before with_plan for '{id}'"
        );
        if let Some(entry) = self.plans.get_mut(&id) {
            entry.workflow_version = workflow_version;
        }
        self
    }

    /// Build the immutable snapshot.
    pub fn build(self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            wall_version: self.wall_version,
            plans: self.plans,
            lanes: self.lanes,
            published_at: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ExecutionPlan;
    use workflow_schema::*;

    fn input_node() -> Node {
        Node {
            id: "in".into(),
            kind: NodeKind::Input,
            config: NodeConfig::Input(InputConfig::default()),
            inputs: vec![],
            outputs: vec![PortDef {
                name: "out".into(),
                port_type: PortType::Message,
            }],
        }
    }

    fn output_node() -> Node {
        Node {
            id: "out".into(),
            kind: NodeKind::Output,
            config: NodeConfig::Output(OutputConfig::default()),
            inputs: vec![PortDef {
                name: "in".into(),
                port_type: PortType::Message,
            }],
            outputs: vec![],
        }
    }

    #[test]
    fn snapshot_empty_initially() {
        let snapshot = RuntimeSnapshot::empty(1);
        assert_eq!(snapshot.version(), 1);
        assert!(snapshot.get_plan("missing").is_none());
        assert_eq!(snapshot.plan_count(), 0);
    }

    #[test]
    fn builder_populates_snapshot() {
        let wf = Workflow {
            id: "wf1".into(),
            name: "test".into(),
            version: 1,
            nodes: vec![input_node(), output_node()],
            edges: vec![Edge {
                source_node: "in".into(),
                source_port: "out".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            }],
        };
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("plan compile failed: {e}"),
        };
        let snapshot = RuntimeSnapshotBuilder::new(42)
            .with_plan("wf1", plan)
            .build();
        assert_eq!(snapshot.version(), 42);
        assert!(snapshot.get_plan("wf1").is_some());
        assert_eq!(snapshot.plan_count(), 1);
    }
}
