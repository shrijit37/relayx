//! Protocol propagation test: LLM node uses the selected protocol, not hardcoded OpenAI.

use std::collections::HashMap;
use std::sync::Arc;

use workflow_runtime::context::{ExecutionContext, LaneClient, LaneEntry, LaneRegistry};
use workflow_runtime::execution::{ExecutionPlan, NodeRuntime};
use workflow_runtime::nodes::{NodeInput, RuntimeValue};
use workflow_schema::*;

/// Per-lane pool resolver: the runtime resolves clients ONLY from per-lane
/// pools (no shared direct fallback — fail-closed egress).
#[derive(Clone)]
struct Pools(HashMap<String, Arc<LaneClient>>);

impl workflow_runtime::AsLaneClient for Pools {
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<LaneClient>> {
        self.0.get(lane_id).cloned()
    }
}

fn pools_for(lanes: &[(&str, url::Url)]) -> Arc<Pools> {
    let mut map = HashMap::new();
    for (id, _base) in lanes {
        map.insert(
            id.to_string(),
            Arc::new(LaneClient::direct(std::time::Duration::from_secs(30), 16)),
        );
    }
    Arc::new(Pools(map))
}

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

fn llm_node(id: &str, lane_id: &str, protocol: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Llm,
        config: NodeConfig::Llm(LlmConfig {
            protocol: Some(protocol.into()),
            model: Some("claude-3-mock".into()),
            temperature: None,
            max_tokens: Some(512),
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

fn simple_edge(source: &str, target: &str) -> Edge {
    Edge {
        source_node: source.into(),
        source_port: "out".into(),
        target_node: target.into(),
        target_port: "in".into(),
        condition: None,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// Verifies the LLM node routes to `/v1/messages` (not `/v1/chat/completions`)
/// when the protocol is `anthropic_messages`, and the body is Anthropic format
/// (has top-level `max_tokens` with an integer value, not OpenAI's string-based
/// messages array).
#[tokio::test]
async fn anthropic_protocol_routes_to_messages_endpoint() {
    let mock_cfg = mock_upstream::MockConfig {
        mode: mock_upstream::MockMode::Json,
        json_body: r#"{
            "id": "msg_mock_456",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-mock",
            "content": [{"type": "text", "text": "Hello from Anthropic mock!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#
        .into(),
        ..mock_upstream::MockConfig::default()
    };
    let mock = mock_upstream::spawn_mock(mock_cfg)
        .await
        .expect("failed to spawn mock");
    let base_url: url::Url = format!("http://{}", mock.addr)
        .parse()
        .expect("invalid mock base URL");

    let mut lanes = LaneRegistry::new();
    lanes.register(LaneEntry {
        id: "anthropic-lane".into(),
        base_url: base_url.clone(),
        authorization: None,
        egress: "direct".into(),
        proxy_url: None,
    });
    let lanes = Arc::new(lanes);

    let wf = Workflow {
        id: "wf-anthropic".into(),
        name: "anthropic-protocol-test".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            llm_node("llm1", "anthropic-lane", "anthropic_messages"),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
    };
    let plan = ExecutionPlan::compile(&wf).expect("failed to compile plan");
    let rt = NodeRuntime::new(plan);

    let mut ctx = ExecutionContext::new(wf.id.clone(), "test-run".into(), lanes);
    ctx.lane_clients = Some(pools_for(&[("anthropic-lane", base_url.clone())]));

    let input = NodeInput::message(RuntimeValue::Json(serde_json::json!({
        "messages": [{"role": "user", "content": "Hello!"}]
    })));

    let output = rt.execute(&ctx, input).await.expect("execution failed");

    // Verify the response decodes correctly (mock returned Anthropic format).
    match &output.value {
        RuntimeValue::Json(json) => {
            assert!(
                json.get("id").is_some(),
                "expected canonical response with id, got: {json}"
            );
        }
        other => panic!("expected JSON response, got: {other:?}"),
    }

    // Check the captured request — the mock's /v1/messages handler stored it.
    let request_path = mock
        .state
        .last_request_path
        .lock()
        .expect("state lock poisoned")
        .clone()
        .expect("no request path captured");
    assert_eq!(
        request_path, "/v1/messages",
        "Anthropic protocol must route to /v1/messages, got {request_path}"
    );

    let captured = mock
        .state
        .last_request_body
        .lock()
        .expect("state lock poisoned")
        .clone()
        .expect("no request body captured — wrong endpoint was hit");

    let body: serde_json::Value =
        serde_json::from_str(&captured).expect("captured body is not valid JSON");

    // Anthropic format has top-level `max_tokens` as a number.
    assert!(
        body.get("max_tokens").is_some(),
        "expected Anthropic body to have `max_tokens` field, got: {body}"
    );
    assert!(
        body["max_tokens"].is_number(),
        "max_tokens should be a number in Anthropic format, got: {}",
        body["max_tokens"]
    );

    // Anthropic format does NOT have `messages[0].content` as a bare string
    // array like OpenAI's role strings; verify messages exist.
    let messages = body.get("messages").expect("missing messages field");
    assert!(messages.is_array(), "messages should be an array");
    assert!(
        !messages.as_array().unwrap().is_empty(),
        "messages should not be empty"
    );
}

/// Verifies multimodal content blocks (images) are preserved through
/// extract_messages, not collapsed to text.
#[tokio::test]
async fn multimodal_content_blocks_are_preserved() {
    let mock_cfg = mock_upstream::MockConfig {
        mode: mock_upstream::MockMode::Json,
        json_body: r#"{
            "id": "msg_mock_789",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-mock",
            "content": [{"type": "text", "text": "I see the image."}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#
        .into(),
        ..mock_upstream::MockConfig::default()
    };
    let mock = mock_upstream::spawn_mock(mock_cfg)
        .await
        .expect("failed to spawn mock");
    let base_url: url::Url = format!("http://{}", mock.addr)
        .parse()
        .expect("invalid mock base URL");

    let mut lanes = LaneRegistry::new();
    lanes.register(LaneEntry {
        id: "anthropic-lane".into(),
        base_url: base_url.clone(),
        authorization: None,
        egress: "direct".into(),
        proxy_url: None,
    });
    let lanes = Arc::new(lanes);

    let wf = Workflow {
        id: "wf-multimodal".into(),
        name: "multimodal-test".into(),
        version: 1,
        nodes: vec![
            input_node("in"),
            llm_node("llm1", "anthropic-lane", "anthropic_messages"),
            output_node("out"),
        ],
        edges: vec![simple_edge("in", "llm1"), simple_edge("llm1", "out")],
    };
    let plan = ExecutionPlan::compile(&wf).expect("failed to compile plan");
    let rt = NodeRuntime::new(plan);

    let mut ctx = ExecutionContext::new(wf.id.clone(), "test-run".into(), lanes);
    ctx.lane_clients = Some(pools_for(&[("anthropic-lane", base_url.clone())]));

    let input = NodeInput::message(RuntimeValue::Json(serde_json::json!({
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "Describe this image:"},
                {"type": "image_url", "image_url": {"url": "https://example.com/img.png", "detail": "high"}}
            ]
        }]
    })));

    let _output = rt.execute(&ctx, input).await.expect("execution failed");

    // The mock should have received the request on /v1/messages.
    let captured = mock
        .state
        .last_request_body
        .lock()
        .expect("state lock poisoned")
        .clone()
        .expect("no request body captured");

    let body: serde_json::Value =
        serde_json::from_str(&captured).expect("captured body is not valid JSON");

    let messages = body
        .get("messages")
        .expect("missing messages field")
        .as_array()
        .expect("messages is not an array");

    // The Anthropic adapter encodes content as a blocks array. Check that
    // both text and image blocks are present (not collapsed to a single string).
    let first_msg = &messages[0];
    let content = first_msg
        .get("content")
        .expect("missing content in first message");

    // In Anthropic wire format, multi-block content is serialized as an array.
    if let Some(blocks) = content.as_array() {
        let types: Vec<&str> = blocks
            .iter()
            .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
            .collect();
        assert!(
            types.contains(&"text"),
            "expected text block in content, got: {types:?}"
        );
        assert!(
            types.contains(&"image"),
            "expected image block in content, got: {types:?}"
        );
    } else {
        panic!(
            "expected content to be an array of blocks, got: {}",
            content
        );
    }
}
