//! Workflow definition types for the node-based runtime.
//!
//! Provides strongly typed representations for workflow graphs:
//! nodes, edges, ports, and validation. The visual editor (React Flow)
//! produces a workflow definition; this crate defines the canonical
//! in-memory representation that the compiler consumes.
//!
//! ```text
//! React Flow JSON → Workflow (this crate) → Validation → Execution IR
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ─── Identifiers ────────────────────────────────────────────────────────────

/// Opaque node identifier. Unique within a workflow.
pub type NodeId = String;

/// Opaque port name on a node.
pub type PortName = String;

// ─── Node model ─────────────────────────────────────────────────────────────

/// A single node in a workflow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier within the workflow.
    pub id: NodeId,
    /// The kind of execution this node performs.
    pub kind: NodeKind,
    /// Configuration payload (schema varies by `kind`).
    pub config: NodeConfig,
    /// Input ports this node accepts.
    #[serde(default)]
    pub inputs: Vec<PortDef>,
    /// Output ports this node produces.
    #[serde(default)]
    pub outputs: Vec<PortDef>,
}

/// The type of execution a node performs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Receives the initial input for the workflow.
    Input,
    /// Produces the final output of the workflow.
    Output,
    /// Calls an LLM provider via the protocol engine.
    Llm,
    /// Routes to one of several downstream paths.
    Router,
    /// Transforms data between nodes.
    Transform,
    /// Branches based on a condition.
    Condition,
    /// Invokes an MCP tool.
    Mcp,
    /// Loads and applies a Skill.
    Skill,
    /// Tries a sequence of providers until one succeeds.
    Fallback,
    /// Retries a downstream node's execution with configurable policy.
    Retry,
    /// A node kind reserved for externally registered node kinds; the runtime
    /// refuses to execute it (no extension registry is installed).
    Custom,
}

/// Configuration for a node. Variants correspond to `NodeKind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeConfig {
    Input(InputConfig),
    Output(OutputConfig),
    Llm(LlmConfig),
    Router(RouterConfig),
    Transform(TransformConfig),
    Condition(ConditionConfig),
    Mcp(McpConfig),
    Skill(SkillConfig),
    Fallback(FallbackConfig),
    Retry(RetryConfig),
    Custom(CustomConfig),
}

/// Configuration for an Input node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputConfig {
    /// Expected input port type.
    #[serde(default)]
    pub input_type: PortType,
}

/// Configuration for an Output node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output port type.
    #[serde(default)]
    pub output_type: PortType,
}

/// Configuration for an LLM node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Protocol to use (source-facing). If omitted, uses the lane's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Model override. If omitted, uses the lane's default model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Temperature override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Lane to route this request to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_id: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Configuration for a Router node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Routing strategy.
    #[serde(default)]
    pub strategy: RouterStrategy,
    /// Number of output ports, derived from the graph at compile time.
    /// Round-robin cycles through these; must be ≥ 1.
    #[serde(default = "default_output_ports")]
    pub output_ports: usize,
}

fn default_output_ports() -> usize {
    2
}

/// How a Router node selects its downstream path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterStrategy {
    /// First matching path.
    #[default]
    FirstMatch,
    /// Round-robin across paths.
    RoundRobin,
}

/// Configuration for a Transform node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransformConfig {
    /// The transform operation.
    #[serde(default)]
    pub operation: TransformOperation,
}

/// What a Transform node does to the data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformOperation {
    /// Pass data through unchanged.
    #[default]
    Passthrough,
    /// Extract a field from the data.
    Extract,
    /// Merge multiple inputs.
    Merge,
    /// Filter data.
    Filter,
}

/// Configuration for a Condition node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionConfig {
    /// The condition expression.
    pub condition: String,
    /// Field to evaluate.
    pub field: String,
    /// Comparison operator.
    pub operator: ConditionOp,
    /// Value to compare against.
    pub value: serde_json::Value,
}

