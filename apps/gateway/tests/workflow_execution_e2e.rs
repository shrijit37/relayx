//! End-to-end compiled-workflow execution through the real gateway.
//!
//! Proves the runtime architecture with live routes:
//! - Input → LLM → Output (fast path) against a real mock upstream
//! - Input → LLM → Output via the interpreter (WorkflowExecution)
//! - fallback: primary provider fails → backup provider serves the request
//! - streaming: the LLM node requests + decodes an SSE streamed response
//!
//! In every case the compiled plan comes from the published `RuntimeSnapshot`
//! (per-lane pool included), and the request enters through the real gateway
//! HTTP endpoint.

use std::sync::Arc;
use std::time::Duration;

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use relay_gateway::lanes::HyperPoolBuilder;
use relay_gateway::observability::{PublicationState, WireSnapshot, WireWorkflow};
use relay_gateway::server::GatewayServer;
use test_harness::post_hyper;
use workflow_runtime::InMemoryPublisher;
use workflow_schema::*;

fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("local addr")
        .port()
}

fn input_node() -> Node {
    Node {
        id: "in".into(),
        kind: NodeKind::Input,
        config: NodeConfig::Input(InputConfig::default()),
        inputs: vec![],
        outputs: vec![PortDef {
            name: "out".into(),
            port_type: PortType::Message,
        }],
    }
}

fn output_node() -> Node {
    Node {
        id: "out".into(),
        kind: NodeKind::Output,
        config: NodeConfig::Output(OutputConfig::default()),
        inputs: vec![PortDef {
            name: "in".into(),
            port_type: PortType::Message,
        }],
        outputs: vec![],
    }
}

fn llm_node(lane_id: &str, stream: bool, model: &str) -> Node {
    Node {
        id: "llm".into(),
        kind: NodeKind::Llm,
        config: NodeConfig::Llm(LlmConfig {
            protocol: None,
            model: Some(model.into()),
            temperature: None,
            max_tokens: None,
            stream,
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

fn edge(source: &str, target: &str) -> Edge {
    Edge {
        source_node: source.into(),
        source_port: "out".into(),
        target_node: target.into(),
        target_port: "in".into(),
        condition: None,
    }
}

/// A simple Input → LLM → Output workflow (classifies as fast path).
fn llm_workflow(model: &str, lane_id: &str, stream: bool) -> Workflow {
    Workflow {
        id: "llm-wf".into(),
        name: "llm".into(),
        version: 1,
        nodes: vec![
            input_node(),
            llm_node(lane_id, stream, model),
            output_node(),
        ],
        edges: vec![edge("in", "llm"), edge("llm", "out")],
    }
}

/// A workflow with a Fallback node between two providers.
fn fallback_workflow(primary: &str, backup: &str) -> Workflow {
    Workflow {
        id: "fallback-wf".into(),
        name: "fallback".into(),
        version: 1,
        nodes: vec![
            input_node(),
            Node {
                id: "fb".into(),
                kind: NodeKind::Fallback,
                config: NodeConfig::Fallback(FallbackConfig {
                    providers: vec![
                        FallbackProvider {
                            lane_id: primary.into(),
                            model: "primary-model".into(),
                            protocol: None,
                        },
                        FallbackProvider {
                            lane_id: backup.into(),
                            model: "backup-model".into(),
                            protocol: None,
                        },
                    ],
                    rounds: 1,
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
            output_node(),
        ],
        edges: vec![edge("in", "fb"), edge("fb", "out")],
    }
}

/// Build + publish a `RuntimeSnapshot` from the workflow with per-lane pools,
/// then return a gateway sharing that publication state.
async fn spawn_workflow_gateway(
    workflow_id: &str,
    workflow: Workflow,
    lanes: &[(String, String)],
) -> (u16, Arc<InMemoryPublisher>, Arc<PublicationState>, String) {
    let proxy_port = free_port();
    let admin_port = free_port();
    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:{proxy_port}"
admin_listen = "127.0.0.1:{admin_port}"
total_timeout_ms = 15000
graceful_shutdown_ms = 500

[[routes]]
id = "workflow"
path_prefix = "/v1/workflow"
methods = ["POST"]
workflow_id = "{workflow_id}"
"#
    ))
    .expect("valid workflow config");

    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(Duration::from_secs(90), 16)),
    ));

    // Publish the workflow + its lanes.
    let wire = WireSnapshot {
        snapshot_version: 1,
        workflows: vec![WireWorkflow {
            id: workflow_id.into(),
            workflow,
            version: 1,
            lanes: lanes
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        relay_gateway::observability::WireLane {
                            base_url: v.clone(),
                            authorization: None,
                        },
                    )
                })
                .collect(),
        }],
    };
    publication
        .publish_workflows(wire)
        .map_err(|e| panic!("publish failed: {e}"))
        .expect("publish");

    let server = match GatewayServer::with_publication(config, Some(publication.clone())) {
        Ok(s) => s,
        Err(e) => panic!("server build failed: {e}"),
    };
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Wait for readiness.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let up = tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
            .await
            .is_ok();
        if up {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("gateway did not become ready");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    (
        proxy_port,
        publisher,
        publication.clone(),
        format!("http://127.0.0.1:{proxy_port}/v1/workflow"),
    )
}

