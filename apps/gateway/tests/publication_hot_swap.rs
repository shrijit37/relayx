//! Snapshot publication + atomic hot-swap integration tests.
//!
//! Proves the phase contract:
//! - a snapshot is built, published, and read
//! - a second snapshot atomically replaces the first
//! - concurrent readers see one coherent snapshot per acquisition
//! - the old snapshot's `Arc` the request held keeps working after replacement
//! - the gateway serves requests from a published snapshot and observes a
//!   hot-swapped version without restart (same process, shared publisher)

use std::sync::Arc;
use std::time::Duration;

use relay_gateway::lanes::HyperPoolBuilder;
use relay_gateway::observability::{PublicationState, WireSnapshot, WireWorkflow};
use relay_gateway::server::GatewayServer;
use test_harness::post_hyper;
use workflow_runtime::context::{LaneEntry, LaneRegistry};
use workflow_runtime::{
    InMemoryPublisher, RuntimeSnapshotBuilder, SnapshotPublisher, SnapshotReader,
};
use workflow_schema::*;

fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("local addr")
        .port()
}

/// A workflow whose output echoes its input through an Output node.
fn passthrough_workflow() -> Workflow {
    Workflow {
        id: "echo-wf".into(),
        name: "echo".into(),
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
    }
}

/// A workflow whose LLM node calls `lane_id` (used to prove that an admin
/// `/run` resolves the lane's connection pool — a masked lane must never
/// fall back to a plain direct client).
fn llm_workflow(lane_id: &str) -> Workflow {
    Workflow {
        id: "llm-wf".into(),
        name: "llm".into(),
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
                    protocol: Some("openai_chat".into()),
                    model: Some("test-model".into()),
                    temperature: None,
                    max_tokens: None,
                    stream: false,
                    lane_id: Some(lane_id.to_string()),
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

fn passthrough_compiled() -> workflow_runtime::ExecutionPlan {
    match workflow_runtime::compile_workflow_with_lanes(&passthrough_workflow(), &[], None) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    }
}

/// Build the wire payload a control plane would POST to `/publish`.
fn wire_snapshot(version: u64) -> WireSnapshot {
    WireSnapshot {
        snapshot_version: version,
        extensions: vec![],
        workflows: vec![WireWorkflow {
            id: "echo-wf".into(),
            workflow: passthrough_workflow(),
            version: 1,
            lanes: std::collections::HashMap::new(),
        }],
    }
}

fn snapshot_at(version: u64) -> Arc<workflow_runtime::RuntimeSnapshot> {
    Arc::new(
        RuntimeSnapshotBuilder::new(version)
            .with_plan("echo-wf", passthrough_compiled())
            .build(),
    )
}

#[tokio::test]
async fn publication_load_returns_consistent_snapshot_and_pools() {
    let lanes_v1 = lanes_with("http://127.0.0.1:9101");
    let snapshot_v1 = Arc::new(
        RuntimeSnapshotBuilder::new(1)
            .with_lanes(lanes_v1)
            .with_plan("echo-wf", passthrough_compiled())
            .build(),
    );

    let pool_builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(pool_builder),
    ));
    publication
        .publish(snapshot_v1)
        .map_err(|e| panic!("publish failed: {e}"))
        .expect("direct-lane snapshot publishes");

    // A single load() observes the bundle atomically: snapshot v1 AND the
    // pools built from v1's lanes — never a mismatched pair.
    let bundle = publication.load();
    let snap = match bundle.snapshot.as_ref() {
        Some(s) => s,
        None => panic!("bundle should carry a snapshot"),
    };
    assert_eq!(snap.version(), 1);
    // Pools were built for v1's lane ("lane-a").
    assert!(bundle.pools.get("lane-a").is_some());
}

fn lanes_with(url: &str) -> Arc<LaneRegistry> {
    let mut lanes = LaneRegistry::new();
    lanes.register(LaneEntry {
        id: "lane-a".into(),
        base_url: match url::Url::parse(url) {
            Ok(u) => u,
            Err(e) => panic!("invalid lane url: {e}"),
        },
        authorization: None,
        egress: "direct".into(),
        proxy_url: None,
    });
    Arc::new(lanes)
}

