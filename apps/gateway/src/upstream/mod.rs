//! Upstream connection management.
//!
//! Phase 1 uses a single hyper-util legacy client as the connection pool.
//! Pool keying is by upstream address (which hyper's own pool does).
//! Per-lane isolation arrives with Phase 3 lanes.
//!
//! The hot-path guarantee: the client is built once at startup and never
//! rebuilds connectors per-request.

use std::time::Duration;

/// Build the shared hyper client used for all upstream requests.
///
/// This client maintains a connection pool keyed by destination authority.
/// Connections are kept alive and reused. Idle connections below
/// `idle_timeout` are returned to the pool; stale ones are closed.
pub fn build_http_client(
    idle_timeout: Duration,
    max_idle_per_host: usize,
) -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    axum::body::Body,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .pool_idle_timeout(idle_timeout)
        .pool_max_idle_per_host(max_idle_per_host)
        .retry_canceled_requests(false)
        .build(hyper_util::client::legacy::connect::HttpConnector::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_client_smoke() {
        let _client = build_http_client(Duration::from_secs(90), 64);
    }
}
