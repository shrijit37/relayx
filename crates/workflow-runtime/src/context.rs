//! Execution context — runtime information for node execution.

use std::sync::Arc;
use std::time::Duration;

use crate::extension::ExtensionRegistry;
use crate::nodes::RuntimeValue;
use crate::snapshot::RuntimeSnapshot;

/// Type alias for the gateway's shared HTTP client.
pub type GatewayHttpClient = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    axum::body::Body,
>;

/// Type-erased lane-aware HTTP client.
///
/// Each lane has its own connection pool backed by one of three connector
/// families:
///
/// | egress   | Underlying connector                         | Purpose                            |
/// |----------|----------------------------------------------|------------------------------------|
/// | direct   | `HttpConnector` (TCP, no proxy)              | Default cheapest path              |
/// | masked   | `Tunnel<HttpConnector>` (HTTP CONNECT)       | Hide egress IP via HTTP CONNECT    |
/// | masked   | `SocksV5<HttpConnector>` (SOCKS5)            | Hide egress IP via SOCKS5          |
///
/// All three connectors return `TokioIo<TcpStream>` — the same transport —
/// so the `Client`'s response future type is identical across variants.
/// We store each variant as a pre-built `Client` inside an `Arc<dyn Any>`
/// and dispatch via `downcast_ref`, avoiding a heap allocation per request.
///
/// This type implements `tower::Service` so callers use it transparently.
#[derive(Clone)]
pub struct LaneClient {
    egress: String,
    /// Pre-built client keyed by the egress mode.
    client: Arc<dyn std::any::Any + Send + Sync>,
}

/// Per-egress client containers (private — only `LaneClient` dispatches on them).
struct DirectClient(GatewayHttpClient);
type HttpProxyClientInner = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::proxy::Tunnel<
        hyper_util::client::legacy::connect::HttpConnector,
    >,
    axum::body::Body,
>;
struct HttpProxyClient(HttpProxyClientInner);
type Socks5ClientInner = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::proxy::SocksV5<
        hyper_util::client::legacy::connect::HttpConnector,
    >,
    axum::body::Body,
>;
struct Socks5Client(Socks5ClientInner);

impl LaneClient {
    /// Create a direct client (no proxy).
    pub fn direct(idle_timeout: Duration, max_idle: usize) -> Self {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(hyper_util::client::legacy::connect::HttpConnector::new());
        Self {
            egress: "direct".into(),
            client: Arc::new(DirectClient(client)),
        }
    }

    /// Wrap a pre-built shared direct client as a direct `LaneClient`.
    ///
    /// Used by the proxy path for `direct`-egress lanes that have no
    /// published lane pool (pure-proxy deployments): the shared client is
    /// already the gateway's connection pool, so wrapping it adds no new
    /// sockets and keeps the fast path exactly as before. Never used for
    /// `masked` lanes — a masked lane must resolve its own tunneled pool.
    pub fn from_shared(client: Arc<GatewayHttpClient>) -> Self {
        Self {
            egress: "direct".into(),
            client: Arc::new(DirectClient((*client).clone())),
        }
    }

    /// Create an HTTP CONNECT proxy client.
    pub fn http_proxy(
        proxy_url: &str,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        let proxy_uri: http::Uri = proxy_url
            .parse()
            .map_err(|e| format!("invalid proxy_url: {e}"))?;
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();
        let tunnel = hyper_util::client::legacy::connect::proxy::Tunnel::new(proxy_uri, connector);
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(tunnel);
        Ok(Self {
            egress: "masked".into(),
            client: Arc::new(HttpProxyClient(client)),
        })
    }

    /// Create a SOCKS5 proxy client.
    pub fn socks5(
        proxy_url: &str,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        let proxy_uri: http::Uri = proxy_url
            .parse()
            .map_err(|e| format!("invalid proxy_url: {e}"))?;
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();
        let socks = hyper_util::client::legacy::connect::proxy::SocksV5::new(proxy_uri, connector);
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(socks);
        Ok(Self {
            egress: "masked".into(),
            client: Arc::new(Socks5Client(client)),
        })
    }

