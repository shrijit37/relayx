//! Benchmark harness helpers.
//!
//! Provides `spawn_json_mock` and `spawn_gateway` used by the
//! proxy-overhead criterion benches. Fixtures are spawned once per run in
//! the benchmark's setup phase.

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use relay_gateway::config::GatewayConfig;
use relay_gateway::server::GatewayServer;

use std::net::{SocketAddr, TcpListener};

/// Find an ephemeral free port.
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(18080)
}

/// Spawn a json-mode mock upstream returning `json_body`.
pub async fn spawn_json_mock(json_body: &str) -> anyhow::Result<mock_upstream::MockUpstream> {
    spawn_mock(MockConfig {
        mode: MockMode::Json,
        json_body: json_body.to_owned(),
        ..Default::default()
    })
    .await
    .map_err(|e| anyhow::anyhow!("mock spawn: {e}"))
}

/// Spawn the gateway on an ephemeral port with a single lane to `upstream`.
pub async fn spawn_gateway(upstream_addr: SocketAddr) -> anyhow::Result<SocketAddr> {
    let port = free_port();
    let admin_port = free_port();
    let listen = SocketAddr::from(([127, 0, 0, 1], port));

    let config = GatewayConfig::from_toml_str(&format!(
        r#"
snapshot_version = 1

[server]
listen = "{listen}"
admin_listen = "127.0.0.1:{admin_port}"
total_timeout_ms = 30000
graceful_shutdown_ms = 1000

[[routes]]
id = "echo"
path_prefix = "/v1/echo"
methods = ["POST"]
lane = "mock"

[[lanes]]
id = "mock"
base_url = "http://{upstream_addr}"
connect_timeout_ms = 2000
idle_timeout_ms = 30000
frame_timeout_ms = 10000
max_concurrent = 64
max_idle = 32
"#
    ))
    .map_err(|e| anyhow::anyhow!("gateway config parse: {e}"))?;

    let server_addr = listen;
    tokio::spawn(async move {
        match GatewayServer::new(config) {
            Ok(server) => {
                if let Err(e) = server.run().await {
                    eprintln!("gateway server failed during benchmark: {e:#}");
                }
            }
            Err(e) => eprintln!("gateway server failed to start during benchmark: {e:#}"),
        }
    });

    // Wait until the proxy listener accepts connections.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(server_addr).await.is_ok() {
            return Ok(server_addr);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    Err(anyhow::anyhow!(
        "gateway did not become ready on {server_addr}"
    ))
}
