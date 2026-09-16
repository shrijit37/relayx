//! Execution IR and node runtime.
//!
//! The `ExecutionPlan` is a flat, validated, pre-computed execution graph.
//! The `NodeRuntime` executes nodes in topological order with port-based
//! data routing and conditional edge evaluation.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::ExecutionContext;
use crate::error::{NodeError, WorkflowError};
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
use workflow_schema::{NodeConfig, NodeId, NodeKind};

// ─── Edge conditions ────────────────────────────────────────────────────────

/// A condition that gates an execution edge.
#[derive(Debug, Clone)]
pub enum EdgeCondition {
    /// True when the source node's output field equals a value.
    FieldEquals {
        field: String,
        value: serde_json::Value,
    },
    /// True when the source node's output port name equals a value.
    PortEquals {
        port: String,
        value: serde_json::Value,
    },
}

impl EdgeCondition {
    /// Evaluate this condition against a node's output port and value.
    fn evaluate(&self, output_port: Option<&str>, output_value: &RuntimeValue) -> bool {
        match self {
            EdgeCondition::FieldEquals { field, value } => {
                let field_val = output_value.get_field(field);
                match field_val {
                    Some(rv) => rv.to_json() == *value,
                    None => false,
                }
            }
            EdgeCondition::PortEquals { port, value } => {
                let port_matches = output_port == Some(port.as_str());
                if value.as_bool() == Some(true) {
                    port_matches
                } else {
                    !port_matches
                }
            }
        }
    }
}

// ─── Execution IR types ─────────────────────────────────────────────────────

/// Version of the Execution IR format. Bump on any structural change to the
/// plan that would invalidate previously compiled plans.
pub const PLAN_VERSION: u64 = 1;

/// Classification of a compiled plan for execution-path selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanClassification {
    /// Trivial pipeline: input → single LLM → output. Bypasses the interpreter.
    FastPathSimple,
    /// Linear pipeline that requires protocol translation.
    FastPathTranslated,
    /// Non-trivial graph (conditions, routers, multiple LLMs, fallback, retry).
    WorkflowExecution,
}

/// Pre-resolved metadata for fast-path plans, computed at compile time.
///
/// Carries the *real* `LlmConfig` from the plan node so the fast path
/// behaves identically to the interpreter (same protocol, streaming flag,
/// lane, and model).
#[derive(Debug, Clone)]
pub struct FastPathMetadata {
    /// Node id of the single LLM node.
    pub llm_node_id: String,
    /// The LLM node's actual configuration.
    pub llm_config: workflow_schema::LlmConfig,
}

/// A compiled execution node — the runtime representation of a workflow node.
#[derive(Debug, Clone)]
pub struct ExecNode {
    /// Unique node ID.
    pub id: NodeId,
    /// The kind of node.
    pub kind: NodeKind,
    /// Configuration payload.
    pub config: NodeConfig,
    /// Node IDs that feed into this node.
    pub upstream: Vec<NodeId>,
    /// Node IDs this node feeds into.
    pub downstream: Vec<NodeId>,
}

/// A compiled edge — represents data flow between nodes.
#[derive(Debug, Clone)]
pub struct ExecEdge {
    /// Source node ID.
    pub source: NodeId,
    /// Source port name.
    pub source_port: String,
    /// Target node ID.
    pub target: NodeId,
    /// Target port name.
    pub target_port: String,
    /// Optional condition that must be true for this edge to be active.
    pub condition: Option<EdgeCondition>,
}

/// A compiled execution plan — the runtime model of a validated workflow.
///
/// Immutable after compilation. Plans carry a version, a deterministic
/// content hash, and an execution classification so the runtime can select
/// the correct path without re-interpreting the workflow.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    nodes: Vec<ExecNode>,
    edges: Vec<ExecEdge>,
    /// Topological order indices: node_id → position in `nodes`.
    order: HashMap<NodeId, usize>,
    /// Edges indexed by source node.
    edges_from: HashMap<NodeId, Vec<usize>>,
    /// IR format version.
    plan_version: u64,
    /// Deterministic content hash of the compiled plan.
    plan_hash: String,
    /// Execution-path classification.
    classification: PlanClassification,
    /// Fast-path pre-resolved metadata (present only when classified fast path).
    fast_path: Option<FastPathMetadata>,
}