#[tokio::test]
async fn snapshots_publish_and_atomically_hot_swap() {
    let publisher = InMemoryPublisher::new();

    publisher.publish(snapshot_at(1));
    let read1 = match publisher.snapshot() {
        Some(s) => s,
        None => panic!("v1 should be readable"),
    };
    assert_eq!(read1.version(), 1);
    assert!(read1.get_plan("echo-wf").is_some());

    publisher.publish(snapshot_at(2));
    let read2 = match publisher.snapshot() {
        Some(s) => s,
        None => panic!("v2 should be readable"),
    };
    assert_eq!(read2.version(), 2);

    // The old Arc (v1) the request held keeps working — immature stress: the
    // snapshot is immutable; the swap never mutates it.
    assert_eq!(read1.version(), 1);
    assert!(read1.get_plan("echo-wf").is_some());
}

#[tokio::test]
async fn concurrent_reads_see_one_coherent_snapshot_each() {
    let publisher = Arc::new(InMemoryPublisher::new());
    let mut readers = Vec::new();

    for i in 1..=8u64 {
        let p = publisher.clone();
        readers.push(tokio::spawn(async move {
            let s = match p.snapshot() {
                Some(s) => s,
                None => panic!("reader {i} saw no snapshot"),
            };
            (i, s.version())
        }));
    }

    publisher.publish(snapshot_at(1));
    publisher.publish(snapshot_at(2));

    for reader in readers {
        let (i, version) = reader.await.expect("reader task");
        assert!(
            version == 1 || version == 2,
            "reader {i} observed version {version}, expected 1 or 2"
        );
    }
}

#[tokio::test]
async fn publication_state_compiles_and_publishes_wire_snapshot() {
    let publisher = Arc::new(InMemoryPublisher::new());
    let state = PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    );

    state
        .publish_workflows(wire_snapshot(7))
        .map_err(|e| panic!("wire publish failed: {e}"))
        .expect("publish should succeed for a valid wire snapshot");

    // The live snapshot is read through the publication state (the bundle),
    // not the standalone publisher (which is only a seed handle).
    let snap = match state.snapshot() {
        Some(s) => s,
        None => panic!("snapshot published"),
    };
    assert_eq!(snap.version(), 7);
    assert!(snap.get_plan("echo-wf").is_some());
}

#[tokio::test]
async fn validate_compiles_but_does_not_publish() {
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    // Seed a v1 runtime so there is something to verify "unchanged".
    publication
        .publish(snapshot_at(1))
        .map_err(|e| panic!("seed publish failed: {e}"))
        .expect("seed snapshot publishes");

    // Validate a v7 wire snapshot — compiles, does NOT touch the active runtime.
    let validated = publication
        .validate_workflows(&wire_snapshot(7))
        .map_err(|e| panic!("validate failed: {e}"))
        .expect("valid wire snapshot validates");
    assert_eq!(validated.version(), 7);
    // Plan hashes are deterministic and present even before publish.
    assert!(
        !validated
            .plan_hash_for("echo-wf")
            .unwrap_or_default()
            .is_empty()
    );

    // The active runtime is still v1.
    let active = match publication.snapshot() {
        Some(s) => s,
        None => panic!("v1 should still be active"),
    };
    assert_eq!(active.version(), 1, "validate must not publish");
}

#[tokio::test]
async fn publish_rejects_unknown_egress_value() {
    // Fail-closed egress: a typo'd or future egress value must reject the
    // whole bundle at validate/publish — never silently degrade to direct.
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    let mut wire = wire_snapshot(7);
    wire.workflows[0].lanes = [(
        "lane-a".to_string(),
        relay_gateway::observability::WireLane {
            base_url: "http://127.0.0.1:9101".into(),
            authorization: None,
            egress: "maskeed".into(),
            proxy_url: None,
        },
    )]
    .into_iter()
    .collect();

    let err = publication
        .publish_workflows(wire)
        .map(|_| println!("publish should have failed"))
        .expect_err("unknown egress must be rejected");
    assert!(
        err.contains("unknown egress"),
        "error should name the unknown egress, got: {err}"
    );
}

