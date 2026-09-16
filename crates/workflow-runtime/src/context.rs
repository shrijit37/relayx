//! Execution context — runtime information for node execution.

use std::sync::Arc;
use std::time::Duration;

use crate::extension::ExtensionRegistry;
use crate::nodes::RuntimeValue;
use crate::snapshot::RuntimeSnapshot;

/// Type alias for the gateway's shared HTTP client.
pub type GatewayHttpClient = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    axum::body::Body,
>;

/// Build a gateway HTTP client with keep-alive pooling and a bounded idle
/// pool. This is the single constructor for the plain upstream client —
/// the gateway's per-lane layer wraps it with `retry_canceled_requests`
/// off. Callers wanting the identical default client (admin `/run`,
/// workflow-runtime tests) use this instead of hand-rolling a builder.
pub fn gateway_client(idle_timeout: Duration, max_idle: usize) -> Arc<GatewayHttpClient> {
    Arc::new(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .pool_idle_timeout(idle_timeout)
            .pool_max_idle_per_host(max_idle)
            .build(hyper_util::client::legacy::connect::HttpConnector::new()),
    )
}

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
    /// HTTP client for upstream provider calls.
    pub upstream_client: Option<Arc<GatewayHttpClient>>,
    /// MCP tool executor — if provided, MCP nodes call real tools.
    pub mcp_executor: Option<Arc<dyn McpToolExecutor>>,
    /// Skill loader — if provided, Skill nodes load real skills.
    pub skill_loader: Option<Arc<dyn SkillLoader>>,
    /// The "lane-aware" client resolver: hands back a client per lane name.
    pub lane_clients: Option<Arc<dyn AsLaneClient>>,
    /// Execution metadata (snapshot version, plan hash) for observability.
    pub metadata: ExecutionMetadata,
    /// Snapshot/plan metadata exposed as capability context.
    pub snapshot: Option<Arc<crate::snapshot::RuntimeSnapshot>>,
    /// Optional reporter of execution milestones.
    pub reporter: Arc<dyn crate::milestone::MilestoneReporter>,
    /// Extension registry — maps custom node kinds to validator + executor.
    pub extension_registry: Option<Arc<ExtensionRegistry>>,
    /// Optional SSE wire-bytes sender for token-level streaming.
    /// When present, LLM nodes send formatted SSE token deltas through
    /// this channel as they arrive from upstream providers.
    pub token_sender: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>,
}

/// What snapshot/plan state an execution carries.
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetadata {
    /// Snapshot wall version.
    pub snapshot_version: u64,
    /// Plan hash of the compiled workflow.
    pub plan_hash: String,
    /// The workflow's own version (the ACTIVE version being executed).
    pub workflow_version: u64,
}

impl ExecutionMetadata {
    /// Attach snapshot+plan identity to an existing execution context.
    pub fn from_snapshot(snapshot: &RuntimeSnapshot, workflow_id: &str) -> Self {
        Self {
            snapshot_version: snapshot.version(),
            plan_hash: snapshot
                .plan_hash_for(workflow_id)
                .unwrap_or_default()
                .to_owned(),
            workflow_version: snapshot.workflow_version_for(workflow_id).unwrap_or(0),
        }
    }
}

// ─── Lane-aware client resolver ──────────────────────────────────────────────

/// Resolves an upstream HTTP client for a lane by name.
///
/// The data plane implements this extension trait over its lane snapshot map
/// so a node can obtain the connection pool bound to a specific lane without
/// the runtime knowing anything about per-lane pools.
pub trait AsLaneClient: Send + Sync {
    /// Client for the named lane, if that lane has a pool.
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<GatewayHttpClient>>;
}

// ─── MCP tool executor trait ────────────────────────────────────────────────

/// Trait for executing MCP tools. Implementations connect to real MCP servers.
#[async_trait::async_trait]
pub trait McpToolExecutor: Send + Sync {
    /// Execute an MCP tool and return the result.
    async fn execute_tool(
        &self,
        server_ref: &str,
        tool_name: &str,
        input: &RuntimeValue,
    ) -> Result<RuntimeValue, crate::error::NodeError>;
}

// ─── Skill loader trait ─────────────────────────────────────────────────────

/// Trait for loading and applying skills. Implementations connect to skill registries.
#[async_trait::async_trait]
pub trait SkillLoader: Send + Sync {
    /// Load a skill and return its content as a runtime value.
    async fn load_skill(&self, skill_ref: &str) -> Result<RuntimeValue, crate::error::NodeError>;
}

// ─── Lane registry ──────────────────────────────────────────────────────────

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
    /// Optional static authorization header value (e.g. `Bearer <token>`)
    /// attached to requests routed through this lane. Resolved at publish
    /// time from a `credential_ref` on the control plane — never stored in
    /// workflow JSON. `None` for lanes that carry no auth.
    pub authorization: Option<String>,
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

    /// Iterate over all registered lanes.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &LaneEntry)> {
        self.lanes.iter()
    }

    /// Number of registered lanes.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    /// Whether the registry has no lanes.
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
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
            upstream_client: None,
            mcp_executor: None,
            skill_loader: None,
            lane_clients: None,
            metadata: ExecutionMetadata::default(),
            snapshot: None,
            reporter: Arc::new(crate::milestone::NoopReporter),
            extension_registry: None,
            token_sender: None,
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
            upstream_client: self.upstream_client.clone(),
            mcp_executor: self.mcp_executor.clone(),
            skill_loader: self.skill_loader.clone(),
            lane_clients: self.lane_clients.clone(),
            metadata: self.metadata.clone(),
            snapshot: self.snapshot.clone(),
            reporter: self.reporter.clone(),
            extension_registry: self.extension_registry.clone(),
            token_sender: self.token_sender.clone(),
        }
    }
}