impl ExecutionPlan {
    /// Build an execution plan from a validated workflow definition.
    pub fn compile(workflow: &workflow_schema::Workflow) -> Result<Self, WorkflowError> {
        let mut node_map: HashMap<NodeId, ExecNode> = HashMap::new();

        for node in &workflow.nodes {
            node_map.insert(
                node.id.clone(),
                ExecNode {
                    id: node.id.clone(),
                    kind: node.kind.clone(),
                    config: node.config.clone(),
                    upstream: Vec::new(),
                    downstream: Vec::new(),
                },
            );
        }

        let mut edges = Vec::new();
        for edge in &workflow.edges {
            if let Some(source) = node_map.get_mut(&edge.source_node) {
                source.downstream.push(edge.target_node.clone());
            }
            if let Some(target) = node_map.get_mut(&edge.target_node) {
                target.upstream.push(edge.source_node.clone());
            }

            // Convert schema EdgeCondition to execution EdgeCondition.
            // For Condition nodes, edges with specific source ports
            // automatically get a PortEquals condition.
            let condition = edge
                .condition
                .as_ref()
                .map(|ec| EdgeCondition::FieldEquals {
                    field: ec.field.clone(),
                    value: ec.value.clone(),
                })
                .or_else(|| {
                    if let Some(source_node) = node_map.get(&edge.source_node)
                        && matches!(source_node.kind, NodeKind::Condition)
                    {
                        return Some(EdgeCondition::PortEquals {
                            port: edge.source_port.clone(),
                            value: serde_json::Value::Bool(true),
                        });
                    }
                    None
                });

            edges.push(ExecEdge {
                source: edge.source_node.clone(),
                source_port: edge.source_port.clone(),
                target: edge.target_node.clone(),
                target_port: edge.target_port.clone(),
                condition,
            });
        }

        // Topological sort via Kahn's algorithm.
        let sorted = topological_sort(&node_map)?;
        let order: HashMap<NodeId, usize> = sorted
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();

        // Apply the router output port count from the actual edge topology:
        // RoundRobin cycles over how many distinct output ports the router has.
        for node in node_map.values_mut() {
            if let NodeConfig::Router(ref mut rc) = node.config {
                let port_count = workflow
                    .edges
                    .iter()
                    .filter(|e| e.source_node == node.id)
                    .map(|e| &e.source_port)
                    .collect::<std::collections::HashSet<_>>()
                    .len();
                rc.output_ports = port_count.max(1);
            }
        }

        // Index edges by source node for fast lookup during execution.
        let mut edges_from: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (idx, edge) in edges.iter().enumerate() {
            edges_from.entry(edge.source.clone()).or_default().push(idx);
        }

        let mut nodes: Vec<ExecNode> = node_map.into_values().collect();
        nodes.sort_by_key(|n| order.get(&n.id).copied().unwrap_or(usize::MAX));

        let (classification, fast_path) = classify_plan(&nodes);
        let plan_hash = compute_plan_hash(&nodes, &edges);

        Ok(Self {
            nodes,
            edges,
            order,
            edges_from,
            plan_version: PLAN_VERSION,
            plan_hash,
            classification,
            fast_path,
        })
    }

    /// Return the execution nodes in topological order.
    pub fn nodes(&self) -> &[ExecNode] {
        &self.nodes
    }

    /// Return the compiled edges.
    pub fn edges(&self) -> &[ExecEdge] {
        &self.edges
    }

    /// IR format version.
    pub fn plan_version(&self) -> u64 {
        self.plan_version
    }

    /// Deterministic content hash of the plan.
    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

    /// Execution-path classification for this plan.
    pub fn classification(&self) -> PlanClassification {
        self.classification
    }

    /// Pre-resolved fast-path metadata, if this plan is classified fast path.
    pub fn fast_path(&self) -> Option<&FastPathMetadata> {
        self.fast_path.as_ref()
    }

    /// Get the topological index for a node.
    pub fn index_of(&self, node_id: &str) -> Option<usize> {
        self.order.get(node_id).copied()
    }

    /// Look up a node by ID.
    pub fn get_node(&self, node_id: &str) -> Option<&ExecNode> {
        self.nodes.iter().find(|n| n.id == node_id)
    }

    /// Get edges from a source node.
    fn edges_from(&self, source: &NodeId) -> &[usize] {
        self.edges_from.get(source).map_or(&[], |v| v.as_slice())
    }
}

