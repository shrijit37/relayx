//! Fast-path execution for simple workflows.
//!
//! When a plan reduces to Input → Llm → Output, the topological interpreter
//! is unnecessary. This module provides a direct execution path that skips
//! the port-data-store, edge evaluation, and per-node loop.

use crate::ExecutionPlan;
use crate::context::ExecutionContext;
use crate::error::WorkflowError;
use crate::execution::PlanClassification;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::LlmConfig;

/// Execute a plan via the fast path if possible.
///
/// Returns `Err(WorkflowError::Validation(...))` if the plan is not
/// classified as a fast path — callers should fall back to the full
/// interpreter.
pub async fn execute_fast_path(
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, WorkflowError> {
    match plan.classification() {
        PlanClassification::FastPathSimple | PlanClassification::FastPathTranslated => {}
        PlanClassification::WorkflowExecution => {
            return Err(WorkflowError::Validation("plan is not a fast path".into()));
        }
    }

    let meta = match plan.fast_path() {
        Some(m) => m,
        None => {
            return Err(WorkflowError::Validation(
                "fast path metadata missing".into(),
            ));
        }
    };

    let llm_config = LlmConfig {
        protocol: None,
        model: Some(meta.model.clone()),
        temperature: None,
        max_tokens: None,
        stream: false,
        lane_id: Some(meta.lane_id.clone()),
    };

    crate::nodes::llm::execute(&llm_config, ctx, input)
        .await
        .map_err(|e| WorkflowError::Runtime {
            node_id: meta.llm_node_id.clone(),
            source: e,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ExecutionPlan;
    use workflow_schema::*;

    fn llm_only_workflow() -> Workflow {
        Workflow {
            id: "fast".into(),
            name: "fast".into(),
            version: 1,
            nodes: vec![
                Node {
                    id: "in".into(),
                    kind: NodeKind::Input,
                    config: NodeConfig::Input(InputConfig::default()),
                    inputs: vec![],
                    outputs: vec![PortDef {
                        name: "out".into(),
                        port_type: PortType::Message,
                    }],
                },
                Node {
                    id: "llm1".into(),
                    kind: NodeKind::Llm,
                    config: NodeConfig::Llm(LlmConfig {
                        protocol: None,
                        model: Some("test".into()),
                        temperature: None,
                        max_tokens: None,
                        stream: true,
                        lane_id: Some("test-lane".into()),
                    }),
                    inputs: vec![PortDef {
                        name: "in".into(),
                        port_type: PortType::Message,
                    }],
                    outputs: vec![PortDef {
                        name: "out".into(),
                        port_type: PortType::Message,
                    }],
                },
                Node {
                    id: "out".into(),
                    kind: NodeKind::Output,
                    config: NodeConfig::Output(OutputConfig::default()),
                    inputs: vec![PortDef {
                        name: "in".into(),
                        port_type: PortType::Message,
                    }],
                    outputs: vec![],
                },
            ],
            edges: vec![
                Edge {
                    source_node: "in".into(),
                    source_port: "out".into(),
                    target_node: "llm1".into(),
                    target_port: "in".into(),
                    condition: None,
                },
                Edge {
                    source_node: "llm1".into(),
                    source_port: "out".into(),
                    target_node: "out".into(),
                    target_port: "in".into(),
                    condition: None,
                },
            ],
        }
    }

    #[test]
    fn simple_workflow_classifies_as_fast_path() {
        let wf = llm_only_workflow();
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        assert_eq!(plan.classification(), PlanClassification::FastPathSimple);
        assert!(plan.fast_path().is_some());
    }

    #[test]
    fn non_simple_workflow_not_fast_path() {
        let wf = Workflow {
            id: "complex".into(),
            name: "complex".into(),
            version: 1,
            nodes: vec![
                Node {
                    id: "in".into(),
                    kind: NodeKind::Input,
                    config: NodeConfig::Input(InputConfig::default()),
                    inputs: vec![],
                    outputs: vec![PortDef {
                        name: "out".into(),
                        port_type: PortType::Message,
                    }],
                },
                Node {
                    id: "cond".into(),
                    kind: NodeKind::Condition,
                    config: NodeConfig::Condition(ConditionConfig {
                        condition: "f".into(),
                        field: "f".into(),
                        operator: ConditionOp::Equal,
                        value: serde_json::json!("x"),
                    }),
                    inputs: vec![PortDef {
                        name: "in".into(),
                        port_type: PortType::Message,
                    }],
                    outputs: vec![
                        PortDef {
                            name: "true".into(),
                            port_type: PortType::Message,
                        },
                        PortDef {
                            name: "false".into(),
                            port_type: PortType::Message,
                        },
                    ],
                },
                Node {
                    id: "out".into(),
                    kind: NodeKind::Output,
                    config: NodeConfig::Output(OutputConfig::default()),
                    inputs: vec![PortDef {
                        name: "in".into(),
                        port_type: PortType::Message,
                    }],
                    outputs: vec![],
                },
            ],
            edges: vec![
                Edge {
                    source_node: "in".into(),
                    source_port: "out".into(),
                    target_node: "cond".into(),
                    target_port: "in".into(),
                    condition: None,
                },
                Edge {
                    source_node: "cond".into(),
                    source_port: "true".into(),
                    target_node: "out".into(),
                    target_port: "in".into(),
                    condition: None,
                },
            ],
        };
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        assert_eq!(plan.classification(), PlanClassification::WorkflowExecution);
        assert!(plan.fast_path().is_none());
    }

    #[test]
    fn plan_hash_deterministic_same_input() {
        let wf = llm_only_workflow();
        let h1 = match ExecutionPlan::compile(&wf) {
            Ok(p) => p.plan_hash().to_owned(),
            Err(e) => panic!("compile 1 failed: {e}"),
        };
        let h2 = match ExecutionPlan::compile(&wf) {
            Ok(p) => p.plan_hash().to_owned(),
            Err(e) => panic!("compile 2 failed: {e}"),
        };
        assert_eq!(h1, h2);
    }

    #[test]
    fn fast_path_refuses_non_fast_plan() {
        let wf = Workflow {
            id: "c".into(),
            name: "c".into(),
            version: 1,
            nodes: vec![
                Node {
                    id: "in".into(),
                    kind: NodeKind::Input,
                    config: NodeConfig::Input(InputConfig::default()),
                    inputs: vec![],
                    outputs: vec![PortDef {
                        name: "out".into(),
                        port_type: PortType::Message,
                    }],
                },
                Node {
                    id: "llm".into(),
                    kind: NodeKind::Llm,
                    config: NodeConfig::Llm(LlmConfig {
                        protocol: None,
                        model: None,
                        temperature: None,
                        max_tokens: None,
                        stream: false,
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
                },
                Node {
                    id: "llm2".into(),
                    kind: NodeKind::Llm,
                    config: NodeConfig::Llm(LlmConfig {
                        protocol: None,
                        model: None,
                        temperature: None,
                        max_tokens: None,
                        stream: false,
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
                },
                Node {
                    id: "out".into(),
                    kind: NodeKind::Output,
                    config: NodeConfig::Output(OutputConfig::default()),
                    inputs: vec![PortDef {
                        name: "in".into(),
                        port_type: PortType::Message,
                    }],
                    outputs: vec![],
                },
            ],
            edges: vec![
                Edge {
                    source_node: "in".into(),
                    source_port: "out".into(),
                    target_node: "llm".into(),
                    target_port: "in".into(),
                    condition: None,
                },
                Edge {
                    source_node: "llm".into(),
                    source_port: "out".into(),
                    target_node: "llm2".into(),
                    target_port: "in".into(),
                    condition: None,
                },
                Edge {
                    source_node: "llm2".into(),
                    source_port: "out".into(),
                    target_node: "out".into(),
                    target_port: "in".into(),
                    condition: None,
                },
            ],
        };
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        assert_eq!(plan.classification(), PlanClassification::WorkflowExecution);
    }
}
