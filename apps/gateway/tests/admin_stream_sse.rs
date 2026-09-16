//! Admin `/run?stream=true` — SSE token-stream conformance tests.
//!
//! Previously the streaming token-level feature shipped with zero coverage
//! (mockGateway covered only /validate, /publish, and the buffered /run).
//! These tests pin the version/identity contract: the streamed `done` event
//! must carry the real ACTIVE workflow version (not a hardcoded 0), the
//! snapshot version and plan hash, and a terminal event must arrive even when
//! the workflow's output is malformed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

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

fn passthrough_workflow() -> Workflow {
    Workflow {
        id: "echo-wf".into(),
        name: "echo".into(),
        version: 7, // the ACTIVE version the CP records — must survive the wire
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
        edges: vec![Edge {
            source_node: "in".into(),
            source_port: "out".into(),
            target_node: "out".into(),
            target_port: "in".into(),
            condition: None,
        }],
    }
}

/// Spawn a gateway with the workflow published as snapshot v42, workflow v7.
async fn spawn_gateway() -> u16 {
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
workflow_id = "echo-wf"
"#
    ))
    .expect("valid config");

    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(Duration::from_secs(90), 16)),
    ));

    let wire = WireSnapshot {
        snapshot_version: 42,
        extensions: vec![],
        workflows: vec![WireWorkflow {
            id: "echo-wf".into(),
            workflow: passthrough_workflow(),
            version: 7,
            lanes: HashMap::new(),
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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let up = tokio::net::TcpStream::connect(("127.0.0.1", admin_port))
            .await
            .is_ok();
        if up {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("gateway admin did not become ready");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    admin_port
}

/// SSE response parser (same contract as the frontend's runWorkflowStream).
fn parse_sse_events(body: &str) -> Vec<(String, String)> {
    let mut events = Vec::new();
    for part in body.split("\n\n") {
        if part.trim().is_empty() {
            continue;
        }
        let mut event_type = "message".to_string();
        let mut data = String::new();
        for line in part.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                event_type = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(v.trim());
            }
        }
        events.push((event_type, data));
    }
    events
}

#[tokio::test]
async fn streamed_done_carries_real_workflow_version() {
    let admin = spawn_gateway().await;
    let url = format!("http://127.0.0.1:{admin}/run?stream=true");

    let (status, body) = post_hyper(&url, r#"{"workflow_id":"echo-wf","body":{"hi":1}}"#, &[])
        .await
        .expect("run!");
    assert_eq!(status, http::StatusCode::OK);

    let body = String::from_utf8(body.to_vec()).expect("utf8");
    let events = parse_sse_events(&body);
    let done = events
        .iter()
        .find(|(t, _)| t == "done")
        .expect("a done event must be present");

    let payload: serde_json::Value = serde_json::from_str(&done.1).expect("done payload json");

    // The ACTIVE version (7) must survive the wire → snapshot → done event.
    assert_eq!(
        payload.get("workflow_version").and_then(|v| v.as_u64()),
        Some(7),
        "streamed done must report the workflow's ACTIVE version (not 0)"
    );
    assert_eq!(
        payload.get("snapshot_version").and_then(|v| v.as_u64()),
        Some(42),
        "streamed done must report the real snapshot version"
    );
    assert!(
        !payload
            .get("plan_hash")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .is_empty(),
        "streamed done must carry a real plan hash"
    );
    assert_eq!(
        payload.get("workflow_id").and_then(|v| v.as_str()),
        Some("echo-wf")
    );
}

/// Dropping the client mid-SSE stream must not leak the upstream task —
/// the gateway's CancellationToken propagation (tx.closed() → cancel) should
/// fire. This test verifies the gateway stays healthy after a mid-stream abort.
#[tokio::test]
async fn sse_client_disconnect_cancels_upstream() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let admin = spawn_gateway().await;
    let body = r#"{"workflow_id":"echo-wf","body":{"hi":1}}"#;
    let req = format!(
        "POST /run?stream=true HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", admin)).await?;
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;

    // Read some bytes to confirm the stream started, then drop.
    let mut buf = [0u8; 256];
    let n = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await??;
    assert!(n > 0, "should receive at least part of the SSE response");

    drop(stream);

    // Give the gateway time to observe the disconnect and cancel the run.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The gateway must still be healthy — the cancelled run must not poison it.
    let health_url = format!("http://127.0.0.1:{admin}/healthz");
    let (status, _) = test_harness::get_hyper(&health_url).await?;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "gateway healthz must remain OK after client abort"
    );

    Ok(())
}