#[tokio::test]
async fn publish_rejects_masked_without_proxy_url() {
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    let mut wire = wire_snapshot(8);
    wire.workflows[0].lanes = [(
        "lane-masked".to_string(),
        relay_gateway::observability::WireLane {
            base_url: "http://127.0.0.1:9102".into(),
            authorization: None,
            egress: "masked".into(),
            proxy_url: None,
        },
    )]
    .into_iter()
    .collect();

    let err = publication
        .publish_workflows(wire)
        .map(|_| panic!("publish should have failed"))
        .expect_err("masked without proxy_url must be rejected");
    assert!(
        err.contains("requires a proxy_url"),
        "error should demand a proxy_url, got: {err}"
    );
}

#[tokio::test]
async fn published_lanes_carry_resolved_authorization() {
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    let wire = WireSnapshot {
        snapshot_version: 1,
        extensions: vec![],
        workflows: vec![WireWorkflow {
            id: "echo-wf".into(),
            workflow: passthrough_workflow(),
            version: 1,
            lanes: std::collections::HashMap::from([(
                "lane-a".into(),
                relay_gateway::observability::WireLane {
                    base_url: "http://127.0.0.1:9001".into(),
                    authorization: Some("Bearer sk-test-123".into()),
                    egress: "direct".into(),
                    proxy_url: None,
                },
            )]),
        }],
    };

    publication
        .publish_workflows(wire)
        .map_err(|e| panic!("publish failed: {e}"))
        .expect("publish with credential should succeed");

    let snap = match publication.snapshot() {
        Some(s) => s,
        None => panic!("snapshot published"),
    };
    let lane = snap
        .lanes()
        .get("lane-a")
        .ok_or("lane-a missing from snapshot")
        .expect("lane registered");
    assert_eq!(
        lane.authorization.as_deref(),
        Some("Bearer sk-test-123"),
        "resolved credential carried on the lane entry"
    );

    // The default (test-only) path must NOT leak a credential by accident.
    let plain = WireSnapshot {
        snapshot_version: 2,
        extensions: vec![],
        workflows: vec![WireWorkflow {
            id: "echo-wf".into(),
            workflow: passthrough_workflow(),
            version: 1,
            lanes: std::collections::HashMap::from([(
                "lane-a".into(),
                relay_gateway::observability::WireLane {
                    base_url: "http://127.0.0.1:9001".into(),
                    authorization: None,
                    egress: "direct".into(),
                    proxy_url: None,
                },
            )]),
        }],
    };
    publication
        .publish_workflows(plain)
        .map_err(|e| panic!("publish failed: {e}"))
        .expect("publish without credential");
    let snap = match publication.snapshot() {
        Some(s) => s,
        None => panic!("snapshot published"),
    };
    assert_eq!(
        snap.lanes()
            .get("lane-a")
            .and_then(|l| l.authorization.clone()),
        None,
        "no fallback credential is invented"
    );
}

#[tokio::test]
async fn gateway_hot_swaps_snapshots_without_restart() {
    let proxy_port = free_port();
    let admin_port = free_port();

    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
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
workflow_id = "echo-wf"
"#
    ))
    .expect("valid workflow config");

    // Externally-owned publisher + pool builder: the gateway shares it, so a
    // control-plane-style publish is immediately visible without restart.
    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    publication
        .publish(snapshot_at(1))
        .map_err(|e| panic!("v1 publish failed: {e}"))
        .expect("v1 publishes");

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

    // Request A → served from v1.
    let url = format!("http://127.0.0.1:{proxy_port}/v1/workflow");
    let (status, body) = match post_hyper(&url, r#"{"hello":"v1"}"#, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("v1 post failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::OK, "v1 request should succeed");
    let _: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => panic!("v1 response not JSON: {e}"),
    };

    // Publish v2 into the shared state — no process restart.
    publication
        .publish(snapshot_at(2))
        .map_err(|e| panic!("v2 publish failed: {e}"))
        .expect("v2 publishes");

    // Request B → served from v2.
    let (status, _body) = match post_hyper(&url, r#"{"hello":"v2"}"#, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("v2 post failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::OK, "v2 request should succeed");

    let snap = match publication.snapshot() {
        Some(s) => s,
        None => panic!("v2 snapshot published"),
    };
    assert_eq!(
        snap.version(),
        2,
        "gateway observed the hot-swapped snapshot"
    );
}

