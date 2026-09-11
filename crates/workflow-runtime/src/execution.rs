//! Execution IR and node runtime.
//!
//! The `ExecutionPlan` is a flat, validated, pre-computed execution graph.
//! The `NodeRuntime` executes nodes in topological order.

use crate::context::ExecutionContext;
use crate::error::{NodeError, WorkflowError};
use crate::nodes::{NodeInput, NodeOutput};
use std::collections::HashMap;
use workflow_schema::{NodeConfig, NodeId, NodeKind};

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
    /// Target node ID.
    pub target: NodeId,
}

/// A compiled execution plan — the runtime model of a validated workflow.
#[derive(Debug)]
pub struct ExecutionPlan {
    /// All nodes in topological execution order.
    pub nodes: Vec<ExecNode>,
    /// All edges in the graph.
    pub edges: Vec<ExecEdge>,
    /// Topological order indices: node_id → position in `nodes`.
    order: HashMap<NodeId, usize>,
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
            edges.push(ExecEdge {
                source: edge.source_node.clone(),
                target: edge.target_node.clone(),
            });
        }

        // Topological sort via Kahn's algorithm.
        let sorted = topological_sort(&node_map)?;
        let order: HashMap<NodeId, usize> = sorted
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();

        let mut nodes: Vec<ExecNode> = node_map.into_values().collect();
        nodes.sort_by_key(|n| order.get(&n.id).copied().unwrap_or(usize::MAX));

        Ok(Self {
            nodes,
            edges,
            order,
        })
    }

    /// Get the topological index for a node.
    pub fn index_of(&self, node_id: &str) -> Option<usize> {
        self.order.get(node_id).copied()
    }

    /// Look up a node by ID.
    pub fn get_node(&self, node_id: &str) -> Option<&ExecNode> {
        self.nodes.iter().find(|n| n.id == node_id)
    }
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

/// The node runtime — executes a compiled plan.
pub struct NodeRuntime {
    plan: ExecutionPlan,
}

impl NodeRuntime {
    /// Create a runtime from a compiled execution plan.
    pub fn new(plan: ExecutionPlan) -> Self {
        Self { plan }
    }

    /// Execute the workflow with the given input.
    ///
    /// Nodes execute in topological order. Each node receives the output
    /// of its upstream nodes as input. The final node's output is returned.
    pub async fn execute(
        &self,
        ctx: &ExecutionContext,
        input: NodeInput,
    ) -> Result<NodeOutput, WorkflowError> {
        let mut node_outputs: HashMap<NodeId, NodeOutput> = HashMap::new();
        node_outputs.insert("__input__".to_owned(), NodeOutput::Message(input.to_json()));

        for exec_node in &self.plan.nodes {
            if ctx.cancel_token.is_cancelled() {
                return Err(WorkflowError::Cancelled);
            }

            let node_ctx = ctx.for_node(&exec_node.id);

            // Gather inputs from upstream nodes.
            let node_input = if exec_node.upstream.is_empty() {
                // Input node — receive workflow-level input.
                node_outputs
                    .remove("__input__")
                    .map(|o| NodeInput::Message(o.to_json()))
                    .unwrap_or(NodeInput::Message(serde_json::Value::Null))
            } else {
                // Collect outputs from upstream nodes.
                let upstream_data: Vec<serde_json::Value> = exec_node
                    .upstream
                    .iter()
                    .filter_map(|up_id| node_outputs.get(up_id))
                    .map(|o| o.to_json())
                    .collect();

                if upstream_data.len() == 1 {
                    NodeInput::Message(upstream_data.into_iter().next().unwrap_or_default())
                } else {
                    NodeInput::Message(serde_json::Value::Array(upstream_data))
                }
            };

            tracing::debug!(
                node_id = %exec_node.id,
                node_type = ?exec_node.kind,
                workflow_id = %ctx.workflow_id,
                run_id = %ctx.run_id,
                "executing node"
            );

            let start = std::time::Instant::now();
            let result = execute_node(exec_node, &node_ctx, node_input).await;
            let duration = start.elapsed();

            match result {
                Ok(output) => {
                    tracing::debug!(
                        node_id = %exec_node.id,
                        duration_ms = duration.as_millis(),
                        "node completed"
                    );
                    node_outputs.insert(exec_node.id.clone(), output);
                }
                Err(e) => {
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

        // Return the output of the last node (should be Output node).
        self.plan
            .nodes
            .last()
            .and_then(|n| node_outputs.remove(&n.id))
            .ok_or_else(|| WorkflowError::Validation("no output produced".into()))
    }
}

/// Dispatch execution to the appropriate node handler.
async fn execute_node(
    node: &ExecNode,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    match &node.config {
        NodeConfig::Input(_) => Ok(NodeOutput::Message(input.to_json())),
        NodeConfig::Output(_) => Ok(NodeOutput::Message(input.to_json())),
        NodeConfig::Llm(config) => crate::nodes::llm::execute(config, ctx, input).await,
        NodeConfig::Transform(config) => crate::nodes::transform::execute(config, ctx, input).await,
        NodeConfig::Condition(config) => crate::nodes::condition::execute(config, ctx, input).await,
        NodeConfig::Router(config) => crate::nodes::router::execute(config, ctx, input).await,
        NodeConfig::Mcp(config) => crate::nodes::mcp::execute(config, ctx, input).await,
        NodeConfig::Skill(config) => crate::nodes::skill::execute(config, ctx, input).await,
    }
}
