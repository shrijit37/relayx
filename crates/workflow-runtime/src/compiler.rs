//! Workflow compiler with extended validation.
//!
//! The compiler wraps [`ExecutionPlan::compile`] and adds capability,
//! lane, and protocol validation on top of the schema-level checks.
//! Plans produced by the compiler are deterministic for the same input
//! and dependency versions.

use std::sync::Arc;

use crate::context::LaneRegistry;
use crate::error::WorkflowError;
use crate::execution::ExecutionPlan;
use workflow_schema::Workflow;

/// Validation context for the compiler.
///
/// Holds the data-plane registries the compiler needs to validate
/// references without performing any database lookups.
#[derive(Debug)]
pub struct CompileContext {
    /// Available lanes.
    pub lanes: Arc<LaneRegistry>,
}

/// Additional compiler-specific validation errors.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// Schema validation failed.
    #[error("schema validation failed: {0}")]
    Schema(String),

    /// A workflow references a lane that does not exist.
    #[error("lane '{0}' not found in registry")]
    LaneNotFound(String),

    /// The compiler failed internally.
    #[error("compilation failed: {0}")]
    Internal(String),

    /// The workflow itself is structurally invalid.
    #[error("workflow error: {0}")]
    Workflow(#[from] WorkflowError),
}

impl From<CompileError> for WorkflowError {
    fn from(e: CompileError) -> Self {
        match e {
            CompileError::Schema(msg) | CompileError::Internal(msg) => {
                WorkflowError::Validation(msg)
            }
            CompileError::LaneNotFound(id) => {
                WorkflowError::Validation(format!("lane '{id}' not found in registry"))
            }
            CompileError::Workflow(w) => w,
        }
    }
}

/// Compile a workflow with extended validation against the data-plane
/// registries.
///
/// # Validation steps
///
/// 1. Schema validation (duplicate IDs, cycles, reachability, ports).
/// 2. Lane reference validation (Llm nodes with explicit lane_id must resolve).
/// 3. Execution plan compilation (topological sort, classification, hashing).
pub fn compile_workflow(
    workflow: &Workflow,
    ctx: &CompileContext,
) -> Result<ExecutionPlan, CompileError> {
    // 1. Schema validation.
    workflow
        .validate()
        .map_err(|errs| CompileError::Schema(format!("{errs:?}")))?;

    // 2. Lane reference validation.
    validate_lane_refs(workflow, ctx)?;

    // 3. Full compilation via ExecutionPlan::compile.
    let plan = ExecutionPlan::compile(workflow)?;

    Ok(plan)
}