    /// Build a `LaneClient` from a lane entry using the pool settings.
    ///
    /// Fail-closed egress: `masked` without a `proxy_url` is an error —
    /// the caller must never be handed a direct client for a lane the
    /// operator expects to be proxied (that would leak the gateway IP).
    pub fn from_lane(
        egress: &str,
        proxy_url: Option<&str>,
        _connect_timeout: Duration,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        match egress {
            "direct" => Ok(Self::direct(idle_timeout, max_idle)),
            "masked" => match proxy_url {
                Some(url) if url.starts_with("socks5") => Self::socks5(url, idle_timeout, max_idle),
                Some(url) => Self::http_proxy(url, idle_timeout, max_idle),
                None => Err("lane egress=masked requires a proxy_url".into()),
            },
            other => Err(format!(
                "unknown egress '{other}' (expected 'direct' or 'masked')"
            )),
        }
    }

    /// Egress mode string — direct, masked, or future value.
    pub fn egress(&self) -> &str {
        &self.egress
    }

    /// Whether this client can actually send requests.
    ///
    /// A correctly built direct or tunnel client is always usable; a
    /// downcast-mismatched client (wrong egress container for the mode) is
    /// not. Fallback uses this to decide whether a provider was genuinely
    /// attempted.
    pub fn available(&self) -> bool {
        match self.egress.as_str() {
            "direct" => self.client.downcast_ref::<DirectClient>().is_some(),
            "masked" => {
                self.client.downcast_ref::<HttpProxyClient>().is_some()
                    || self.client.downcast_ref::<Socks5Client>().is_some()
            }
            _ => self.client.downcast_ref::<DirectClient>().is_some(),
        }
    }

    /// Send a request through this lane's client, awaiting the response.
    ///
    /// Mirrors `hyper_util::legacy::Client::request`; the LLM node calls this
    /// so it can use a lane pool transparently.
    pub async fn request(
        &self,
        req: http::Request<axum::body::Body>,
    ) -> Result<http::Response<hyper::body::Incoming>, std::io::Error> {
        match self.egress.as_str() {
            "direct" => {
                let Some(c) = self.client.downcast_ref::<DirectClient>() else {
                    return Err(std::io::Error::other("direct client downcast failed"));
                };
                c.0.request(req).await.map_err(io_err)
            }
            "masked" => {
                if let Some(c) = self.client.downcast_ref::<HttpProxyClient>() {
                    return c.0.request(req).await.map_err(io_err);
                }
                if let Some(c) = self.client.downcast_ref::<Socks5Client>() {
                    return c.0.request(req).await.map_err(io_err);
                }
                Err(std::io::Error::other("masked client downcast failed"))
            }
            _ => {
                let Some(c) = self.client.downcast_ref::<DirectClient>() else {
                    return Err(std::io::Error::other(
                        "fallback direct client downcast failed",
                    ));
                };
                c.0.request(req).await.map_err(io_err)
            }
        }
    }
}

fn io_err(e: hyper_util::client::legacy::Error) -> std::io::Error {
    std::io::Error::other(e)
}

impl tower_service::Service<http::Request<axum::body::Body>> for LaneClient {
    type Response = http::Response<hyper::body::Incoming>;
    type Error = std::io::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<axum::body::Body>) -> Self::Future {
        // All three concrete Client types return Response<Incoming> via
        // `client.request()`.  We dispatch per-variant and box each
        // future into the same pinned-dyn type so the associated type
        // is unified across Direct, HttpProxy, and Socks5.
        match self.egress.as_str() {
            "direct" => {
                let Some(c) = self.client.downcast_ref::<DirectClient>() else {
                    return Box::pin(async {
                        Err(std::io::Error::other("direct client downcast failed"))
                    });
                };
                let client = c.0.clone();
                Box::pin(async move { client.request(req).await.map_err(std::io::Error::other) })
            }
            "masked" => {
                if let Some(c) = self.client.downcast_ref::<HttpProxyClient>() {
                    let client = c.0.clone();
                    return Box::pin(async move {
                        client.request(req).await.map_err(std::io::Error::other)
                    });
                }
                if let Some(c) = self.client.downcast_ref::<Socks5Client>() {
                    let client = c.0.clone();
                    return Box::pin(async move {
                        client.request(req).await.map_err(std::io::Error::other)
                    });
                }
                Box::pin(async { Err(std::io::Error::other("masked client downcast failed")) })
            }
            _ => {
                let Some(c) = self.client.downcast_ref::<DirectClient>() else {
                    return Box::pin(async {
                        Err(std::io::Error::other(
                            "fallback direct client downcast failed",
                        ))
                    });
                };
                let client = c.0.clone();
                Box::pin(async move { client.request(req).await.map_err(std::io::Error::other) })
            }
        }
    }
}