/// Comparison operators for conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    Contains,
    NotContains,
    IsEmpty,
    IsNotEmpty,
}

/// Configuration for an MCP node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Reference to the MCP server.
    pub server_ref: String,
    /// Name of the tool to invoke.
    pub tool_name: String,
    /// Whether the tool schema is deferred (loaded on first use).
    #[serde(default)]
    pub deferred: bool,
}

/// Configuration for a Skill node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    /// Reference to the skill.
    pub skill_ref: String,
    /// Whether to use progressive loading (metadata → content).
    #[serde(default = "default_true")]
    pub progressive: bool,
}

/// Configuration for a Fallback node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Ordered list of provider lane ids to try.
    pub providers: Vec<FallbackProvider>,
    /// How many times the whole provider list is cycled for transient
    /// failures. `0` means try each provider exactly once.
    #[serde(default = "fallback_default_rounds")]
    pub rounds: u32,
}

fn fallback_default_rounds() -> u32 {
    1
}

/// A single provider entry in a fallback chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackProvider {
    /// Lane id.
    pub lane_id: String,
    /// Model override.
    pub model: String,
    /// Optional protocol override. If unset, defaults to OpenAI Chat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Configuration for a Retry node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (including the initial one).
    pub max_attempts: u32,
    /// Delay between retries in milliseconds.
    #[serde(default = "default_retry_delay_ms")]
    pub delay_ms: u64,
    /// Retry on timeout errors.
    #[serde(default = "default_true")]
    pub on_timeout: bool,
    /// Retry on provider errors (5xx / connection).
    #[serde(default = "default_true")]
    pub on_provider_error: bool,

    /// The LLM node configuration this retry re-invokes. Replaces the
    /// hard-coded "default" lane the previous implementation invented.
    pub target: LlmConfig,
}

fn default_retry_delay_ms() -> u64 {
    1000
}

/// Configuration for an externally registered (custom) node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomConfig {
    /// Opaque kind string identifying this custom node executor.
    pub kind: String,
    /// Opaque configuration passed through to the registered executor.
    #[serde(default)]
    pub payload: serde_json::Value,
}

// ─── Port model ─────────────────────────────────────────────────────────────

/// Definition of a port on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortDef {
    /// Port name (unique within the node).
    pub name: PortName,
    /// The type of data this port carries.
    #[serde(default)]
    pub port_type: PortType,
}

/// The type of data flowing through a port.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    /// A conversation message.
    #[default]
    Message,
    /// A stream of events.
    Stream,
    /// A tool call request.
    ToolCall,
    /// A tool call result.
    ToolResult,
    /// Arbitrary JSON.
    Json,
    /// Boolean value.
    Bool,
}

// ─── Edge model ─────────────────────────────────────────────────────────────

/// A connection between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Source node ID.
    pub source_node: NodeId,
    /// Source port name.
    pub source_port: PortName,
    /// Target node ID.
    pub target_node: NodeId,
    /// Target port name.
    pub target_port: PortName,
    /// Optional condition that must be true for this edge to be active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<EdgeCondition>,
}

/// A condition that gates an edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCondition {
    /// Field to evaluate on the source output.
    pub field: String,
    /// Comparison operator.
    pub operator: ConditionOp,
    /// Value to compare against.
    pub value: serde_json::Value,
}

// ─── Workflow ───────────────────────────────────────────────────────────────

/// A complete workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique workflow identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Version number (monotonically increasing).
    #[serde(default = "default_version")]
    pub version: u64,
    /// Nodes in the workflow.
    pub nodes: Vec<Node>,
    /// Edges connecting nodes.
    pub edges: Vec<Edge>,
}

fn default_version() -> u64 {
    1
}

// ─── Validation ─────────────────────────────────────────────────────────────

