//! Integration tests for the workflow-runtime execution engine.

use std::sync::Arc;

use workflow_runtime::WorkflowError;
use workflow_runtime::context::{ExecutionContext, LaneEntry, LaneRegistry};
use workflow_runtime::execution::{ExecutionPlan, NodeRuntime};
use workflow_runtime::nodes::{NodeInput, RuntimeValue};
use workflow_schema::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

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

fn transform_node(id: &str, op: TransformOperation) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Transform,
        config: NodeConfig::Transform(TransformConfig { operation: op }),
        inputs: vec![PortDef {
            name: "in".into(),
            port_type: PortType::Json,
        }],
        outputs: vec![PortDef {
            name: "out".into(),
            port_type: PortType::Json,
        }],
    }
}

fn condition_node(id: &str, field: &str, op: ConditionOp, value: serde_json::Value) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Condition,
        config: NodeConfig::Condition(ConditionConfig {
            condition: format!("{field} {op:?}"),
            field: field.into(),
            operator: op,
            value,
        }),
        inputs: vec![PortDef {
            name: "in".into(),
            port_type: PortType::Json,
        }],
        outputs: vec![
            PortDef {
                name: "true".into(),
                port_type: PortType::Bool,
            },
            PortDef {
                name: "false".into(),
                port_type: PortType::Bool,
            },
        ],
    }
}