/// Build a gateway HTTP client with keep-alive pooling and a bounded idle
/// pool. This is the single constructor for the plain upstream client —
/// the gateway's per-lane layer wraps it with `retry_canceled_requests`
/// off. Callers wanting the identical default client (admin `/run`,
/// workflow-runtime tests) use this instead of hand-rolling a builder.
pub fn gateway_client(idle_timeout: Duration, max_idle: usize) -> Arc<GatewayHttpClient> {
    Arc::new(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .pool_idle_timeout(idle_timeout)
            .pool_max_idle_per_host(max_idle)
            .build(hyper_util::client::legacy::connect::HttpConnector::new()),
    )
}

/// Runtime context passed to every node during execution.
///
/// Contains no database fields — all state is memory-resident or
/// snapshot-based, keeping the hot path free of round trips.
pub struct ExecutionContext {
    /// The workflow this execution belongs to.
    pub workflow_id: String,
    /// Unique run identifier.
    pub run_id: String,
    /// The node currently executing.
    pub node_id: String,
    /// Cancellation token — checked by nodes that support cancellation.
    pub cancel_token: tokio_util::sync::CancellationToken,
    /// Optional deadline — nodes should abort if exceeded.
    pub deadline: Option<tokio::time::Instant>,
    /// Default timeout for individual node operations.
    pub default_timeout: Duration,
    /// Lane registry for LLM nodes to resolve provider connections.
    pub lane_registry: Arc<LaneRegistry>,
    /// MCP tool executor — if provided, MCP nodes call real tools.
    pub mcp_executor: Option<Arc<dyn McpToolExecutor>>,
    /// Skill loader — if provided, Skill nodes load real skills.
    pub skill_loader: Option<Arc<dyn SkillLoader>>,
    /// The "lane-aware" client resolver: hands back a client per lane name.
    pub lane_clients: Option<Arc<dyn AsLaneClient>>,
    /// Execution metadata (snapshot version, plan hash) for observability.
    pub metadata: ExecutionMetadata,
    /// Snapshot/plan metadata exposed as capability context.
    pub snapshot: Option<Arc<crate::snapshot::RuntimeSnapshot>>,
    /// Optional reporter of execution milestones.
    pub reporter: Arc<dyn crate::milestone::MilestoneReporter>,
    /// Extension registry — maps custom node kinds to validator + executor.
    pub extension_registry: Option<Arc<ExtensionRegistry>>,
    /// Optional SSE wire-bytes sender for token-level streaming.
    /// When present, LLM nodes send formatted SSE token deltas through
    /// this channel as they arrive from upstream providers.
    pub token_sender: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>,
}

/// What snapshot/plan state an execution carries.
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetadata {
    /// Snapshot wall version.
    pub snapshot_version: u64,
    /// Plan hash of the compiled workflow.
    pub plan_hash: String,
    /// The workflow's own version (the ACTIVE version being executed).
    pub workflow_version: u64,
}