#[tokio::test]
async fn fast_path_llm_workflow_runs_e2e() {
    // A json-mode mock returns a fixed chat-completions-style body.
    let body = r#"{
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 0,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hello from mock"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    }"#;
    let mock = match spawn_mock(MockConfig {
        mode: MockMode::Json,
        json_body: body.into(),
        ..Default::default()
    })
    .await
    {
        Ok(m) => m,
        Err(e) => panic!("mock spawn failed: {e}"),
    };

    let lane_url = format!("http://{}", mock.addr);
    let (proxy_port, _publisher, _publication, url) = spawn_workflow_gateway(
        "llm-wf",
        llm_workflow("test-model", "mock", false),
        &[("mock".into(), lane_url.clone())],
    )
    .await;
    let _ = proxy_port;

    // The workflow route executes the compiled plan → LLM node → mock upstream.
    let (status, body) = match post_hyper(
        &url,
        r#"{"messages":[{"role":"user","content":"hi"}]}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("workflow post failed: {e}"),
    };
    assert_eq!(
        status,
        http::StatusCode::OK,
        "workflow route should succeed"
    );

    if mock
        .state
        .requests_served
        .load(std::sync::atomic::Ordering::Relaxed)
        == 0
    {
        panic!("mock upstream was never called — the LLM node did not reach the provider");
    }

    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => panic!("response not JSON: {e}"),
    };
    // The canonical response serializes content blocks; assert the mock's
    // reply text is present somewhere in the response.
    let rendered = serde_json::to_string(&json).unwrap_or_default();
    assert!(
        rendered.contains("hello from mock"),
        "expected the provider text in the workflow response, got {rendered}"
    );
}

#[tokio::test]
async fn fallback_workflow_fails_over_to_backup_provider() {
    let body = r#"{
        "id": "chatcmpl-backup",
        "object": "chat.completion",
        "created": 0,
        "model": "backup-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "served by backup"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    }"#;
    let mock = match spawn_mock(MockConfig {
        mode: MockMode::Json,
        json_body: body.into(),
        ..Default::default()
    })
    .await
    {
        Ok(m) => m,
        Err(e) => panic!("mock spawn failed: {e}"),
    };

    // Use a never-listening port as the dead primary.
    let dead_port = free_port();
    let dead_lane = format!("http://127.0.0.1:{dead_port}");

    let (proxy_port, _p, _pub, url) = spawn_workflow_gateway(
        "fallback-wf",
        fallback_workflow("dead", "backup"),
        &[
            ("dead".into(), dead_lane),
            ("backup".into(), format!("http://{}", mock.addr)),
        ],
    )
    .await;
    let _ = proxy_port;

    let (status, body) = match post_hyper(
        &url,
        r#"{"messages":[{"role":"user","content":"hi"}]}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("fallback post failed: {e}"),
    };
    assert_eq!(
        status,
        http::StatusCode::OK,
        "fallback route should succeed"
    );

    assert_eq!(
        mock.state
            .requests_served
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the backup provider should serve exactly one request"
    );

    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => panic!("fallback response not JSON: {e}"),
    };
    let rendered = serde_json::to_string(&json).unwrap_or_default();
    assert!(
        rendered.contains("served by backup"),
        "expected backup text, got {rendered}"
    );
}

#[tokio::test]
async fn streaming_llm_workflow_runs_e2e() {
    // SSE-mode mock streams OpenAI-style chunks.
    let mock = match spawn_mock(MockConfig {
        mode: MockMode::Sse,
        chunks: 3,
        chunk_size: 8,
        raw_sse: Some(vec![
            r#"{"id":"chunk-1","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}]}"#.into(),
            r#"{"id":"chunk-1","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}]}"#.into(),
            r#"{"id":"chunk-1","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#.into(),
        ]),
        ..Default::default()
    })
    .await
    {
        Ok(m) => m,
        Err(e) => panic!("mock spawn failed: {e}"),
    };

    let (proxy_port, _p, _pub, url) = spawn_workflow_gateway(
        "llm-wf",
        llm_workflow("test-model", "mock", true),
        &[("mock".into(), format!("http://{}", mock.addr))],
    )
    .await;
    let _ = proxy_port;

    let (status, body) = match post_hyper(
        &url,
        r#"{"messages":[{"role":"user","content":"hi"}]}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("workflow post failed: {e}"),
    };
    assert_eq!(
        status,
        http::StatusCode::OK,
        "streaming workflow route should succeed"
    );

    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => panic!("streaming response not JSON: {e}"),
    };
    let rendered = serde_json::to_string(&json).unwrap_or_default();
    assert!(
        rendered.contains("Hello"),
        "expected streamed text 'Hello' in the response, got {rendered}"
    );
}
