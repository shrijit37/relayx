//! Execution context — runtime information for node execution.

use std::sync::Arc;
use std::time::Duration;

/// Runtime context passed to every node during execution.
///
/// Contains no database fields — all state is memory-resident or
/// snapshot-based, keeping the hot path free of round trips.
pub struct ExecutionContext {
    /// The workflow this execution belongs to.
    pub workflow_id: String,
    /// Unique run identifier.
    pub run_id: String,
    /// The node currently executing.
    pub node_id: String,
    /// Cancellation token — checked by nodes that support cancellation.
    pub cancel_token: tokio_util::sync::CancellationToken,
    /// Optional deadline — nodes should abort if exceeded.
    pub deadline: Option<tokio::time::Instant>,
    /// Default timeout for individual node operations.
    pub default_timeout: Duration,
    /// Lane registry for LLM nodes to resolve provider connections.
    pub lane_registry: Arc<LaneRegistry>,
}

/// Registry of available lanes (provider/endpoint combinations).
///
/// Populated from the config snapshot at workflow startup time.
#[derive(Debug, Default)]
pub struct LaneRegistry {
    lanes: std::collections::HashMap<String, LaneEntry>,
}

/// A single lane entry.
#[derive(Debug, Clone)]
pub struct LaneEntry {
    /// Lane identifier.
    pub id: String,
    /// Base URL for the upstream provider.
    pub base_url: url::Url,
}

impl LaneRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a lane.
    pub fn register(&mut self, entry: LaneEntry) {
        self.lanes.insert(entry.id.clone(), entry);
    }

    /// Look up a lane by ID.
    pub fn get(&self, lane_id: &str) -> Option<&LaneEntry> {
        self.lanes.get(lane_id)
    }
}

impl ExecutionContext {
    /// Create a new execution context.
    pub fn new(workflow_id: String, run_id: String, lane_registry: Arc<LaneRegistry>) -> Self {
        Self {
            workflow_id,
            run_id,
            node_id: String::new(),
            cancel_token: tokio_util::sync::CancellationToken::new(),
            deadline: None,
            default_timeout: std::time::Duration::from_secs(60),
            lane_registry,
        }
    }

    /// Create a child context for a specific node.
    pub fn for_node(&self, node_id: &str) -> Self {
        Self {
            workflow_id: self.workflow_id.clone(),
            run_id: self.run_id.clone(),
            node_id: node_id.to_owned(),
            cancel_token: self.cancel_token.child_token(),
            deadline: self.deadline,
            default_timeout: self.default_timeout,
            lane_registry: self.lane_registry.clone(),
        }
    }
}
