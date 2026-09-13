//! Standalone mock LLM upstream for manual testing and benchmarks.
//!
//! Usage:
//!   mock-upstream --port 8101 [--mode json] [--chunks 10] [--chunk-size 512] [--ttfb-ms 0]

use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use mock_upstream::{MockConfig, MockMode, spawn_mock_on};

#[derive(Parser, Debug)]
#[command(
    name = "mock-upstream",
    version,
    about = "Mock LLM upstream for relay-x"
)]
struct Cli {
    /// Port to listen on.
    #[arg(long, default_value_t = 8101)]
    port: u16,

    /// Response mode: `sse` (streaming) or `json` (single-shot).
    #[arg(long, default_value = "sse")]
    mode: String,

    /// Number of SSE chunks to emit.
    #[arg(long, default_value_t = 10)]
    chunks: usize,

    /// Bytes per SSE chunk.
    #[arg(long, default_value_t = 512)]
    chunk_size: usize,

    /// Artificial latency before response headers (ms).
    #[arg(long, default_value_t = 0)]
    ttfb_ms: u64,

    /// Delay between chunks (ms).
    #[arg(long, default_value_t = 0)]
    chunk_delay_ms: u64,

    /// Response body for JSON mode (defaults to a valid chat completion).
    #[arg(long)]
    json_body: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mode = match cli.mode.as_str() {
        "json" => MockMode::Json,
        "sse" => MockMode::Sse,
        other => {
            anyhow::bail!("unknown mode '{other}' (expected 'sse' or 'json')");
        }
    };

    let config = MockConfig {
        mode,
        chunks: cli.chunks,
        chunk_size: cli.chunk_size,
        ttfb: Duration::from_millis(cli.ttfb_ms),
        chunk_delay: Duration::from_millis(cli.chunk_delay_ms),
        json_body: cli.json_body.unwrap_or_else(|| {
            r#"{"id":"chatcmpl-1","object":"chat.completion","created":1234567890,"model":"gpt-4","choices":[{"index":0,"message":{"role":"assistant","content":"Hello from the Phase 6 live stack"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":6,"total_tokens":11}}"#
                .into()
        }),
        ..Default::default()
    };

    let listen = SocketAddr::from(([0, 0, 0, 0], cli.port));
    let mock = spawn_mock_on(config, listen).await?;
    let addr = mock.addr;

    println!("mock-upstream listening on {} (mode={})", addr, cli.mode);
    println!("  /v1/echo          POST echo server");
    println!("  /v1/chat/completions  POST (SSE or JSON)");
    println!("  /health          GET health");
    println!("  /stats           GET counters");
    println!();
    println!("Press Ctrl+C to stop.");

    // Keep mock alive until Ctrl+C. Drop sends the shutdown signal.
    tokio::signal::ctrl_c().await?;
    drop(mock);

    Ok(())
}
