//! Test harness: in-process mock upstream + gateway spawn helpers.
//!
//! Lives in a library crate (not under `tests/`) so its public helpers are
//! never flagged as dead code by any integration-test binary that uses a
//! subset of them.

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use relay_gateway::config::GatewayConfig;
use relay_gateway::server::GatewayServer;

use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

/// Pick an ephemeral free port by binding and dropping the listener.
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(18080)
}

/// An error produced when a test fixture fails to spawn.
#[derive(Debug)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpawnError {}

/// Addresses the gateway binds.
#[derive(Clone, Copy, Debug)]
pub struct GatewayAddrs {
    pub proxy: SocketAddr,
    pub admin: SocketAddr,
}

/// Spawn a gateway whose single lane forwards to `upstream_addr`.
/// Serves `/v1/echo` and `/v1/chat/completions`.
///
/// Waits (up to 5s) for the proxy listener to accept before returning,
/// so tests don't race the bind.
pub async fn spawn_gateway(upstream_addr: SocketAddr) -> Result<GatewayAddrs, SpawnError> {
    spawn_gateway_with_timeout(upstream_addr, 10000).await
}

/// Spawn a gateway with a custom request timeout (in ms).
pub async fn spawn_gateway_with_timeout(
    upstream_addr: SocketAddr,
    timeout_ms: u64,
) -> Result<GatewayAddrs, SpawnError> {
    let port = free_port();
    let admin_port = free_port();
    let listen = SocketAddr::from(([127, 0, 0, 1], port));
    let admin_listen = SocketAddr::from(([127, 0, 0, 1], admin_port));

    let config = GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "{listen}"
admin_listen = "{admin_listen}"
total_timeout_ms = {timeout_ms}
graceful_shutdown_ms = 500

[[routes]]
id = "mock-chat"
path_prefix = "/v1/chat/completions"
methods = ["POST"]
lane = "mock"

[[routes]]
id = "mock-echo"
path_prefix = "/v1/echo"
methods = ["POST", "GET"]
lane = "mock"

[[lanes]]
id = "mock"
base_url = "http://{upstream_addr}"
connect_timeout_ms = 1000
idle_timeout_ms = 60000
frame_timeout_ms = 5000
max_concurrent = 32
max_idle = 16
"#
    ))
    .map_err(|e| SpawnError(format!("gateway config parse: {e}")))?;

    tokio::spawn(async move {
        match GatewayServer::new(config) {
            Ok(server) => {
                if let Err(e) = server.run().await {
                    eprintln!("gateway server exited: {e:#}");
                }
            }
            Err(e) => eprintln!("gateway server failed to start: {e:#}"),
        }
    });

    // Wait for readiness by polling the proxy port.
    let addrs = GatewayAddrs {
        proxy: listen,
        admin: admin_listen,
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(addrs.proxy).await.is_ok() {
            return Ok(addrs);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    Err(SpawnError(format!(
        "gateway did not become ready on {}",
        addrs.proxy
    )))
}

/// A spawned stack: gateway + the mock it routes to.
pub struct TestStack {
    pub gateway: GatewayAddrs,
    pub mock: mock_upstream::MockUpstream,
}

/// Spawn a json-mode mock and a gateway routing to it.
pub async fn spawn_json_stack(json_body: &str) -> Result<TestStack, SpawnError> {
    let mock = spawn_mock(MockConfig {
        mode: MockMode::Json,
        json_body: json_body.to_owned(),
        ..Default::default()
    })
    .await
    .map_err(|e| SpawnError(format!("mock spawn: {e}")))?;
    let gateway = spawn_gateway(mock.addr).await?;
    Ok(TestStack { gateway, mock })
}

/// Spawn an SSE-mode mock and a gateway routing to it.
pub async fn spawn_sse_stack(chunks: usize, chunk_size: usize) -> Result<TestStack, SpawnError> {
    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        chunks,
        chunk_size,
        ..Default::default()
    })
    .await
    .map_err(|e| SpawnError(format!("mock spawn: {e}")))?;
    let gateway = spawn_gateway(mock.addr).await?;
    Ok(TestStack { gateway, mock })
}

/// Bind a listener, note its port, then close it. Race-prone but adequate
/// for pointing the gateway at a dead upstream in connection-refused tests.
pub fn dead_upstream_addr() -> Result<SocketAddr, SpawnError> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| SpawnError(format!("bind dead listener: {e}")))?
        .local_addr()
        .map_err(|e| SpawnError(format!("dead listener address: {e}")))
}

/// HTTP POST via a fresh hyper client; returns (status, body bytes).
pub async fn post_hyper(
    url: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> anyhow::Result<(http::StatusCode, bytes::Bytes)> {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let mut builder = http::Request::builder()
        .method(http::Method::POST)
        .uri(url);
    let mut has_content_type = false;
    for (name, value) in headers {
        builder = builder.header(*name, *value);
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
    }
    if !has_content_type {
        builder = builder.header(http::header::CONTENT_TYPE, "application/json");
    }

    let req = builder.body(axum::body::Body::from(body.as_bytes().to_vec()))?;

    let resp = client.request(req).await?;
    let status = resp.status();
    let collected = http_body_util::BodyExt::collect(resp.into_body()).await?;
    Ok((status, collected.to_bytes()))
}

/// HTTP GET via a fresh hyper client; returns (status, body bytes).
pub async fn get_hyper(url: &str) -> anyhow::Result<(http::StatusCode, bytes::Bytes)> {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = http::Request::builder()
        .method(http::Method::GET)
        .uri(url)
        .body(axum::body::Body::empty())?;
    let resp = client.request(req).await?;
    let status = resp.status();
    let collected = http_body_util::BodyExt::collect(resp.into_body()).await?;
    Ok((status, collected.to_bytes()))
}
