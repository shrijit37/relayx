use std::sync::Arc;

use anyhow::Context;
use clap::Parser;

use relay_gateway::config::GatewayConfig;
use relay_gateway::lanes::HyperPoolBuilder;
use relay_gateway::observability::{self, PublicationState};
use relay_gateway::server::GatewayServer;
use workflow_runtime::InMemoryPublisher;

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

    // The gateway always owns a publication state so the admin `/publish`
    // endpoint is live (the control plane drives it). Pure-proxy configs
    // simply never receive a publish; workflow configs become
    // control-plane consumers with an atomic hot-swap bundle.
    let pool_builder = HyperPoolBuilder::new(std::time::Duration::from_secs(90), 64);
    let publication = Arc::new(PublicationState::new(
        Arc::new(InMemoryPublisher::new()),
        Default::default(),
        Box::new(pool_builder),
    ));

    let server = GatewayServer::with_publication(config, Some(publication))?;
    server.run().await
}