impl ExecutionMetadata {
    /// Attach snapshot+plan identity to an existing execution context.
    pub fn from_snapshot(snapshot: &RuntimeSnapshot, workflow_id: &str) -> Self {
        Self {
            snapshot_version: snapshot.version(),
            plan_hash: snapshot
                .plan_hash_for(workflow_id)
                .unwrap_or_default()
                .to_owned(),
            workflow_version: snapshot.workflow_version_for(workflow_id).unwrap_or(0),
        }
    }
}

// ─── Lane-aware client resolver ──────────────────────────────────────────────

/// Resolves an upstream HTTP client for a lane by name.
///
/// The data plane implements this extension trait over its lane snapshot map
/// so a node can obtain the connection pool bound to a specific lane without
/// the runtime knowing anything about per-lane pools.
pub trait AsLaneClient: Send + Sync {
    /// Client for the named lane, if that lane has a pool.
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<LaneClient>>;
}

// ─── MCP tool executor trait ────────────────────────────────────────────────

/// Trait for executing MCP tools. Implementations connect to real MCP servers.
#[async_trait::async_trait]
pub trait McpToolExecutor: Send + Sync {
    /// Execute an MCP tool and return the result.
    async fn execute_tool(
        &self,
        server_ref: &str,
        tool_name: &str,
        input: &RuntimeValue,
    ) -> Result<RuntimeValue, crate::error::NodeError>;
}

// ─── Skill loader trait ─────────────────────────────────────────────────────

/// Trait for loading and applying skills. Implementations connect to skill registries.
#[async_trait::async_trait]
pub trait SkillLoader: Send + Sync {
    /// Load a skill and return its content as a runtime value.
    async fn load_skill(&self, skill_ref: &str) -> Result<RuntimeValue, crate::error::NodeError>;
}

// ─── Lane registry ──────────────────────────────────────────────────────────

/// Registry of available lanes (provider/endpoint combinations).
///
/// Populated from the config snapshot at workflow startup time.
#[derive(Debug, Default)]
pub struct LaneRegistry {
    lanes: std::collections::HashMap<String, LaneEntry>,
}

/// A single lane entry.
#[derive(Debug, Clone)]
pub struct LaneEntry {
    /// Lane identifier.
    pub id: String,
    /// Base URL for the upstream provider.
    pub base_url: url::Url,
    /// Optional static authorization header value (e.g. `Bearer <token>`)
    /// attached to requests routed through this lane. Resolved at publish
    /// time from a `credential_ref` on the control plane — never stored in
    /// workflow JSON. `None` for lanes that carry no auth.
    pub authorization: Option<String>,
    /// Egress mode: `"direct"` (default, gateway IP) or `"masked"`
    /// (via a lane proxy). Unknown values are rejected at publish/pool-build
    /// time — they never silently degrade to `direct`.
    pub egress: String,
    /// Proxy URL for masked egress: `http://host:port` (HTTP CONNECT) or
    /// `socks5://host:port` (SOCKS5). Ignored unless `egress == "masked"`.
    pub proxy_url: Option<String>,
}

impl LaneRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a lane.
    pub fn register(&mut self, entry: LaneEntry) {
        self.lanes.insert(entry.id.clone(), entry);
    }

    /// Look up a lane by ID.
    pub fn get(&self, lane_id: &str) -> Option<&LaneEntry> {
        self.lanes.get(lane_id)
    }

    /// Iterate over all registered lanes.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &LaneEntry)> {
        self.lanes.iter()
    }

    /// Number of registered lanes.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    /// Whether the registry has no lanes.
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }
}

impl ExecutionContext {
    /// Create a new execution context.
    pub fn new(workflow_id: String, run_id: String, lane_registry: Arc<LaneRegistry>) -> Self {
        Self {
            workflow_id,
            run_id,
            node_id: String::new(),
            cancel_token: tokio_util::sync::CancellationToken::new(),
            deadline: None,
            default_timeout: std::time::Duration::from_secs(60),
            lane_registry,
            mcp_executor: None,
            skill_loader: None,
            lane_clients: None,
            metadata: ExecutionMetadata::default(),
            snapshot: None,
            reporter: Arc::new(crate::milestone::NoopReporter),
            extension_registry: None,
            token_sender: None,
        }
    }