/// Errors produced during workflow validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("duplicate node id: '{0}'")]
    DuplicateNodeId(NodeId),

    #[error("edge references unknown source node: '{0}'")]
    UnknownSourceNode(NodeId),

    #[error("edge references unknown target node: '{0}'")]
    UnknownTargetNode(NodeId),

    #[error("edge references unknown source port '{port}' on node '{node}'")]
    UnknownSourcePort { node: NodeId, port: PortName },

    #[error("edge references unknown target port '{port}' on node '{node}'")]
    UnknownTargetPort { node: NodeId, port: PortName },

    #[error("workflow has a cycle involving node: '{0}'")]
    CycleDetected(NodeId),

    #[error("no Input node found in workflow")]
    NoInputNode,

    #[error("no Output node found in workflow")]
    NoOutputNode,

    #[error("node '{0}' is not reachable from any Input node")]
    UnreachableNode(NodeId),

    #[error("node '{0}' does not lead to any Output node")]
    DeadEndNode(NodeId),

    #[error("workflow must have at least one node")]
    EmptyWorkflow,
}

impl Workflow {
    /// Validate the workflow graph. Checks:
    /// - No duplicate node IDs
    /// - All edge references point to existing nodes/ports
    /// - No cycles
    /// - Exactly one Input and one Output node
    /// - All nodes reachable from Input
    /// - All nodes lead to Output
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        // Empty workflow.
        if self.nodes.is_empty() {
            errors.push(ValidationError::EmptyWorkflow);
            return Err(errors);
        }

        // Duplicate node IDs.
        let mut seen_ids = std::collections::HashSet::new();
        for node in &self.nodes {
            if !seen_ids.insert(&node.id) {
                errors.push(ValidationError::DuplicateNodeId(node.id.clone()));
            }
        }

        // Build node map for lookups.
        let node_map: HashMap<&NodeId, &Node> = self.nodes.iter().map(|n| (&n.id, n)).collect();

        // Count Input/Output nodes.
        let input_count = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Input)
            .count();
        let output_count = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Output)
            .count();

        if input_count == 0 {
            errors.push(ValidationError::NoInputNode);
        }
        if output_count == 0 {
            errors.push(ValidationError::NoOutputNode);
        }

        // Edge validation.
        let mut adj: HashMap<&NodeId, Vec<&NodeId>> = HashMap::new();
        let mut reverse_adj: HashMap<&NodeId, Vec<&NodeId>> = HashMap::new();

        for edge in &self.edges {
            if let Some(source) = node_map.get(&edge.source_node) {
                // Check source port exists.
                if !source.outputs.iter().any(|p| p.name == edge.source_port) {
                    errors.push(ValidationError::UnknownSourcePort {
                        node: edge.source_node.clone(),
                        port: edge.source_port.clone(),
                    });
                }
            } else {
                errors.push(ValidationError::UnknownSourceNode(edge.source_node.clone()));
            }

            if let Some(target) = node_map.get(&edge.target_node) {
                // Check target port exists.
                if !target.inputs.iter().any(|p| p.name == edge.target_port) {
                    errors.push(ValidationError::UnknownTargetPort {
                        node: edge.target_node.clone(),
                        port: edge.target_port.clone(),
                    });
                }
            } else {
                errors.push(ValidationError::UnknownTargetNode(edge.target_node.clone()));
            }

            adj.entry(&edge.source_node)
                .or_default()
                .push(&edge.target_node);
            reverse_adj
                .entry(&edge.target_node)
                .or_default()
                .push(&edge.source_node);
        }

        // Cycle detection (DFS).
        if let Some(cycle_node) = detect_cycle(&node_map, &adj) {
            errors.push(ValidationError::CycleDetected(cycle_node.clone()));
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Reachability from Input nodes.
        let input_nodes: Vec<&NodeId> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Input)
            .map(|n| &n.id)
            .collect();

        let reachable_from_input = bfs_reachable(&input_nodes, &adj);
        for node in &self.nodes {
            if !reachable_from_input.contains(&node.id) {
                errors.push(ValidationError::UnreachableNode(node.id.clone()));
            }
        }

        // Reachability to Output nodes.
        let output_nodes: Vec<&NodeId> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Output)
            .map(|n| &n.id)
            .collect();

        let reachable_to_output = bfs_reachable(&output_nodes, &reverse_adj);
        for node in &self.nodes {
            if !reachable_to_output.contains(&node.id) {
                errors.push(ValidationError::DeadEndNode(node.id.clone()));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Detect cycles using DFS. Returns the first node in a cycle, or None.
fn detect_cycle<'a>(
    node_map: &HashMap<&'a NodeId, &'a Node>,
    adj: &HashMap<&'a NodeId, Vec<&'a NodeId>>,
) -> Option<&'a NodeId> {
    let mut visited = std::collections::HashSet::new();
    let mut in_stack = std::collections::HashSet::new();

    fn dfs<'b>(
        node: &'b NodeId,
        adj: &HashMap<&'b NodeId, Vec<&'b NodeId>>,
        visited: &mut std::collections::HashSet<&'b NodeId>,
        in_stack: &mut std::collections::HashSet<&'b NodeId>,
    ) -> Option<&'b NodeId> {
        if in_stack.contains(node) {
            return Some(node);
        }
        if visited.contains(node) {
            return None;
        }
        visited.insert(node);
        in_stack.insert(node);
        if let Some(neighbors) = adj.get(node) {
            for neighbor in neighbors {
                if let Some(cycle) = dfs(neighbor, adj, visited, in_stack) {
                    return Some(cycle);
                }
            }
        }
        in_stack.remove(node);
        None
    }

    for node_id in node_map.keys() {
        if let Some(cycle) = dfs(node_id, adj, &mut visited, &mut in_stack) {
            return Some(cycle);
        }
    }
    None
}

