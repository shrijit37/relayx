//! relay-x gateway library.
//!
//! Exposes the pieces integration tests and benchmarks need to embed
//! the gateway in-process: config, server, proxy, errors, observability.

pub mod config;
pub mod errors;
pub mod observability;
pub mod protocol;
pub mod proxy;
pub mod server;
pub mod transport;
pub mod upstream;

/// Convenience: load + compile config from a TOML file.
pub fn load_config(
    path: impl AsRef<std::path::Path>,
) -> Result<config::GatewayConfig, config::ConfigError> {
    config::GatewayConfig::from_file(path)
}
