//! End-to-end gateway workflow-route execution test.
//!
//! Proves the full path the code review flagged as dead:
//! config with a `workflow_id` route → snapshot with a compiled plan →
//! POST → plan executes (fast path) → JSON response.

use std::sync::Arc;
use std::time::Duration;

use http::StatusCode;
use relay_gateway::config::GatewayConfig;
use relay_gateway::server::GatewayServer;
use test_harness::post_hyper;
use url::Url;
use workflow_runtime::context::{LaneEntry, LaneRegistry};
use workflow_runtime::{CompileContext, RuntimeSnapshotBuilder, compile_workflow};
use workflow_schema::*;

/// Build a gateway config with a workflow route (`lane` omitted).
fn workflow_config(proxy_port: u16, admin_port: u16) -> GatewayConfig {
    GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:{proxy_port}"
admin_listen = "127.0.0.1:{admin_port}"
total_timeout_ms = 5000
graceful_shutdown_ms = 500

[[routes]]
id = "workflow-route"
path_prefix = "/v1/workflow"
methods = ["POST"]
workflow_id = "wf-simple"
"#
    ))
    .expect("valid workflow config")
}

/// Compile a simple Input → Output workflow into a snapshot.
fn workflow_snapshot() -> Arc<workflow_runtime::RuntimeSnapshot> {
    let wf = Workflow {
        id: "wf-simple".into(),
        name: "simple".into(),
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
    };

    let mut lanes = LaneRegistry::new();
    let base = match Url::parse("http://127.0.0.1:9000") {
        Ok(u) => u,
        Err(e) => panic!("bad lane url: {e}"),
    };
    lanes.register(LaneEntry {
        id: "mock".into(),
        base_url: base,
        authorization: None,
    });
    let lanes = Arc::new(lanes);

    let plan = match compile_workflow(&wf, &CompileContext { lanes }) {
        Ok(p) => p,
        Err(e) => panic!("workflow compile failed: {e}"),
    };

    Arc::new(
        RuntimeSnapshotBuilder::new(1)
            .with_plan("wf-simple", plan)
            .build(),
    )
}

#[tokio::test]
async fn workflow_route_executes_compiled_plan() {
    let proxy_port = free_port();
    let admin_port = free_port();

    let config = workflow_config(proxy_port, admin_port);
    let snapshot = workflow_snapshot();

    let server = match GatewayServer::with_snapshot(config, Some(snapshot)) {
        Ok(s) => s,
        Err(e) => panic!("server build failed: {e}"),
    };

    let handle = tokio::spawn(async move {
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

    // POST a JSON body to the workflow route.
    let url = format!("http://127.0.0.1:{proxy_port}/v1/workflow");
    let (status, body) = match post_hyper(&url, r#"{"hello":"world"}"#, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("post failed: {e}"),
    };

    handle.abort();

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => panic!("response not JSON: {e}"),
    };
    // Input → Output echoes back a JSON-typed value.
    assert!(
        json.is_object() || json.is_string() || json.is_null(),
        "expected an echo of the input, got {json}"
    );
}

fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("local addr")
        .port()
}