fn llm_node(id: &str, lane_id: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Llm,
        config: NodeConfig::Llm(LlmConfig {
            protocol: Some("openai_chat".into()),
            model: Some("gpt-test".into()),
            temperature: None,
            max_tokens: None,
            stream: false,
            lane_id: Some(lane_id.into()),
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

fn router_node(id: &str, strategy: RouterStrategy) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Router,
        config: NodeConfig::Router(RouterConfig {
            strategy,
            output_ports: 2,
        }),
        inputs: vec![PortDef {
            name: "in".into(),
            port_type: PortType::Message,
        }],
        outputs: vec![
            PortDef {
                name: "route_0".into(),
                port_type: PortType::Message,
            },
            PortDef {
                name: "route_1".into(),
                port_type: PortType::Message,
            },
        ],
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

fn conditional_edge(source: &str, target: &str, source_port: &str) -> Edge {
    Edge {
        source_node: source.into(),
        source_port: source_port.into(),
        target_node: target.into(),
        target_port: "in".into(),
        condition: None,
    }
}

fn build_and_validate(wf: &Workflow) -> ExecutionPlan {
    if let Err(e) = wf.validate() {
        panic!("workflow validation failed: {e:?}");
    }
    match ExecutionPlan::compile(wf) {
        Ok(plan) => plan,
        Err(e) => panic!("execution plan compilation failed: {e}"),
    }
}

fn run_sync(plan: ExecutionPlan, input: RuntimeValue, lanes: Arc<LaneRegistry>) -> RuntimeValue {
    let rt = NodeRuntime::new(plan);
    let ctx = ExecutionContext::new("test-wf".into(), "test-run".into(), lanes);
    match tokio_test::block_on(rt.execute(&ctx, NodeInput::message(input))) {
        Ok(output) => output.value,
        Err(e) => panic!("execution failed: {e}"),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn test_input_to_output_passthrough() {
    let wf = Workflow {
        id: "wf1".into(),
        name: "passthrough".into(),
        version: 1,
        nodes: vec![input_node("in"), output_node("out")],
        edges: vec![simple_edge("in", "out")],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let result = run_sync(plan, RuntimeValue::String("hello".into()), lanes);
    assert_eq!(result, RuntimeValue::String("hello".into()));
}

#[test]
fn test_input_transform_output_passthrough() {
    let wf = Workflow {
        id: "wf2".into(),
        name: "transform-passthrough".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            transform_node("t1", TransformOperation::Passthrough),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "t1"), simple_edge("t1", "out")],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let result = run_sync(plan, RuntimeValue::Number(42.0), lanes);
    assert_eq!(result, RuntimeValue::Number(42.0));
}

#[test]
fn test_condition_true_branch() {
    let wf = Workflow {
        id: "wf3".into(),
        name: "condition-true".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            condition_node("cond", "flag", ConditionOp::Equal, serde_json::json!(true)),
            output_node("out"),
        ],
        edges: vec![
            simple_edge("in", "cond"),
            conditional_edge("cond", "out", "true"),
        ],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let input = RuntimeValue::Json(serde_json::json!({"flag": true}));
    let result = run_sync(plan, input, lanes);
    assert_eq!(result, RuntimeValue::Bool(true));
}

#[test]
fn test_condition_false_branch_skipped() {
    let wf = Workflow {
        id: "wf4".into(),
        name: "condition-false".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            condition_node("cond", "flag", ConditionOp::Equal, serde_json::json!(true)),
            output_node("out"),
        ],
        edges: vec![
            simple_edge("in", "cond"),
            conditional_edge("cond", "out", "true"), // only true path
        ],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let input = RuntimeValue::Json(serde_json::json!({"flag": false}));
    let rt = NodeRuntime::new(plan);
    let ctx = ExecutionContext::new("wf4".into(), "test-run".into(), lanes);
    let result = tokio_test::block_on(rt.execute(&ctx, NodeInput::message(input)));
    assert!(
        matches!(result, Err(WorkflowError::Validation(_))),
        "Expected validation error (no output), got: {result:?}"
    );
}

#[test]
fn test_router_first_match() {
    let wf = Workflow {
        id: "wf5".into(),
        name: "router".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            router_node("r1", RouterStrategy::FirstMatch),
            output_node("out"),
        ],
        edges: vec![
            simple_edge("in", "r1"),
            Edge {
                source_node: "r1".into(),
                source_port: "route_0".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            },
        ],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let result = run_sync(plan, RuntimeValue::String("test".into()), lanes);
    assert_eq!(result, RuntimeValue::String("test".into()));
}

#[test]
fn test_llm_node_with_mock_upstream() {
    let result = tokio_test::block_on(async {
        let mock_cfg = mock_upstream::MockConfig {
            mode: mock_upstream::MockMode::Json,
            json_body: r#"{"id":"chatcmpl-test","object":"chat.completion","created":1234567890,"model":"gpt-test","choices":[{"index":0,"message":{"role":"assistant","content":"Hello from mock!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#.into(),
            ..mock_upstream::MockConfig::default()
        };
        let mock = match mock_upstream::spawn_mock(mock_cfg).await {
            Ok(m) => m,
            Err(e) => panic!("failed to spawn mock: {e}"),
        };
        let base_url: url::Url = match format!("http://{}", mock.addr).parse() {
            Ok(m) => m,
            Err(e) => panic!("failed to spawn mock: {e}"),
        };
        let mut lanes = LaneRegistry::new();
        lanes.register(LaneEntry {
            id: "test-lane".into(),
            base_url,
            authorization: None,
        });
        let lanes = Arc::new(lanes);

        let wf = Workflow {
            id: "wf6".into(),
            name: "llm".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                llm_node("llm1", "test-lane"),
                output_node("out"),
            ],
            edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
        };
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        let rt_runtime = NodeRuntime::new(plan);

        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http();

        let mut ctx = ExecutionContext::new(wf.id.clone(), "test-run".into(), lanes);
        ctx.upstream_client = Some(Arc::new(client));

        let input = NodeInput::message(RuntimeValue::Json(serde_json::json!({
            "messages": [{"role": "user", "content": "Hello!"}]
        })));

        let output = rt_runtime.execute(&ctx, input).await;
        drop(mock);
        match output {
            Ok(o) => o.value,
            Err(e) => panic!("execution failed: {e}"),
        }
    });

    // Mock upstream in JSON mode returns {"ok":true,"result":"hello"}
    // The LLM node decodes it as a canonical response.
    match &result {
        RuntimeValue::Json(json) => {
            assert!(
                json.get("id").is_some() || json.get("model").is_some(),
                "Expected canonical response with id/model, got: {json}"
            );
        }
        other => panic!("Expected JSON response, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_concurrent_executions() {
    let mock = match mock_upstream::spawn_mock(mock_upstream::MockConfig::default()).await {
        Ok(m) => m,
        Err(e) => panic!("failed to spawn mock: {e}"),
    };
    let base_url: url::Url = match format!("http://{}", mock.addr).parse() {
        Ok(m) => m,
        Err(e) => panic!("failed to spawn mock: {e}"),
    };
    let mut lanes = LaneRegistry::new();
    lanes.register(LaneEntry {
        id: "lane1".into(),
        base_url,
        authorization: None,
    });
    let lanes = Arc::new(lanes);

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let mut handles = vec![];
    for i in 0..5 {
        let lanes = lanes.clone();
        let client = client.clone();
        let wf = Workflow {
            id: format!("wf-concurrent-{i}"),
            name: format!("concurrent-{i}"),
            version: 1,
            nodes: vec![input_node("in"), output_node("out")],
            edges: vec![simple_edge("in", "out")],
        };
        handles.push(tokio::spawn(async move {
            let plan = match ExecutionPlan::compile(&wf) {
                Ok(p) => p,
                Err(e) => panic!("compile failed: {e}"),
            };
            let rt = NodeRuntime::new(plan);
            let mut ctx = ExecutionContext::new(wf.id.clone(), format!("run-{i}"), lanes);
            ctx.upstream_client = Some(Arc::new(client.clone()));
            let input = NodeInput::message(RuntimeValue::String(format!("msg-{i}")));
            match rt.execute(&ctx, input).await {
                Ok(o) => o.value,
                Err(e) => panic!("execution failed: {e}"),
            }
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let result = match handle.await {
            Ok(v) => v,
            Err(e) => panic!("task {i} failed: {e}"),
        };
        assert_eq!(result, RuntimeValue::String(format!("msg-{i}")));
    }
}

#[tokio::test]
async fn test_cancellation() {
    let wf = Workflow {
        id: "wf-cancel".into(),
        name: "cancel".into(),
        version: 1,
        nodes: vec![input_node("in"), output_node("out")],
        edges: vec![simple_edge("in", "out")],
    };
    let plan = match ExecutionPlan::compile(&wf) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };
    let rt = NodeRuntime::new(plan);
    let lanes = Arc::new(LaneRegistry::new());
    let ctx = ExecutionContext::new("wf-cancel".into(), "cancel-test".into(), lanes);
    ctx.cancel_token.cancel();
    let input = NodeInput::message(RuntimeValue::Null);
    let result = rt.execute(&ctx, input).await;
    assert!(
        matches!(result, Err(WorkflowError::Cancelled)),
        "Expected Cancelled, got: {result:?}"
    );
}

#[test]
fn test_transform_extract() {
    let wf = Workflow {
        id: "wf-extract".into(),
        name: "extract".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            transform_node("t1", TransformOperation::Extract),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "t1"), simple_edge("t1", "out")],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let input = RuntimeValue::Json(serde_json::json!({"field": "name", "name": "Alice"}));
    let result = run_sync(plan, input, lanes);
    assert_eq!(result, RuntimeValue::String("Alice".into()));
}

#[test]
fn test_transform_filter() {
    let wf = Workflow {
        id: "wf-filter".into(),
        name: "filter".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            transform_node("t1", TransformOperation::Filter),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "t1"), simple_edge("t1", "out")],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let input = RuntimeValue::Json(serde_json::json!(["hello", "", "world", null, "foo"]));
    let result = run_sync(plan, input, lanes);
    let arr = match &result {
        RuntimeValue::Json(serde_json::Value::Array(a)) => a.clone(),
        other => panic!("Expected array, got: {other:?}"),
    };
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0], serde_json::json!("hello"));
    assert_eq!(arr[1], serde_json::json!("world"));
    assert_eq!(arr[2], serde_json::json!("foo"));
}

#[test]
fn test_transform_merge() {
    let wf = Workflow {
        id: "wf-merge".into(),
        name: "merge".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            transform_node("t1", TransformOperation::Merge),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "t1"), simple_edge("t1", "out")],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let input = RuntimeValue::Json(serde_json::json!([{"a": 1}, {"b": 2}]));
    let result = run_sync(plan, input, lanes);
    match &result {
        RuntimeValue::Json(serde_json::Value::Object(map)) => {
            assert_eq!(map.get("a"), Some(&serde_json::json!(1)));
            assert_eq!(map.get("b"), Some(&serde_json::json!(2)));
        }
        other => panic!("Expected merged object, got: {other:?}"),
    }
}

#[test]
fn test_router_round_robin_three_ports() {
    use std::sync::Mutex;
    use workflow_runtime::milestone::MilestoneReporter;

    // Router with 3 output ports cycles route_0 → route_1 → route_2 on ONE runtime.
    // The reporter records the port the router selected on each node_completed call.
    #[derive(Default)]
    struct Recording {
        ports: Mutex<Vec<String>>,
    }
    impl MilestoneReporter for Recording {
        fn node_completed(&self, node_id: &str, output_port: Option<&str>) {
            if node_id == "route" {
                self.ports
                    .lock()
                    .unwrap()
                    .push(output_port.unwrap_or("").to_string());
            }
        }
        fn node_failed(&self, _node_id: &str, _error: &str) {}
    }

    let wf = Workflow {
        id: "wf-router3".into(),
        name: "router3".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            Node {
                id: "route".into(),
                kind: NodeKind::Router,
                config: NodeConfig::Router(RouterConfig {
                    strategy: RouterStrategy::RoundRobin,
                    output_ports: 3,
                }),
                inputs: vec![PortDef {
                    name: "in".into(),
                    port_type: PortType::Message,
                }],
                outputs: vec![
                    PortDef {
                        name: "route_0".into(),
                        port_type: PortType::Message,
                    },
                    PortDef {
                        name: "route_1".into(),
                        port_type: PortType::Message,
                    },
                    PortDef {
                        name: "route_2".into(),
                        port_type: PortType::Message,
                    },
                ],
            },
            output_node("out0"),
            output_node("out1"),
            output_node("out2"),
        ],
        edges: vec![
            simple_edge("in", "route"),
            conditional_edge("route", "out0", "route_0"),
            conditional_edge("route", "out1", "route_1"),
            conditional_edge("route", "out2", "route_2"),
        ],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let recording = Arc::new(Recording::default());
    let rt = NodeRuntime::new(plan);
    for i in 0..6u32 {
        let mut ctx = ExecutionContext::new("test-wf".into(), format!("run-{i}"), lanes.clone());
        ctx.reporter = recording.clone();
        // NodeRuntime::execute takes &self; the counter persists across calls.
        let result = tokio_test::block_on(rt.execute(
            &ctx,
            NodeInput::message(RuntimeValue::String(format!("msg-{i}"))),
        ))
        .expect("execution succeeded");
        assert_eq!(result.port, None); // output nodes report no port
    }
    let ports = recording.ports.lock().unwrap().clone();
    assert_eq!(
        ports,
        vec![
            "route_0", "route_1", "route_2", "route_0", "route_1", "route_2"
        ]
    );
}

#[test]
fn test_router_single_port_always_route_0() {
    use std::sync::Mutex;
    use workflow_runtime::milestone::MilestoneReporter;

    #[derive(Default)]
    struct Recording {
        ports: Mutex<Vec<String>>,
    }
    impl MilestoneReporter for Recording {
        fn node_completed(&self, node_id: &str, output_port: Option<&str>) {
            if node_id == "route" {
                self.ports
                    .lock()
                    .unwrap()
                    .push(output_port.unwrap_or("").to_string());
            }
        }
        fn node_failed(&self, _node_id: &str, _error: &str) {}
    }

    let wf = Workflow {
        id: "wf-router1".into(),
        name: "router1".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            Node {
                id: "route".into(),
                kind: NodeKind::Router,
                config: NodeConfig::Router(RouterConfig {
                    strategy: RouterStrategy::RoundRobin,
                    output_ports: 1,
                }),
                inputs: vec![PortDef {
                    name: "in".into(),
                    port_type: PortType::Message,
                }],
                outputs: vec![PortDef {
                    name: "route_0".into(),
                    port_type: PortType::Message,
                }],
            },
            output_node("out0"),
        ],
        edges: vec![
            simple_edge("in", "route"),
            conditional_edge("route", "out0", "route_0"),
        ],
    };
    let plan = build_and_validate(&wf);
    let lanes = Arc::new(LaneRegistry::new());
    let recording = Arc::new(Recording::default());
    let rt = NodeRuntime::new(plan);
    for i in 0..4u32 {
        let mut ctx = ExecutionContext::new("test-wf".into(), format!("run-{i}"), lanes.clone());
        ctx.reporter = recording.clone();
        let result = tokio_test::block_on(
            rt.execute(&ctx, NodeInput::message(RuntimeValue::String("x".into()))),
        )
        .expect("execution succeeded");
        assert_eq!(result.port, None);
    }
    let ports = recording.ports.lock().unwrap().clone();
    assert_eq!(ports, vec!["route_0", "route_0", "route_0", "route_0"]);
}

// ─── RuntimeValue integer precision ─────────────────────────────────────────

mod runtime_value_integer {
    use super::*;

    #[test]
    fn test_integer_from_json_preserves_precision() {
        let v = RuntimeValue::from_json(serde_json::json!(i64::MAX));
        assert!(matches!(v, RuntimeValue::Integer(i64::MAX)));
        assert_eq!(v.to_json(), serde_json::json!(i64::MAX));
    }

    #[test]
    fn test_integer_condition_equality() {
        // Integer(42) == json!(42) → true branch
        let wf = Workflow {
            id: "wf-int-eq".into(),
            name: "int-eq".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                condition_node("cond", "n", ConditionOp::Equal, serde_json::json!(42)),
                output_node("out"),
            ],
            edges: vec![
                simple_edge("in", "cond"),
                conditional_edge("cond", "out", "true"),
            ],
        };
        let lanes = Arc::new(LaneRegistry::new());
        let result = run_sync(
            build_and_validate(&wf),
            RuntimeValue::Json(serde_json::json!({"n": 42})),
            lanes,
        );
        assert_eq!(result, RuntimeValue::Bool(true));

        // Integer(42) == json!(42.0) → false; no "true" edge → validation error
        let wf = Workflow {
            id: "wf-int-eq-float".into(),
            name: "int-eq-float".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                condition_node("cond", "n", ConditionOp::Equal, serde_json::json!(42.0)),
                output_node("out"),
            ],
            edges: vec![
                simple_edge("in", "cond"),
                conditional_edge("cond", "out", "true"),
            ],
        };
        let plan = build_and_validate(&wf);
        let lanes = Arc::new(LaneRegistry::new());
        let rt = NodeRuntime::new(plan);
        let ctx = ExecutionContext::new("wf-int-eq-float".into(), "test-run".into(), lanes);
        let result = tokio_test::block_on(rt.execute(
            &ctx,
            NodeInput::message(RuntimeValue::Json(serde_json::json!({"n": 42}))),
        ));
        assert!(
            matches!(result, Err(WorkflowError::Validation(_))),
            "Expected validation error (condition false), got: {result:?}"
        );
    }

    #[test]
    fn test_integer_condition_comparison() {
        // Integer(10) > json!(5)
        let wf = Workflow {
            id: "wf-int-gt".into(),
            name: "int-gt".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                condition_node("cond", "n", ConditionOp::GreaterThan, serde_json::json!(5)),
                output_node("out"),
            ],
            edges: vec![
                simple_edge("in", "cond"),
                conditional_edge("cond", "out", "true"),
            ],
        };
        let lanes = Arc::new(LaneRegistry::new());
        let result = run_sync(
            build_and_validate(&wf),
            RuntimeValue::Json(serde_json::json!({"n": 10})),
            lanes,
        );
        assert_eq!(result, RuntimeValue::Bool(true));

        // Integer(10) < json!(20)
        let wf = Workflow {
            id: "wf-int-lt".into(),
            name: "int-lt".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                condition_node("cond", "n", ConditionOp::LessThan, serde_json::json!(20)),
                output_node("out"),
            ],
            edges: vec![
                simple_edge("in", "cond"),
                conditional_edge("cond", "out", "true"),
            ],
        };
        let lanes = Arc::new(LaneRegistry::new());
        let result = run_sync(
            build_and_validate(&wf),
            RuntimeValue::Json(serde_json::json!({"n": 10})),
            lanes,
        );
        assert_eq!(result, RuntimeValue::Bool(true));
    }

    #[test]
    fn test_integer_large_values() {
        let two_53: i64 = 1 << 53;
        for n in [two_53, two_53 + 1] {
            let v = RuntimeValue::from_json(serde_json::json!(n));
            assert!(
                matches!(v, RuntimeValue::Integer(_)),
                "expected Integer for {n}, got {v:?}"
            );
            assert_eq!(v.to_json(), serde_json::json!(n));
        }
    }

    #[test]
    fn test_mcp_no_executor_returns_error() {
        let wf = Workflow {
            id: "wf-mcp-err".into(),
            name: "mcp-err".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                Node {
                    id: "mcp1".into(),
                    kind: NodeKind::Mcp,
                    config: NodeConfig::Mcp(McpConfig {
                        server_ref: "test-server".into(),
                        tool_name: "test-tool".into(),
                        deferred: false,
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
                output_node("out"),
            ],
            edges: vec![simple_edge("in", "mcp1"), simple_edge("mcp1", "out")],
        };
        let plan = build_and_validate(&wf);
        let lanes = Arc::new(LaneRegistry::new());
        let rt = NodeRuntime::new(plan);
        let ctx = ExecutionContext::new("test-wf".into(), "run-0".into(), lanes);
        let result = tokio_test::block_on(rt.execute(&ctx, NodeInput::message(RuntimeValue::Null)));
        assert!(
            result.is_err(),
            "MCP with no executor should return error, not fabricated success"
        );
    }

    #[test]
    fn test_skill_no_loader_returns_error() {
        let wf = Workflow {
            id: "wf-skill-err".into(),
            name: "skill-err".into(),
            version: 1,
            nodes: vec![
                input_node("in"),
                Node {
                    id: "skill1".into(),
                    kind: NodeKind::Skill,
                    config: NodeConfig::Skill(SkillConfig {
                        skill_ref: "test-skill".into(),
                        progressive: false,
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
                output_node("out"),
            ],
            edges: vec![simple_edge("in", "skill1"), simple_edge("skill1", "out")],
        };
        let plan = build_and_validate(&wf);
        let lanes = Arc::new(LaneRegistry::new());
        let rt = NodeRuntime::new(plan);
        let ctx = ExecutionContext::new("test-wf".into(), "run-0".into(), lanes);
        let result = tokio_test::block_on(rt.execute(&ctx, NodeInput::message(RuntimeValue::Null)));
        assert!(
            result.is_err(),
            "Skill with no loader should return error, not fabricated success"
        );
    }
}
