//! Extension-boundary proof tests.
//!
//! These tests demonstrate the core definition of done for this phase:
//! adding a new node type and a new provider requires ZERO changes to the
//! core executor (scheduler, interpreter, compiler match arms).

use std::sync::Arc;

use async_trait::async_trait;
use url::Url;
use workflow_runtime::Capabilities;
use workflow_runtime::NodeExecutor;
use workflow_runtime::context::{ExecutionContext, LaneEntry, LaneRegistry};
use workflow_runtime::error::NodeError;
use workflow_runtime::execution::ExecutionPlan;
use workflow_runtime::nodes::{NodeInput, NodeOutput, NodeRegistry, RuntimeValue};

// ─── A foreign node implementation ─────────────────────────────────────────

/// A node that uppercases string input. Defined entirely outside the core.
struct UppercaseNode;

#[async_trait]
impl NodeExecutor for UppercaseNode {
    fn kind(&self) -> &str {
        "uppercase"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            streaming: true,
            ..Default::default()
        }
    }

    async fn execute(
        &self,
        _ctx: &ExecutionContext,
        input: NodeInput,
    ) -> Result<NodeOutput, NodeError> {
        let text = match input.value.as_text() {
            Some(t) => t.to_uppercase(),
            None => {
                return Err(NodeError::InputMismatch {
                    expected: "string input".into(),
                    got: format!("{:?}", input.value),
                });
            }
        };
        Ok(NodeOutput::message(RuntimeValue::String(text)))
    }
}

// ─── Extension proof: node ─────────────────────────────────────────────────

#[tokio::test]
async fn foreign_node_executes_via_engine_without_core_changes() {
    let mut registry = NodeRegistry::new();
    registry.register(UppercaseNode);

    // Build a workflow with an Input → Custom → Output shape. The only
    // extension point is the NodeRegistry passed to compile_with_registry;
    // the engine's built-in match arms were NOT modified to know "uppercase".
    let wf = custom_node_workflow();
    let plan = match ExecutionPlan::compile_with_registry(&wf, Arc::new(registry)) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };

    let lanes = Arc::new(LaneRegistry::new());
    let ctx = ExecutionContext::new("wf".into(), "run".into(), lanes);
    let runtime = workflow_runtime::NodeRuntime::new(plan);

    let output = match runtime
        .execute(
            &ctx,
            NodeInput::message(RuntimeValue::String("hello".into())),
        )
        .await
    {
        Ok(o) => o,
        Err(e) => panic!("engine execution failed: {e}"),
    };
    assert_eq!(output.value, RuntimeValue::String("HELLO".into()));
}

/// A workflow whose middle node is `Custom { kind: "uppercase" }`.
fn custom_node_workflow() -> workflow_schema::Workflow {
    use workflow_schema::*;
    Workflow {
        id: "custom-wf".into(),
        name: "custom".into(),
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
                id: "up".into(),
                kind: NodeKind::Custom,
                config: NodeConfig::Custom(CustomConfig {
                    kind: "uppercase".into(),
                    payload: serde_json::Value::Null,
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
                target_node: "up".into(),
                target_port: "in".into(),
                condition: None,
            },
            Edge {
                source_node: "up".into(),
                source_port: "out".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            },
        ],
    }
}

// ─── Extension proof: provider ─────────────────────────────────────────────

/// A foreign provider record. Declared with its capabilities, protocol,
/// endpoint, model, and lane. Adding it never touches the core.
fn register_foreign_provider() -> workflow_runtime::ProviderRegistry {
    let base_url = match Url::parse("https://ai.example.com") {
        Ok(u) => u,
        Err(e) => panic!("bad test url: {e}"),
    };
    let mut registry = workflow_runtime::ProviderRegistry::new();
    registry.register(workflow_runtime::ProviderEntry {
        id: "example-llm".into(),
        protocol: protocol_core::canonical::Protocol::OpenAiChatCompletions,
        base_url,
        model: "example-1".into(),
        capabilities: Capabilities {
            streaming: true,
            tools: true,
            structured_output: true,
            ..Default::default()
        },
        lane_id: "example-lane".into(),
    });
    registry
}

#[test]
fn foreign_provider_registers_and_resolves() {
    let registry = register_foreign_provider();
    let provider = match registry.get("example-llm") {
        Some(p) => p,
        None => panic!("provider missing"),
    };
    assert_eq!(provider.id, "example-llm");
    assert_eq!(provider.model, "example-1");
    assert!(provider.capabilities.streaming);
    assert!(provider.capabilities.structured_output);
}

// ─── Foreign registry feeds the LaneRegistry used by the compiler ──────────

#[test]
fn foreign_lane_resolves_for_compiler() {
    let base_url = match Url::parse("http://127.0.0.1:9000") {
        Ok(u) => u,
        Err(e) => panic!("bad test url: {e}"),
    };
    let mut lanes = LaneRegistry::new();
    lanes.register(LaneEntry {
        id: "example-lane".into(),
        base_url,
        authorization: None,
    });
    let lanes = Arc::new(lanes);

    // The compiler's lane validation accepts the foreign lane.
    let wf = simple_llm_workflow();
    let ctx = workflow_runtime::CompileContext { lanes };
    let result = workflow_runtime::compile_workflow(&wf, &ctx);
    assert!(result.is_ok());
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn simple_llm_workflow() -> workflow_schema::Workflow {
    use workflow_schema::*;
    Workflow {
        id: "foreign-wf".into(),
        name: "foreign".into(),
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
                    model: Some("example-1".into()),
                    temperature: None,
                    max_tokens: None,
                    stream: false,
                    lane_id: Some("example-lane".into()),
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
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            },
        ],
    }
}