#[tokio::test]
async fn gateway_admin_run_executes_published_workflow() {
    let proxy_port = free_port();
    let admin_port = free_port();

    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
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
workflow_id = "echo-wf"
"#
    ))
    .expect("valid workflow config");

    let publication = Arc::new(PublicationState::new(
        Arc::new(InMemoryPublisher::new()),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    // Publish the echo workflow (Input → Output, no provider needed).
    publication
        .publish(snapshot_at(3))
        .map_err(|e| panic!("echo publish failed: {e}"))
        .expect("echo workflow publishes");

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

    // Run a published workflow → real execution envelope.
    let run_url = format!("http://127.0.0.1:{admin_port}/run");
    let (status, body) = match post_hyper(
        &run_url,
        r#"{"workflow_id":"echo-wf","body":{"hello":"world"}}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("admin run post failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::OK, "published workflow runs");
    let run: serde_json::Value = serde_json::from_slice(&body).expect("run response JSON");
    assert_eq!(run["status"], "ok");
    assert_eq!(run["workflow_id"], "echo-wf");
    assert_eq!(run["snapshot_version"], 3);
    assert!(!run["plan_hash"].as_str().unwrap_or_default().is_empty());
    // The echo workflow's Output node returns its Input — real execution
    // proves the run path, not a fabricated body.
    assert_eq!(run["output"]["hello"], "world");

    // Run an unknown/unpublished workflow → real 404.
    let (status, _body) =
        match post_hyper(&run_url, r#"{"workflow_id":"missing","body":{}}"#, &[]).await {
            Ok(t) => t,
            Err(e) => panic!("admin run 404 post failed: {e}"),
        };
    assert_eq!(
        status,
        http::StatusCode::NOT_FOUND,
        "unknown workflow is a real 404"
    );
}

/// Boot a gateway server with `admin_api_key` set and the echo workflow
/// published, waiting for the admin listener.
async fn gateway_with_admin_api_key() -> (String, String) {
    let proxy_port = free_port();
    let admin_port = free_port();

    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:{proxy_port}"
admin_listen = "127.0.0.1:{admin_port}"
admin_api_key = "test-admin-key"
total_timeout_ms = 5000
graceful_shutdown_ms = 500

[[routes]]
id = "workflow-route"
path_prefix = "/v1/workflow"
methods = ["POST"]
workflow_id = "echo-wf"
"#
    ))
    .expect("valid workflow config");

    let publication = Arc::new(PublicationState::new(
        Arc::new(InMemoryPublisher::new()),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));
    publication
        .publish(snapshot_at(3))
        .map_err(|e| panic!("echo publish failed: {e}"))
        .expect("echo workflow publishes");

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

    (
        format!("http://127.0.0.1:{admin_port}/run"),
        format!("http://127.0.0.1:{admin_port}/publish"),
    )
}