fn validate_lane_refs(workflow: &Workflow, ctx: &CompileContext) -> Result<(), CompileError> {
    use workflow_schema::{FallbackConfig, NodeConfig};

    for node in &workflow.nodes {
        match &node.config {
            NodeConfig::Llm(llm_cfg) => match llm_cfg.lane_id.as_deref() {
                Some(lane_id) => {
                    if ctx.lanes.get(lane_id).is_none() {
                        return Err(CompileError::LaneNotFound(lane_id.to_owned()));
                    }
                }
                // Lane-less LLM node: only unambiguous with exactly one lane.
                None => {
                    if ctx.lanes.is_empty() {
                        return Err(CompileError::Schema(
                            "LLM node has no lane_id and no lane is registered".into(),
                        ));
                    }
                    if ctx.lanes.len() > 1 {
                        return Err(CompileError::Schema(
                            "LLM node has no lane_id but multiple lanes exist; a lane_id is required"
                                .into(),
                        ));
                    }
                }
            },
            NodeConfig::Fallback(FallbackConfig { providers, .. }) => {
                for provider in providers {
                    if ctx.lanes.get(&provider.lane_id).is_none() {
                        return Err(CompileError::LaneNotFound(provider.lane_id.clone()));
                    }
                }
            }
            NodeConfig::Retry(rt_cfg) => {
                if let Some(lane_id) = rt_cfg.target.lane_id.as_deref()
                    && ctx.lanes.get(lane_id).is_none()
                {
                    return Err(CompileError::LaneNotFound(lane_id.to_owned()));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ExecutionContext, LaneEntry};
    use crate::nodes::{NodeInput, RuntimeValue};
    use std::sync::Arc;
    use url::Url;
    use workflow_schema::*;

    fn test_lane_registry() -> Arc<LaneRegistry> {
        let mut registry = LaneRegistry::new();
        let base_url = match Url::parse("http://127.0.0.1:9000") {
            Ok(u) => u,
            Err(e) => panic!("failed to parse test url: {e}"),
        };
        registry.register(LaneEntry {
            id: "test-lane".into(),
            base_url,
        });
        Arc::new(registry)
    }

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

    fn llm_node(id: &str, lane_id: Option<&str>) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Llm,
            config: NodeConfig::Llm(LlmConfig {
                protocol: None,
                model: Some("gpt-4".into()),
                temperature: None,
                max_tokens: None,
                stream: true,
                lane_id: lane_id.map(|s| s.into()),
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

    fn simple_edge(source: &str, target: &str) -> Edge {
        Edge {
            source_node: source.into(),
            source_port: "out".into(),
            target_node: target.into(),
            target_port: "in".into(),
            condition: None,
        }
    }

    #[test]
    fn compile_simple_workflow() {
        let wf = Workflow {
            id: "wf1".into(),
            name: "simple".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                llm_node("llm1", Some("test-lane")),
                output_node("out"),
            ],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let plan = match compile_workflow(&wf, &ctx) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        assert_eq!(plan.plan_version(), 1);
        assert!(!plan.plan_hash().is_empty());
    }

    #[test]
    fn compile_rejects_unknown_lane() {
        let wf = Workflow {
            id: "wf2".into(),
            name: "bad-lane".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                llm_node("llm1", Some("nonexistent")),
                output_node("out"),
            ],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let result = compile_workflow(&wf, &ctx);
        match result {
            Ok(_) => panic!("expected LaneNotFound"),
            Err(CompileError::LaneNotFound(id)) => assert_eq!(id, "nonexistent"),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn plan_hash_is_deterministic() {
        let wf = Workflow {
            id: "wf3".into(),
            name: "deterministic".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("llm1", None), output_node("out")],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let hash1 = match compile_workflow(&wf, &ctx) {
            Ok(p) => p.plan_hash().to_owned(),
            Err(e) => panic!("compile 1 failed: {e}"),
        };
        let hash2 = match compile_workflow(&wf, &ctx) {
            Ok(p) => p.plan_hash().to_owned(),
            Err(e) => panic!("compile 2 failed: {e}"),
        };
        assert_eq!(hash1, hash2, "same workflow must produce same hash");
    }

    #[test]
    fn lane_less_llm_with_no_lanes_is_rejected() {
        // A lane-less LLM node with ZERO lanes is unambiguous-but-impossible:
        // the runtime would have no lane to route through, so it must fail at
        // compile time (not 500 on the first request).
        let wf = Workflow {
            id: "wf-none".into(),
            name: "no-lanes".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("llm1", None), output_node("out")],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: Arc::new(LaneRegistry::new()),
        };
        let result = compile_workflow(&wf, &ctx);
        assert!(
            result.is_err(),
            "lane-less LLM with no lanes must fail to compile"
        );
    }

    #[test]
    fn lane_less_llm_with_multiple_lanes_is_rejected() {
        // A lane-less LLM node with MULTIPLE lanes is ambiguous — the runtime
        // must not guess. Require an explicit lane_id.
        let mut two_lanes = LaneRegistry::new();
        let a = match Url::parse("http://127.0.0.1:9001") {
            Ok(u) => u,
            Err(e) => panic!("invalid url: {e}"),
        };
        let b = match Url::parse("http://127.0.0.1:9002") {
            Ok(u) => u,
            Err(e) => panic!("invalid url: {e}"),
        };
        two_lanes.register(LaneEntry {
            id: "a".into(),
            base_url: a,
        });
        two_lanes.register(LaneEntry {
            id: "b".into(),
            base_url: b,
        });

        let wf = Workflow {
            id: "wf-two".into(),
            name: "two-lanes".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("llm1", None), output_node("out")],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: Arc::new(two_lanes),
        };
        let result = compile_workflow(&wf, &ctx);
        assert!(
            result.is_err(),
            "lane-less LLM with multiple lanes must fail to compile"
        );
    }

    #[test]
    fn fast_path_classification() {
        let wf = Workflow {
            id: "fp".into(),
            name: "fast-path".into(),
            version: 1,
            nodes: vec![input_node("in"), llm_node("llm1", None), output_node("out")],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let plan = match compile_workflow(&wf, &ctx) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        use crate::execution::PlanClassification;
        assert_eq!(plan.classification(), PlanClassification::FastPathSimple);
        assert!(plan.fast_path().is_some());
    }

    #[test]
    fn workflow_classification_with_condition() {
        let wf = Workflow {
            id: "wf-c".into(),
            name: "conditional".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                llm_node("llm1", None),
                Node {
                    id: "cond".into(),
                    kind: NodeKind::Condition,
                    config: NodeConfig::Condition(ConditionConfig {
                        condition: "field".into(),
                        field: "type".into(),
                        operator: ConditionOp::Equal,
                        value: serde_json::json!("a"),
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
                output_node("out"),
            ],
            edges: vec![
                simple_edge("in", "llm1"),
                simple_edge("llm1", "cond"),
                Edge {
                    source_node: "cond".into(),
                    source_port: "true".into(),
                    target_node: "out".into(),
                    target_port: "in".into(),
                    condition: None,
                },
            ],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let plan = match compile_workflow(&wf, &ctx) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        use crate::execution::PlanClassification;
        assert_eq!(plan.classification(), PlanClassification::WorkflowExecution);
        assert!(plan.fast_path().is_none());
    }

    #[tokio::test]
    async fn compiled_plan_executes() {
        let wf = Workflow {
            id: "exec".into(),
            name: "exec".into(),
            version: 1,
            nodes: vec![input_node("in"), output_node("out")],
            edges: vec![simple_edge("in", "out")],
        };
        let ctx = CompileContext {
            lanes: test_lane_registry(),
        };
        let plan = match compile_workflow(&wf, &ctx) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        let runtime = crate::execution::NodeRuntime::new(plan);
        let lanes = test_lane_registry();
        let exe_ctx = ExecutionContext::new("wf".into(), "run".into(), lanes);
        let out = match runtime
            .execute(
                &exe_ctx,
                NodeInput::message(RuntimeValue::String("hello".into())),
            )
            .await
        {
            Ok(o) => o,
            Err(e) => panic!("execution failed: {e}"),
        };
        assert_eq!(out.value, RuntimeValue::String("hello".into()));
    }
}