/// Classify a compiled plan into its execution-path category and pre-resolve
/// fast-path metadata when the graph reduces to input → one LLM → output.
fn classify_plan(nodes: &[ExecNode]) -> (PlanClassification, Option<FastPathMetadata>) {
    let llm_nodes: Vec<&ExecNode> = nodes.iter().filter(|n| n.kind == NodeKind::Llm).collect();

    if llm_nodes.len() == 1 {
        let llm = llm_nodes[0];
        // A fast path must not branch (conditions/routers) nor involve
        // external capabilities (MCP/Skill nodes).
        let has_branching = nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Condition | NodeKind::Router));
        let has_external = nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Skill | NodeKind::Mcp | NodeKind::Custom));

        if !has_branching
            && !has_external
            && let NodeConfig::Llm(cfg) = &llm.config
        {
            let meta = FastPathMetadata {
                llm_node_id: llm.id.clone(),
                llm_config: cfg.clone(),
            };
            return (PlanClassification::FastPathSimple, Some(meta));
        }
    }

    (PlanClassification::WorkflowExecution, None)
}

/// Compute a deterministic content hash over the compiled nodes and edges.
fn compute_plan_hash(nodes: &[ExecNode], edges: &[ExecEdge]) -> String {
    let mut hasher = Sha256::new();
    for node in nodes {
        hasher.update(node.id.as_bytes());
        if let Ok(kind_json) = serde_json::to_vec(&node.kind) {
            hasher.update(kind_json);
        }
        if let Ok(config_json) = serde_json::to_vec(&node.config) {
            hasher.update(config_json);
        }
    }
    for edge in edges {
        hasher.update(edge.source.as_bytes());
        hasher.update(edge.source_port.as_bytes());
        hasher.update(edge.target.as_bytes());
        hasher.update(edge.target_port.as_bytes());
    }
    let digest = hasher.finalize();
    // Hex-encode for readability.
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Topological sort via Kahn's algorithm. Returns node IDs in execution order.
fn topological_sort(nodes: &HashMap<NodeId, ExecNode>) -> Result<Vec<NodeId>, WorkflowError> {
    let mut in_degree: HashMap<&NodeId, usize> = HashMap::new();
    let mut adj: HashMap<&NodeId, Vec<&NodeId>> = HashMap::new();

    for (id, node) in nodes {
        in_degree.entry(id).or_insert(0);
        for down in &node.downstream {
            adj.entry(id).or_default().push(down);
            *in_degree.entry(down).or_insert(0) += 0;
        }
    }

    // Recount in-degree properly from edges.
    in_degree.values_mut().for_each(|v| *v = 0);
    for (id, node) in nodes {
        in_degree.entry(id).or_insert(0);
        for down in &node.downstream {
            *in_degree.entry(down).or_insert(0) += 1;
        }
    }

    let mut queue: std::collections::VecDeque<&NodeId> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(id, _)| *id)
        .collect();

    let mut sorted = Vec::new();

    while let Some(node_id) = queue.pop_front() {
        sorted.push(node_id.clone());
        if let Some(neighbors) = adj.get(node_id) {
            for neighbor in neighbors {
                if let Some(deg) = in_degree.get_mut(neighbor) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    if sorted.len() != nodes.len() {
        return Err(WorkflowError::Validation(
            "workflow contains a cycle".into(),
        ));
    }

    Ok(sorted)
}

// ─── Port data store ────────────────────────────────────────────────────────

/// Per-run data store keyed by (node_id, port_name).
struct PortDataStore {
    data: HashMap<(NodeId, String), RuntimeValue>,
}

impl PortDataStore {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// Store a value for a specific node and port.
    fn store(&mut self, node_id: &str, port: &str, value: RuntimeValue) {
        self.data
            .insert((node_id.to_owned(), port.to_owned()), value);
    }

    /// Get a value for a specific node and port.
    fn get(&self, node_id: &str, port: &str) -> Option<&RuntimeValue> {
        self.data.get(&(node_id.to_owned(), port.to_owned()))
    }

    /// Get the first available value for a node (any port).
    fn get_first(&self, node_id: &str) -> Option<&RuntimeValue> {
        self.data
            .keys()
            .find(|(nid, _)| nid == node_id)
            .and_then(|key| self.data.get(key))
    }
}

// ─── Node runtime ───────────────────────────────────────────────────────────

/// The node runtime — executes a compiled plan.
pub struct NodeRuntime {
    plan: ExecutionPlan,
    /// Per-router round-robin counters (independent per node id).
    router_counters: HashMap<NodeId, AtomicUsize>,
}

impl NodeRuntime {
    /// Create a runtime from a compiled execution plan.
    pub fn new(plan: ExecutionPlan) -> Self {
        let router_counters = plan
            .nodes
            .iter()
            .filter(|n| matches!(n.config, NodeConfig::Router(_)))
            .map(|n| (n.id.clone(), AtomicUsize::new(0)))
            .collect();
        Self {
            plan,
            router_counters,
        }
    }

    /// Execute the workflow with the given input.
    ///
    /// Nodes execute in topological order. Each node receives data from its
    /// upstream nodes via port-based routing. Conditional edges are evaluated
    /// to determine active paths. Router nodes select downstream paths.
    pub async fn execute(
        &self,
        ctx: &ExecutionContext,
        input: NodeInput,
    ) -> Result<NodeOutput, WorkflowError> {
        let mut store = PortDataStore::new();

        // Store the workflow-level input.
        store.store("__input__", "out", input.value.clone());

        // Track which nodes should be skipped (downstream of inactive conditional edges).
        let mut skipped: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

        // Track the active output port for each node (used by condition/router).
        let mut active_port: HashMap<NodeId, Option<String>> = HashMap::new();

        for exec_node in &self.plan.nodes {
            // Check cancellation.
            if ctx.cancel_token.is_cancelled() {
                return Err(WorkflowError::Cancelled);
            }

            // Check deadline.
            if let Some(deadline) = ctx.deadline
                && tokio::time::Instant::now() > deadline
            {
                return Err(WorkflowError::Timeout(ctx.default_timeout));
            }

            // Skip nodes that are downstream of inactive conditional edges.
            if skipped.contains(&exec_node.id) {
                tracing::debug!(
                    node_id = %exec_node.id,
                    "skipping node (downstream of inactive edge)"
                );
                continue;
            }

            let node_ctx = ctx.for_node(&exec_node.id);

            // Gather inputs from upstream nodes via port-based routing.
            let node_input =
                gather_input(&exec_node.id, &exec_node.upstream, &self.plan.edges, &store);

            tracing::debug!(
                node_id = %exec_node.id,
                node_type = ?exec_node.kind,
                workflow_id = %ctx.workflow_id,
                run_id = %ctx.run_id,
                "executing node"
            );

            let start = std::time::Instant::now();
            let result =
                execute_node(exec_node, &node_ctx, node_input, &self.router_counters).await;
            let duration = start.elapsed();

            match result {
                Ok(output) => {
                    tracing::debug!(
                        node_id = %exec_node.id,
                        duration_ms = duration.as_millis(),
                        port = ?output.port,
                        "node completed"
                    );

                    ctx.reporter
                        .node_completed(&exec_node.id, output.port.as_deref());

                    let port_name = output.port.clone().unwrap_or_else(|| "out".to_owned());
                    store.store(&exec_node.id, &port_name, output.value);
                    active_port.insert(exec_node.id.clone(), output.port);

                    // Check conditional edges from this node.
                    let edge_indices = self.plan.edges_from(&exec_node.id);
                    for &edge_idx in edge_indices {
                        let edge = &self.plan.edges[edge_idx];
                        if let Some(ref condition) = edge.condition {
                            let output_port =
                                active_port.get(&exec_node.id).and_then(|p| p.as_deref());
                            let output_value =
                                store.get_first(&exec_node.id).cloned().unwrap_or_default();
                            if !condition.evaluate(output_port, &output_value) {
                                skipped.insert(edge.target.clone());
                                tracing::debug!(
                                    source = %exec_node.id,
                                    target = %edge.target,
                                    "conditional edge inactive, skipping target"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    ctx.reporter.node_failed(&exec_node.id, &e.to_string());
                    tracing::error!(
                        node_id = %exec_node.id,
                        error = %e,
                        duration_ms = duration.as_millis(),
                        "node execution failed"
                    );
                    return Err(WorkflowError::Runtime {
                        node_id: exec_node.id.clone(),
                        source: e,
                    });
                }
            }
        }

        // Return the output of the Output node. If it was skipped (due to inactive
        // conditional edges), that's a workflow execution error.
        let output_node = self.plan.nodes.iter().find(|n| n.kind == NodeKind::Output);
        if let Some(out) = output_node {
            if skipped.contains(&out.id) {
                return Err(WorkflowError::Validation(
                    "output node was skipped due to inactive conditional edges".into(),
                ));
            }
            store
                .get_first(&out.id)
                .cloned()
                .map(NodeOutput::message)
                .ok_or_else(|| WorkflowError::Validation("no output produced".into()))
        } else {
            Err(WorkflowError::Validation(
                "no output node in workflow".into(),
            ))
        }
    }
}

/// Gather input for a node from its upstream connections.
fn gather_input(
    node_id: &NodeId,
    upstream: &[NodeId],
    edges: &[ExecEdge],
    store: &PortDataStore,
) -> NodeInput {
    if upstream.is_empty() {
        // Input node — receive workflow-level input.
        return NodeInput::message(
            store
                .get("__input__", "out")
                .cloned()
                .unwrap_or(RuntimeValue::Null),
        );
    }

    // Find the edge that connects each upstream to this node, to get target_port.
    let mut port_values: Vec<(Option<String>, RuntimeValue)> = Vec::new();

    for up_id in upstream {
        // Find the edge from up_id → node_id.
        let edge = edges
            .iter()
            .find(|e| e.source == *up_id && e.target == *node_id);
        let target_port = edge.map(|e| e.target_port.clone());

        // Get the upstream output.
        if let Some(value) = store.get_first(up_id) {
            port_values.push((target_port, value.clone()));
        }
    }

    if port_values.len() == 1 {
        if let Some((port, value)) = port_values.into_iter().next() {
            NodeInput { port, value }
        } else {
            NodeInput::message(RuntimeValue::Null)
        }
    } else if port_values.is_empty() {
        NodeInput::message(RuntimeValue::Null)
    } else {
        // Multiple upstream — merge into an array.
        let values: Vec<serde_json::Value> = port_values.iter().map(|(_, v)| v.to_json()).collect();
        NodeInput::message(RuntimeValue::Json(serde_json::Value::Array(values)))
    }
}

/// Dispatch execution to the appropriate node handler.
async fn execute_node(
    node: &ExecNode,
    ctx: &ExecutionContext,
    input: NodeInput,
    router_counters: &HashMap<NodeId, AtomicUsize>,
) -> Result<NodeOutput, NodeError> {
    match &node.config {
        NodeConfig::Input(_) => Ok(NodeOutput::message(input.value)),
        NodeConfig::Output(_) => Ok(NodeOutput::message(input.value)),
        NodeConfig::Llm(config) => crate::nodes::llm::execute(config, ctx, input).await,
        NodeConfig::Transform(config) => crate::nodes::transform::execute(config, ctx, input).await,
        NodeConfig::Condition(config) => crate::nodes::condition::execute(config, ctx, input).await,
        NodeConfig::Router(config) => {
            let counter = router_counters.get(&node.id).ok_or_else(|| {
                NodeError::Internal("router node missing round-robin counter".into())
            })?;
            crate::nodes::router::execute(config, ctx, input, counter).await
        }
        NodeConfig::Mcp(config) => crate::nodes::mcp::execute(config, ctx, input).await,
        NodeConfig::Skill(config) => crate::nodes::skill::execute(config, ctx, input).await,
        NodeConfig::Fallback(config) => crate::nodes::fallback::execute(config, ctx, input).await,
        NodeConfig::Retry(config) => crate::nodes::retry::execute(config, ctx, input).await,
        NodeConfig::Custom(cfg) => {
            let registry = ctx.extension_registry.as_ref().ok_or_else(|| {
                NodeError::Extension(crate::error::ExtensionError::NotRegistered(format!(
                    "custom node kind '{}' requires an extension registry",
                    cfg.kind
                )))
            })?;
            let spec = registry.get(&cfg.kind).ok_or_else(|| {
                NodeError::Extension(crate::error::ExtensionError::NotRegistered(format!(
                    "no extension registered for kind '{}'",
                    cfg.kind
                )))
            })?;

            // Fail closed: a registered validator runs BEFORE the executor. A
            // validation failure rejects the node outright — the executor is
            // never invoked for an invalid config. Validators are optional;
            // without one the executor runs directly.
            if let Some(validator) = spec.validator.as_ref() {
                validator.validate(cfg).await.map_err(|e| {
                    NodeError::Extension(crate::error::ExtensionError::Validation(format!(
                        "custom node kind '{}' failed validation: {e}",
                        cfg.kind
                    )))
                })?;
            }

            spec.executor.execute(cfg, spec.version, input).await
        }
    }
}