    /// Create a child context for a specific node.
    pub fn for_node(&self, node_id: &str) -> Self {
        Self {
            workflow_id: self.workflow_id.clone(),
            run_id: self.run_id.clone(),
            node_id: node_id.to_owned(),
            cancel_token: self.cancel_token.child_token(),
            deadline: self.deadline,
            default_timeout: self.default_timeout,
            lane_registry: self.lane_registry.clone(),
            mcp_executor: self.mcp_executor.clone(),
            skill_loader: self.skill_loader.clone(),
            lane_clients: self.lane_clients.clone(),
            metadata: self.metadata.clone(),
            snapshot: self.snapshot.clone(),
            reporter: self.reporter.clone(),
            extension_registry: self.extension_registry.clone(),
            token_sender: self.token_sender.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
    const TEST_MAX_IDLE: usize = 32;
    const TEST_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn direct_client_builds_successfully() {
        let client = LaneClient::direct(TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert_eq!(client.egress(), "direct");
    }

    #[test]
    fn http_proxy_builds_successfully() {
        let client = LaneClient::http_proxy(
            "http://proxy.example.com:8080",
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        assert_eq!(
            client.ok().map(|c| c.egress().to_owned()),
            Some("masked".into())
        );
    }

    #[test]
    fn socks5_builds_successfully() {
        let client = LaneClient::socks5(
            "socks5://proxy.example.com:1080",
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        assert_eq!(
            client.ok().map(|c| c.egress().to_owned()),
            Some("masked".into())
        );
    }

    #[test]
    fn from_lane_routes_masked_http() {
        let client = LaneClient::from_lane(
            "masked",
            Some("http://proxy.example.com:8080"),
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        assert_eq!(
            client.ok().map(|c| c.egress().to_owned()),
            Some("masked".into())
        );
    }

    #[test]
    fn from_lane_routes_masked_socks5() {
        let client = LaneClient::from_lane(
            "masked",
            Some("socks5://proxy.example.com:1080"),
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        assert_eq!(
            client.ok().map(|c| c.egress().to_owned()),
            Some("masked".into())
        );
    }

    #[test]
    fn from_lane_masked_without_proxy_returns_error() {
        let client = LaneClient::from_lane(
            "masked",
            None,
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(
            client.is_err(),
            "masked egress requires a proxy_url — a direct fallback would leak the gateway IP"
        );
        assert!(
            client
                .err()
                .unwrap_or_default()
                .contains("requires a proxy_url")
        );
    }

    #[test]
    fn from_lane_unknown_egress_returns_error() {
        let client = LaneClient::from_lane(
            "some_future_value",
            None,
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(
            client.is_err(),
            "unknown egress must not silently become a direct client"
        );
    }

    #[test]
    fn http_proxy_invalid_url_returns_error() {
        let client =
            LaneClient::http_proxy("http://exa mple.com:8080", TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert!(client.is_err());
        assert!(client.err().unwrap_or_default().contains("invalid"));
    }

    #[test]
    fn masked_client_without_tunnel_is_not_available() {
        // A lane that resolves to a masked client but lacks a usable tunnel
        // must report unavailable so fallback treats it as "never attempted"
        // instead of sending traffic direct from the gateway IP.
        let client = LaneClient::direct(TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        let client = LaneClient {
            egress: "masked".into(),
            client: client.client,
        };
        assert!(!client.available());
    }

    #[test]
    fn direct_and_masked_clients_are_available() {
        let direct = LaneClient::direct(TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert!(direct.available());
        let masked = match LaneClient::http_proxy(
            "http://proxy.example.com:8080",
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        ) {
            Ok(c) => c,
            Err(e) => panic!("valid proxy should build a masked client: {e}"),
        };
        assert!(masked.available());
    }

    #[test]
    fn socks5_invalid_url_returns_error() {
        let client =
            LaneClient::socks5("http://exa mple.com:8080", TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert!(client.is_err());
        assert!(client.err().unwrap_or_default().contains("invalid"));
    }
}
