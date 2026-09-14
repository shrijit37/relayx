use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::routing::any;
use tokio::net::TcpListener;

use crate::config::ConfigSnapshot;
use crate::lanes::LanePools;
use crate::observability::PublicationState;
use crate::proxy::proxy_handler;
use workflow_runtime::InMemoryPublisher;

/// Shared gateway state, immutable after startup.
pub struct AppState {
    pub config: Arc<ConfigSnapshot>,
    pub client: Arc<
        hyper_util::client::legacy::Client<
            hyper_util::client::legacy::connect::HttpConnector,
            Body,
        >,
    >,
    /// Per-request overall timeout deadline.
    pub timeout: Duration,
    /// Runtime publication state: the active compiled snapshot and the
    /// per-lane connection pools, hot-swappable via `PublicationState`.
    /// None for pure proxy deployments that don't execute workflows.
    pub publication: Option<Arc<PublicationState>>,
}

/// The gateway server — binds listeners and serves traffic.
pub struct GatewayServer {
    config: Arc<ConfigSnapshot>,
    server_config: crate::config::ServerConfig,
    metrics_handle: metrics_exporter_prometheus::PrometheusHandle,
    /// Compiled workflow snapshot, when the deployment executes workflows.
    workflow_snapshot: Option<Arc<workflow_runtime::RuntimeSnapshot>>,
    /// Shared publication state (publisher + per-lane pools). When present,
    /// the server publishes into it and hot-swaps are visible to its workers.
    publication: Option<Arc<PublicationState>>,
}

impl GatewayServer {
    /// Create a new server from a parsed config.
    pub fn new(raw_config: crate::config::GatewayConfig) -> Result<Self, anyhow::Error> {
        Self::with_snapshot(raw_config, None)
    }

    /// Create a new server that also carries a compiled workflow snapshot,
    /// enabling `workflow_id` routes to execute plans.
    pub fn with_snapshot(
        raw_config: crate::config::GatewayConfig,
        workflow_snapshot: Option<Arc<workflow_runtime::RuntimeSnapshot>>,
    ) -> Result<Self, anyhow::Error> {
        let server_config = raw_config.server.clone();
        let config = raw_config.compile()?;

        // Install the metrics recorder early so all `metrics::` macros resolve.
        let metrics_handle = crate::observability::install_metrics()?;

        Ok(Self {
            config: Arc::new(config),
            server_config,
            metrics_handle,
            workflow_snapshot,
            publication: None,
        })
    }

    /// Create a server that shares an externally-owned publication state.
    ///
    /// The caller controls the publisher (e.g. control-plane tests or an
    /// embedded control plane), so each `publish` is immediately visible to
    /// this gateway's data plane without a restart.
    pub fn with_publication(
        raw_config: crate::config::GatewayConfig,
        publication: Option<Arc<PublicationState>>,
    ) -> Result<Self, anyhow::Error> {
        let server_config = raw_config.server.clone();
        let config = raw_config.compile()?;

        let metrics_handle = crate::observability::install_metrics()?;

        Ok(Self {
            config: Arc::new(config),
            server_config,
            metrics_handle,
            workflow_snapshot: None,
            publication,
        })
    }

    /// Run the gateway until shutdown signal.
    pub async fn run(self) -> anyhow::Result<()> {
        // ── Proxy client (connection pool) ─────────────────────────────────
        let client = crate::upstream::build_http_client(Duration::from_secs(90), 64);

        // ── Per-lane pools + snapshot publisher ────────────────────────────
        let pool_builder = crate::lanes::HyperPoolBuilder::new(Duration::from_secs(90), 64);
        let publication_state = match (self.publication.clone(), self.workflow_snapshot.clone()) {
            (Some(external), _) => Some(external),
            (None, Some(snap)) => {
                let state = Arc::new(PublicationState::new(
                    Arc::new(InMemoryPublisher::new()),
                    LanePools::build(&snap, &pool_builder),
                    Box::new(pool_builder),
                ));
                state.publish(snap);
                Some(state)
            }
            (None, None) => None,
        };

        let state = Arc::new(AppState {
            config: self.config.clone(),
            client: Arc::new(client),
            timeout: Duration::from_millis(self.server_config.total_timeout_ms),
            publication: publication_state.clone(),
        });

        // ── Proxy listener ────────────────────────────────────────────────
        let proxy_router = Router::new()
            .fallback(any(proxy_handler))
            .with_state(state.clone());

        let proxy_listener = TcpListener::bind(self.server_config.listen).await?;
        tracing::info!(addr = %self.server_config.listen, "proxy listener started");

        // ── Admin listener ────────────────────────────────────────────────
        let admin_router = match &publication_state {
            Some(ps) => crate::observability::admin_router_with_publication(
                self.metrics_handle.clone(),
                Some(ps.clone()),
                self.server_config.admin_api_key.clone(),
            ),
            None => crate::observability::admin_router(
                self.metrics_handle.clone(),
                self.server_config.admin_api_key.clone(),
            ),
        };

        let admin_listener = TcpListener::bind(self.server_config.admin_listen).await?;
        tracing::info!(addr = %self.server_config.admin_listen, "admin listener started");

        // ── Serve both listeners until shutdown ────────────────────────────
        let shutdown_duration = Duration::from_millis(self.server_config.graceful_shutdown_ms);

        // Serve proxy until ctrl-c.
        let proxy_future = axum::serve(proxy_listener, proxy_router.into_make_service());

        let admin_future = axum::serve(admin_listener, admin_router.into_make_service());

        // `axum::serve` returns a `Serve` which implements `IntoFuture`,
        // not `Future` directly — wrap it before spawning.
        let admin_handle = tokio::spawn(admin_future.into_future());

        tracing::info!("relay-gateway ready");

        tokio::select! {
            result = proxy_future => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "proxy server error");
                    return Err(anyhow::anyhow!(e));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutdown signal received");
            }
        }

        // Graceful shutdown: stop accepting, let in-flight requests drain.
        admin_handle.abort();
        tokio::time::sleep(shutdown_duration).await;
        tracing::info!("relay-gateway stopped");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http::Request;
    use std::sync::{Once, OnceLock};
    use tower::ServiceExt;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Create a test admin router with a test-installed metrics recorder.
    fn test_admin_router() -> anyhow::Result<axum::Router> {
        static HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();
        static INIT: Once = Once::new();
        static INIT_ERR: OnceLock<String> = OnceLock::new();

        INIT.call_once(|| {
            match metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder() {
                Ok(handle) => {
                    let _ = HANDLE.set(handle);
                }
                Err(e) => {
                    let _ = INIT_ERR.set(e.to_string());
                }
            }
        });

        if let Some(err) = INIT_ERR.get() {
            return Err(anyhow::anyhow!("install metrics recorder: {err}"));
        }

        let handle = HANDLE
            .get()
            .ok_or_else(|| anyhow::anyhow!("metrics recorder handle unavailable"))?
            .clone();
        Ok(crate::observability::admin_router(handle, None))
    }

    #[tokio::test]
    async fn test_healthz_returns_200() -> TestResult {
        let router = test_admin_router()?;
        let req = Request::builder().uri("/healthz").body(Body::empty())?;
        let response = router.oneshot(req).await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn test_ready_returns_200() -> TestResult {
        let router = test_admin_router()?;
        let req = Request::builder().uri("/ready").body(Body::empty())?;
        let response = router.oneshot(req).await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        Ok(())
    }
}
