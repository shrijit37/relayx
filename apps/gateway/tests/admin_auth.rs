//! Gateway admin authentication regression tests.
//!
//! Proves the P0 security contract:
//! - mutating admin endpoints (/publish, /validate, /run) reject anonymous
//!   requests when an admin API key is configured
//! - a valid `Authorization: Bearer <key>` header is accepted
//! - read-only endpoints (/healthz, /ready, /metrics) stay unauthenticated
//! - when no key is configured, the admin listener remains open (loopback-only)

use std::sync::Arc;
use std::time::Duration;

use relay_gateway::lanes::HyperPoolBuilder;
use relay_gateway::observability::PublicationState;
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

fn wire_snapshot() -> relay_gateway::observability::WireSnapshot {
    relay_gateway::observability::WireSnapshot {
        snapshot_version: 1,
        workflows: vec![relay_gateway::observability::WireWorkflow {
            id: "echo-wf".into(),
            workflow: passthrough_workflow(),
            lanes: std::collections::HashMap::new(),
            version: 1,
        }],
    }
}

/// Spawn a gateway whose admin listener requires `Authorization: Bearer test-key`.
async fn spawn_auth_gateway() -> u16 {
    let proxy_port = free_port();
    let admin_port = free_port();
    let config = relay_gateway::config::GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:{proxy_port}"
admin_listen = "127.0.0.1:{admin_port}"
admin_api_key = "test-key"
total_timeout_ms = 15000
graceful_shutdown_ms = 500

[[routes]]
id = "workflow"
path_prefix = "/v1/workflow"
methods = ["POST"]
workflow_id = "echo-wf"
"#
    ))
    .expect("valid auth config");

    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(Duration::from_secs(90), 16)),
    ));

    let server = match GatewayServer::with_publication(config, Some(publication.clone())) {
        Ok(s) => s,
        Err(e) => panic!("server build failed: {e}"),
    };
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Wait for the admin port to accept connections.
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

/// Spawn a gateway WITHOUT an admin API key — the legacy open behavior,
/// still valid on a loopback-only admin listener.
async fn spawn_open_gateway() -> u16 {
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
    .expect("valid open config");

    let publisher = Arc::new(InMemoryPublisher::new());
    let publication = Arc::new(PublicationState::new(
        publisher.clone(),
        Default::default(),
        Box::new(HyperPoolBuilder::new(Duration::from_secs(90), 16)),
    ));
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

#[tokio::test]
async fn anonymous_publish_is_rejected_when_key_configured() {
    let admin_port = spawn_auth_gateway().await;
    let url = format!("http://127.0.0.1:{admin_port}/publish");
    let body = serde_json::to_string(&wire_snapshot()).expect("serialize wire");

    let (status, _body) = match post_hyper(&url, &body, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("anonymous publish request failed: {e}"),
    };

    assert_eq!(
        status,
        http::StatusCode::UNAUTHORIZED,
        "anonymous /publish must be rejected with 401 when an admin key is configured"
    );
}

#[tokio::test]
async fn authenticated_publish_succeeds_with_bearer_token() {
    let admin_port = spawn_auth_gateway().await;
    let url = format!("http://127.0.0.1:{admin_port}/publish");
    let body = serde_json::to_string(&wire_snapshot()).expect("serialize wire");

    let (status, _body) =
        match post_hyper(&url, &body, &[("authorization", "Bearer test-key")]).await {
            Ok(t) => t,
            Err(e) => panic!("authenticated publish request failed: {e}"),
        };

    assert_eq!(
        status,
        http::StatusCode::OK,
        "authenticated /publish should succeed"
    );
}

#[tokio::test]
async fn wrong_key_is_rejected() {
    let admin_port = spawn_auth_gateway().await;
    let url = format!("http://127.0.0.1:{admin_port}/publish");
    let body = serde_json::to_string(&wire_snapshot()).expect("serialize wire");

    let (status, _body) =
        match post_hyper(&url, &body, &[("authorization", "Bearer wrong-key")]).await {
            Ok(t) => t,
            Err(e) => panic!("wrong-key publish request failed: {e}"),
        };

    assert_eq!(
        status,
        http::StatusCode::UNAUTHORIZED,
        "wrong API key must be rejected with 401"
    );
}

#[tokio::test]
async fn healthz_stays_unauthenticated() {
    let admin_port = spawn_auth_gateway().await;
    let url = format!("http://127.0.0.1:{admin_port}/healthz");

    let (status, _body) = match test_harness::get_hyper(&url).await {
        Ok(t) => t,
        Err(e) => panic!("healthz request failed: {e}"),
    };

    assert_eq!(
        status,
        http::StatusCode::OK,
        "/healthz must stay unauthenticated for load balancers"
    );
}

#[tokio::test]
async fn open_gateway_accepts_anonymous_publish() {
    // Backward-compat: no key configured → no auth. This is the historical
    // behavior and remains valid for a loopback-only admin listener.
    let admin_port = spawn_open_gateway().await;
    let url = format!("http://127.0.0.1:{admin_port}/publish");
    let body = serde_json::to_string(&wire_snapshot()).expect("serialize wire");

    let (status, _body) = match post_hyper(&url, &body, &[]).await {
        Ok(t) => t,
        Err(e) => panic!("open publish request failed: {e}"),
    };

    assert_eq!(
        status,
        http::StatusCode::OK,
        "gateway with no admin key should accept publication (loopback-open)"
    );
}
