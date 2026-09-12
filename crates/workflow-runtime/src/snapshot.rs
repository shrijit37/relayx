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
use crate::provider::ProviderEntry;

/// Immutable snapshot of all runtime state.
///
/// Constructed by the control plane and atomically published to the data
/// plane. A snapshot is never mutated after construction — a new version
/// replaces the old via `Arc` swap.
#[derive(Debug)]
pub struct RuntimeSnapshot {
    /// Monotonically increasing version for cache invalidation.
    wall_version: u64,
    /// Pre-compiled workflow plans keyed by workflow id.
    plans: HashMap<String, Arc<ExecutionPlan>>,
    /// Registered lanes.
    lanes: Arc<LaneRegistry>,
    /// Registered providers.
    providers: HashMap<String, Arc<ProviderEntry>>,
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
            providers: HashMap::new(),
            published_at: Instant::now(),
        }
    }

    /// Wall version.
    pub fn version(&self) -> u64 {
        self.wall_version
    }

    /// Look up a pre-compiled plan by workflow id.
    pub fn get_plan(&self, workflow_id: &str) -> Option<&Arc<ExecutionPlan>> {
        self.plans.get(workflow_id)
    }

    /// Get the plan hash for a workflow id (for observability).
    pub fn plan_hash_for(&self, workflow_id: &str) -> Option<&str> {
        self.plans.get(workflow_id).map(|p| p.plan_hash())
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

    /// Look up a provider by id.
    pub fn get_provider(&self, provider_id: &str) -> Option<&Arc<ProviderEntry>> {
        self.providers.get(provider_id)
    }

    /// Time since publication.
    pub fn age(&self) -> std::time::Duration {
        self.published_at.elapsed()
    }

    /// Number of compiled plans in the snapshot.
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    /// Number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

/// Builder for constructing a snapshot before publication.
pub struct RuntimeSnapshotBuilder {
    wall_version: u64,
    plans: HashMap<String, Arc<ExecutionPlan>>,
    lanes: Arc<LaneRegistry>,
    providers: HashMap<String, Arc<ProviderEntry>>,
}

impl RuntimeSnapshotBuilder {
    /// Start building a snapshot with a version.
    pub fn new(wall_version: u64) -> Self {
        Self {
            wall_version,
            plans: HashMap::new(),
            lanes: Arc::new(LaneRegistry::new()),
            providers: HashMap::new(),
        }
    }

    /// Insert a lane registry.
    pub fn with_lanes(mut self, lanes: Arc<LaneRegistry>) -> Self {
        self.lanes = lanes;
        self
    }

    /// Register a compiled plan under a workflow id.
    pub fn with_plan(mut self, workflow_id: impl Into<String>, plan: ExecutionPlan) -> Self {
        self.plans.insert(workflow_id.into(), Arc::new(plan));
        self
    }

    /// Register a provider entry.
    pub fn with_provider(mut self, entry: ProviderEntry) -> Self {
        self.providers.insert(entry.id.clone(), Arc::new(entry));
        self
    }

    /// Build the immutable snapshot.
    pub fn build(self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            wall_version: self.wall_version,
            plans: self.plans,
            lanes: self.lanes,
            providers: self.providers,
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