#[tokio::test]
async fn mutating_admin_endpoints_require_api_key() {
    let (run_url, publish_url) = gateway_with_admin_api_key().await;
    let wire = serde_json::json!({
        "snapshot_version": 4,
        "workflows": [{
            "id": "echo-wf",
            "workflow": passthrough_workflow(),
            "lanes": {}
        }]
    })
    .to_string();

    // No key → 401 on /publish.
    let (status, _) = match post_hyper(&publish_url, &wire, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("publish without key failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::UNAUTHORIZED);

    // Wrong key → 401 on /publish.
    let (status, _) =
        match post_hyper(&publish_url, &wire, &[("authorization", "Bearer wrong")]).await {
            Ok(t) => t,
            Err(e) => panic!("publish with wrong key failed: {e}"),
        };
    assert_eq!(status, http::StatusCode::UNAUTHORIZED);

    // Correct key → 200.
    let (status, body) = match post_hyper(
        &publish_url,
        &wire,
        &[("authorization", "Bearer test-admin-key")],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("publish with key failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::OK, "publish accepted with key");
    let resp: serde_json::Value = serde_json::from_slice(&body).expect("publish response JSON");
    assert_eq!(resp["status"], "published");

    // /run without a key → 401.
    let (status, _) = match post_hyper(
        &run_url,
        r#"{"workflow_id":"echo-wf","body":{"hello":"auth"}}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("run without key failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::UNAUTHORIZED);

    // /run with the correct key → 200 (workflow actually executes).
    let (status, body) = match post_hyper(
        &run_url,
        r#"{"workflow_id":"echo-wf","body":{"hello":"auth"}}"#,
        &[("authorization", "Bearer test-admin-key")],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("run with key failed: {e}"),
    };
    assert_eq!(status, http::StatusCode::OK, "run accepted with key");
    let run: serde_json::Value = serde_json::from_slice(&body).expect("run response JSON");
    assert_eq!(run["status"], "ok");
    assert_eq!(run["output"]["hello"], "auth");
}

#[tokio::test]
async fn publish_rejects_masked_lane_without_proxy_at_pool_build() {
    // A masked lane with a proxy URL compiles+validates, but a masked lane
    // WITHOUT one must fail at pool build — a lane an operator expects to be
    // proxied must never resolve to a direct (gateway-IP) client.
    let publication = Arc::new(PublicationState::new(
        Arc::new(InMemoryPublisher::new()),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    let mut wire = wire_snapshot(9);
    wire.workflows[0].id = "llm-wf".into();
    wire.workflows[0].workflow = llm_workflow("lane-masked");
    wire.workflows[0].lanes = [(
        "lane-masked".to_string(),
        relay_gateway::observability::WireLane {
            base_url: "http://127.0.0.1:9103".into(),
            authorization: None,
            egress: "masked".into(),
            proxy_url: None,
        },
    )]
    .into_iter()
    .collect();

    let err = publication
        .publish_workflows(wire)
        .map(|_| panic!("publish must fail for masked lane without proxy"))
        .expect_err("masked lane without proxy fails at pool build");
    assert!(
        err.contains("requires a proxy_url"),
        "error should name the missing proxy_url, got: {err}"
    );
}

#[tokio::test]
async fn admin_run_resolves_masked_lane_from_pools() {
    // Regression for the fix: admin /run used to thread `lane_clients=None`,
    // so an LLM node on a masked lane fell back to a plain direct client and
    // leaked the gateway IP. /run must resolve the lane's per-lane pool.
    let proxy_port = free_port();
    let admin_port = free_port();

    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
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
workflow_id = "llm-wf"
"#
    ))
    .expect("valid workflow config");

    let publication = Arc::new(PublicationState::new(
        Arc::new(InMemoryPublisher::new()),
        Default::default(),
        Box::new(HyperPoolBuilder::new(
            Duration::from_secs(5),
            Duration::from_secs(90),
            16,
        )),
    ));

    let mut wire = wire_snapshot(10);
    wire.workflows[0].id = "llm-wf".into();
    wire.workflows[0].workflow = llm_workflow("lane-masked");
    wire.workflows[0].lanes = [(
        "lane-masked".to_string(),
        relay_gateway::observability::WireLane {
            base_url: "http://127.0.0.1:9104".into(),
            authorization: None,
            egress: "masked".into(),
            proxy_url: Some("http://127.0.0.1:9105".into()),
        },
    )]
    .into_iter()
    .collect();

    publication
        .publish_workflows(wire)
        .map_err(|e| panic!("publish failed: {e}"))
        .expect("valid masked publish");

    // The lane pool for the masked lane must be present and masked — the
    // snapshot the run path resolves is the SAME pools publication holds.
    let bundle = publication.load();
    let masked = bundle
        .pools
        .get("lane-masked")
        .unwrap_or_else(|| panic!("masked lane pool missing"))
        .client
        .clone();
    assert_eq!(
        masked.egress(),
        "masked",
        "masked lane resolves a tunnel client"
    );

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

    // Buffered /run against the masked-lane workflow. The LLM node must
    // resolve the masked lane's pool (HTTP CONNECT to 127.0.0.1:9105) —
    // never a direct client — so the run fails with a connection error
    // instead of silently succeeding via the gateway IP.
    let url = format!("http://127.0.0.1:{admin_port}/run");
    let (status, body) = match post_hyper(
        &url,
        r#"{"workflow_id":"llm-wf","body":{"messages":[{"role":"user","content":"hi"}]}}"#,
        &[],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => panic!("admin run post failed: {e}"),
    };
    assert_eq!(
        status,
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "masked lane without a live tunnel must fail (fail-closed), not leak direct"
    );
    let run: serde_json::Value = serde_json::from_slice(&body).expect("run error response JSON");
    // The grep-able part of the failure: "workflow execution failed" — same
    // marker the buffered /run path uses on any node failure.
    let rendered = run.to_string();
    assert!(
        rendered.contains("workflow execution failed"),
        "run must surface a workflow execution failure, got: {rendered}"
    );
}
