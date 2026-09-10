use anyhow::Context;
use clap::Parser;

use relay_gateway::config::GatewayConfig;
use relay_gateway::observability;
use relay_gateway::server::GatewayServer;

/// relay-x data plane — ultra-low-latency HTTP gateway.
#[derive(Parser, Debug)]
#[command(name = "relay-gateway", version, about)]
struct Cli {
    /// Path to the gateway TOML configuration file.
    #[arg(short, long, default_value = "config/gateway.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    observability::init_tracing();

    let config = GatewayConfig::from_file(&cli.config)
        .with_context(|| format!("failed to load config from {}", cli.config))?;

    tracing::info!(
        version = config.snapshot_version(),
        routes = config.routes().len(),
        lanes = config.lanes().len(),
        "gateway configuration loaded"
    );

    let server = GatewayServer::new(config)?;
    server.run().await
}