/// BFS reachability from a set of starting nodes.
fn bfs_reachable(
    starts: &[&NodeId],
    adj: &HashMap<&NodeId, Vec<&NodeId>>,
) -> std::collections::HashSet<NodeId> {
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    for start in starts {
        queue.push_back(*start);
        visited.insert((*start).clone());
    }
    while let Some(node) = queue.pop_front() {
        if let Some(neighbors) = adj.get(node) {
            for neighbor in neighbors {
                if visited.insert((*neighbor).clone()) {
                    queue.push_back(neighbor);
                }
            }
        }
    }
    visited
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn input_node(id: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Input,
            config: NodeConfig::Input(InputConfig::default()),
            inputs: vec![],
            outputs: vec![PortDef {
                name: "out".into(),
                port_type: PortType::Message,
            }],
        }
    }

    fn output_node(id: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Output,
            config: NodeConfig::Output(OutputConfig::default()),
            inputs: vec![PortDef {
                name: "in".into(),
                port_type: PortType::Message,
            }],
            outputs: vec![],
        }
    }

    fn llm_node(id: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Llm,
            config: NodeConfig::Llm(LlmConfig {
                protocol: None,
                model: Some("gpt-4".into()),
                temperature: None,
                max_tokens: None,
                stream: true,
                lane_id: None,
            }),
            inputs: vec![PortDef {
                name: "in".into(),
                port_type: PortType::Message,
            }],
            outputs: vec![PortDef {
                name: "out".into(),
                port_type: PortType::Message,
            }],
        }
    }

    fn edge(source: &str, target: &str) -> Edge {
        Edge {
            source_node: source.into(),
            source_port: "out".into(),
            target_node: target.into(),
            target_port: "in".into(),
            condition: None,
        }
    }

    #[test]
    fn test_valid_simple_workflow() {
        let wf = Workflow {
            id: "wf1".into(),
            name: "test".into(),
            version: 1,
            nodes: vec![input_node("in"), output_node("out")],
            edges: vec![edge("in", "out")],
        };
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn test_valid_llm_pipeline() {
        let wf = Workflow {
            id: "wf2".into(),
            name: "llm-pipeline".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("llm1"), output_node("out")],
            edges: vec![edge("in", "llm1"), edge("llm1", "out")],
        };
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn test_rejects_empty_workflow() {
        let wf = Workflow {
            id: "empty".into(),
            name: "empty".into(),
            version: 1,
            nodes: vec![],
            edges: vec![],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::EmptyWorkflow))
        );
    }

    #[test]
    fn test_rejects_duplicate_node_ids() {
        let wf = Workflow {
            id: "wf".into(),
            name: "dup".into(),
            version: 1,
            nodes: vec![input_node("a"), input_node("a")],
            edges: vec![],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::DuplicateNodeId(_)))
        );
    }

    #[test]
    fn test_rejects_no_input_node() {
        let wf = Workflow {
            id: "wf".into(),
            name: "no-input".into(),
            version: 1,
            nodes: vec![output_node("out")],
            edges: vec![],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::NoInputNode))
        );
    }

    #[test]
    fn test_rejects_no_output_node() {
        let wf = Workflow {
            id: "wf".into(),
            name: "no-output".into(),
            version: 1,
            nodes: vec![input_node("in")],
            edges: vec![],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::NoOutputNode))
        );
    }

    #[test]
    fn test_rejects_cycle() {
        let wf = Workflow {
            id: "wf".into(),
            name: "cycle".into(),
            version: 1,
            nodes: vec![input_node("a"), llm_node("b"), output_node("c")],
            edges: vec![
                edge("a", "b"),
                edge("b", "b"), // self-cycle
                edge("b", "c"),
            ],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::CycleDetected(_)))
        );
    }

    #[test]
    fn test_rejects_unknown_source_node() {
        let wf = Workflow {
            id: "wf".into(),
            name: "bad-edge".into(),
            version: 1,
            nodes: vec![input_node("in"), output_node("out")],
            edges: vec![edge("nonexistent", "out")],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::UnknownSourceNode(_)))
        );
    }

    #[test]
    fn test_rejects_unreachable_node() {
        // "orphan" node is not connected to Input.
        let wf = Workflow {
            id: "wf".into(),
            name: "unreachable".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("orphan"), output_node("out")],
            edges: vec![edge("in", "out")],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::UnreachableNode(_)))
        );
    }

    #[test]
    fn test_rejects_dead_end_node() {
        // "dead" node leads nowhere (no path to Output).
        let wf = Workflow {
            id: "wf".into(),
            name: "dead-end".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("dead"), output_node("out")],
            edges: vec![edge("in", "dead"), edge("in", "out")],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::DeadEndNode(_)))
        );
    }

    #[test]
    fn test_rejects_unknown_port() {
        let wf = Workflow {
            id: "wf".into(),
            name: "bad-port".into(),
            version: 1,
            nodes: vec![input_node("in"), output_node("out")],
            edges: vec![Edge {
                source_node: "in".into(),
                source_port: "nonexistent".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            }],
        };
        let errs = wf.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::UnknownSourcePort { .. }))
        );
    }

    #[test]
    fn test_condition_edge_valid() {
        let wf = Workflow {
            id: "wf".into(),
            name: "cond".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                llm_node("llm1"),
                llm_node("llm2"),
                output_node("out"),
            ],
            edges: vec![
                edge("in", "llm1"),
                Edge {
                    source_node: "llm1".into(),
                    source_port: "out".into(),
                    target_node: "llm2".into(),
                    target_port: "in".into(),
                    condition: Some(EdgeCondition {
                        field: "result".into(),
                        operator: ConditionOp::Contains,
                        value: serde_json::json!("keyword"),
                    }),
                },
                edge("llm2", "out"),
            ],
        };
        assert!(wf.validate().is_ok());
    }
}
