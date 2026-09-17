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

/// Lane-aware HTTP client.
///
/// Each lane has its own connection pool backed by one of three connector
/// families. The three concrete clients have the identical `Response`/
/// `Error` shape (`Response<Incoming>`), so an enum over the variants is the
/// right model: no type erasure, no per-request `downcast_ref`, and no
/// invalid/unknown egress states that would force defensive `_` fallback
/// arms (a fallback that silently treated an unrecognized egress as direct
/// could leak the gateway IP for a masked lane).
///
/// | variant    | Underlying connector                     | Purpose                          |
/// |------------|------------------------------------------|----------------------------------|
/// | `Direct`   | `HttpConnector` (TCP, no proxy)          | Default cheapest path            |
/// | `HttpProxy`| `Tunnel<HttpConnector>` (HTTP CONNECT)   | Hide egress IP via HTTP CONNECT  |
/// | `Socks5`   | `SocksV5<HttpConnector>` (SOCKS5)        | Hide egress IP via SOCKS5        |
///
/// This type implements `tower::Service` so callers use it transparently.
#[derive(Clone)]
pub enum LaneClient {
    /// Plain TCP connector — the gateway IP is the egress.
    Direct(GatewayHttpClient),
    /// HTTP CONNECT tunnel — egress hides behind the proxy address.
    HttpProxy(HttpProxyClientInner),
    /// SOCKS5 tunnel — egress hides behind the proxy address.
    Socks5(Socks5ClientInner),
}

type HttpProxyClientInner = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::proxy::Tunnel<
        hyper_util::client::legacy::connect::HttpConnector,
    >,
    axum::body::Body,
>;
type Socks5ClientInner = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::proxy::SocksV5<
        hyper_util::client::legacy::connect::HttpConnector,
    >,
    axum::body::Body,
>;

impl LaneClient {
    /// Build an `HttpConnector` with the configured connect timeout applied.
    /// The OS-level TCP connect bound is the operator's `connect_timeout_ms`:
    /// without it, an unreachable proxy/upstream blocks in connect for the
    /// OS default (potentially minutes) inside a workflow step.
    fn base_connector(
        connect_timeout: Duration,
    ) -> hyper_util::client::legacy::connect::HttpConnector {
        let mut connector = hyper_util::client::legacy::connect::HttpConnector::new();
        connector.set_connect_timeout(Some(connect_timeout));
        connector
    }

    /// Create a direct client (no proxy).
    pub fn direct(idle_timeout: Duration, max_idle: usize) -> Self {
        Self::direct_with_timeout(Duration::from_secs(5), idle_timeout, max_idle)
    }

    /// Create a direct client with an explicit connect timeout.
    pub fn direct_with_timeout(
        connect_timeout: Duration,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Self {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(Self::base_connector(connect_timeout));
        LaneClient::Direct(client)
    }

    /// Wrap a pre-built shared direct client as a direct `LaneClient`.
    ///
    /// Used by the proxy path for `direct`-egress lanes that have no
    /// published lane pool (pure-proxy deployments): the shared client is
    /// already the gateway's connection pool, so wrapping it adds no new
    /// sockets and keeps the fast path exactly as before. Never used for
    /// `masked` lanes — a masked lane must resolve its own tunneled pool.
    pub fn from_shared(client: Arc<GatewayHttpClient>) -> Self {
        LaneClient::Direct((*client).clone())
    }

    /// Create an HTTP CONNECT proxy client.
    ///
    /// Rejects any URL whose scheme is not `http`/`https`: the tunnel
    /// connector speaks HTTP CONNECT, so an unrelated scheme would only
    /// fail later, at request time, with an opaque error.
    pub fn http_proxy(
        proxy_url: &str,
        connect_timeout: Duration,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        let proxy_uri: http::Uri = proxy_url
            .parse()
            .map_err(|e| format!("invalid proxy_url: {e}"))?;
        let scheme = proxy_uri
            .scheme_str()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "invalid proxy_url: scheme '{scheme}' is not supported for HTTP CONNECT (use http:// or https://)"
            ));
        }
        let connector = Self::base_connector(connect_timeout);
        let tunnel = hyper_util::client::legacy::connect::proxy::Tunnel::new(proxy_uri, connector);
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(tunnel);
        Ok(LaneClient::HttpProxy(client))
    }

    /// Create a SOCKS5 proxy client.
    ///
    /// Rejects any URL whose scheme is not exactly `socks5` (the RFC scheme;
    /// `socks5h://` and friends are not part of it).
    pub fn socks5(
        proxy_url: &str,
        connect_timeout: Duration,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        let proxy_uri: http::Uri = proxy_url
            .parse()
            .map_err(|e| format!("invalid proxy_url: {e}"))?;
        let scheme = proxy_uri
            .scheme_str()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if scheme != "socks5" {
            return Err(format!(
                "invalid proxy_url: scheme '{scheme}' is not SOCKS5 (use socks5://)"
            ));
        }
        let connector = Self::base_connector(connect_timeout);
        let socks = hyper_util::client::legacy::connect::proxy::SocksV5::new(proxy_uri, connector);
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(idle_timeout)
                .pool_max_idle_per_host(max_idle)
                .build(socks);
        Ok(LaneClient::Socks5(client))
    }

    /// Build a `LaneClient` from a lane entry using the pool settings.
    ///
    /// Fail-closed egress: `masked` without a `proxy_url` is an error —
    /// the caller must never be handed a direct client for a lane the
    /// operator expects to be proxied (that would leak the gateway IP).
    /// `connect_timeout` bounds the TCP connect for every variant.
    pub fn from_lane(
        egress: &str,
        proxy_url: Option<&str>,
        connect_timeout: Duration,
        idle_timeout: Duration,
        max_idle: usize,
    ) -> Result<Self, String> {
        match egress {
            "direct" => Ok(Self::direct_with_timeout(
                connect_timeout,
                idle_timeout,
                max_idle,
            )),
            "masked" => match proxy_url {
                Some(url) => {
                    let scheme = url
                        .split_once("://")
                        .map(|(s, _)| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    match scheme.as_str() {
                        "socks5" => Self::socks5(url, connect_timeout, idle_timeout, max_idle),
                        "http" | "https" => {
                            Self::http_proxy(url, connect_timeout, idle_timeout, max_idle)
                        }
                        _ => Err(format!(
                            "lane egress=masked: unsupported proxy scheme '{scheme}' in '{url}' (expected socks5://, http://, or https://)"
                        )),
                    }
                }
                None => Err("lane egress=masked requires a proxy_url".into()),
            },
            other => Err(format!(
                "unknown egress '{other}' (expected 'direct' or 'masked')"
            )),
        }
    }

    /// Egress mode — `"direct"` or `"masked"` (derived from the variant, so
    /// an invalid state is unrepresentable).
    pub fn egress(&self) -> &str {
        match self {
            LaneClient::Direct(_) => "direct",
            LaneClient::HttpProxy(_) | LaneClient::Socks5(_) => "masked",
        }
    }

    /// Whether this client can actually send requests.
    ///
    /// Every variant holds a correctly built client, so this is always
    /// true for constructed `LaneClient`s. Kept for API compatibility with
    /// the previous downcast-era signature.
    pub fn available(&self) -> bool {
        true
    }

    /// Send a request through this lane's client, awaiting the response.
    ///
    /// Mirrors `hyper_util::legacy::Client::request`; the LLM node calls this
    /// so it can use a lane pool transparently.
    pub async fn request(
        &self,
        req: http::Request<axum::body::Body>,
    ) -> Result<http::Response<hyper::body::Incoming>, std::io::Error> {
        match self {
            LaneClient::Direct(c) => c
                .request(req)
                .await
                .map_err(|e| std::io::Error::other(format!("direct lane transport error: {e}"))),
            LaneClient::HttpProxy(c) => c.request(req).await.map_err(|e| {
                std::io::Error::other(format!("http-proxy lane transport error: {e}"))
            }),
            LaneClient::Socks5(c) => c
                .request(req)
                .await
                .map_err(|e| std::io::Error::other(format!("socks5 lane transport error: {e}"))),
        }
    }
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
        // `client.request()`. Dispatch per-variant and box each future into
        // the same pinned-dyn type so the associated type is unified across
        // Direct, HttpProxy, and Socks5. Transport errors keep their lane
        // context ("http-proxy lane transport error") so a configuration
        // defect is distinguishable from an upstream outage in the logs.
        match self {
            LaneClient::Direct(c) => {
                let client = c.clone();
                Box::pin(async move {
                    client.request(req).await.map_err(|e| {
                        std::io::Error::other(format!("direct lane transport error: {e}"))
                    })
                })
            }
            LaneClient::HttpProxy(c) => {
                let client = c.clone();
                Box::pin(async move {
                    client.request(req).await.map_err(|e| {
                        std::io::Error::other(format!("http-proxy lane transport error: {e}"))
                    })
                })
            }
            LaneClient::Socks5(c) => {
                let client = c.clone();
                Box::pin(async move {
                    client.request(req).await.map_err(|e| {
                        std::io::Error::other(format!("socks5 lane transport error: {e}"))
                    })
                })
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
        assert!(client.available());
    }

    #[test]
    fn http_proxy_builds_successfully() {
        let client = LaneClient::http_proxy(
            "http://proxy.example.com:8080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        let client = match client {
            Ok(c) => c,
            Err(e) => panic!("valid proxy should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
        assert!(client.available());
    }

    #[test]
    fn socks5_builds_successfully() {
        let client = LaneClient::socks5(
            "socks5://proxy.example.com:1080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        let client = match client {
            Ok(c) => c,
            Err(e) => panic!("valid socks5 should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
        assert!(client.available());
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
        let client = match client {
            Ok(c) => c,
            Err(e) => panic!("masked http should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
        assert!(client.available());
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
        let client = match client {
            Ok(c) => c,
            Err(e) => panic!("masked socks5 should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
        assert!(client.available());
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
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("masked without proxy must fail closed"),
        };
        assert!(
            err.contains("requires a proxy_url"),
            "error should name the missing proxy, got: {err}"
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
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("unknown egress must fail closed"),
        };
        assert!(err.contains("unknown egress"), "got: {err}");
    }

    #[test]
    fn http_proxy_invalid_url_returns_error() {
        let client = LaneClient::http_proxy(
            "http://exa mple.com:8080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_err());
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("invalid URL must fail closed"),
        };
        assert!(err.contains("invalid"), "got: {err}");
    }

    #[test]
    fn http_proxy_rejects_non_http_scheme() {
        // SOCKS5 URL fed to the HTTP CONNECT constructor must be rejected at
        // build time, not fail opaquely at request time.
        let client = LaneClient::http_proxy(
            "socks5://proxy.example.com:1080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_err());
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("socks5 URL must be rejected by http_proxy"),
        };
        assert!(err.contains("scheme"), "got: {err}");
    }

    #[test]
    fn socks5_rejects_non_socks5_scheme() {
        let client = LaneClient::socks5(
            "http://proxy.example.com:8080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_err());
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("http URL must be rejected by socks5"),
        };
        assert!(err.contains("scheme"), "got: {err}");
    }

    #[test]
    fn socks5h_scheme_is_rejected() {
        // socks5h is NOT part of the RFC; only socks5 is accepted.
        let client = LaneClient::socks5(
            "socks5h://proxy.example.com:1080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_err());
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("socks5h must be rejected"),
        };
        assert!(err.contains("scheme"), "got: {err}");
    }

    #[test]
    fn from_lane_uppercase_socks5_scheme_routes_to_socks5() {
        // Scheme sniffing is case-insensitive: SOCKS5:// must not be
        // misrouted to HTTP CONNECT.
        let client = LaneClient::from_lane(
            "masked",
            Some("SOCKS5://proxy.example.com:1080"),
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_ok());
        let client = match client {
            Ok(c) => c,
            Err(e) => panic!("uppercase SOCKS5 should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
    }

    #[test]
    fn masked_without_tunnel_is_unrepresentable() {
        // The enum makes a "masked but holding a direct client" state
        // unrepresentable — a direct client built via `direct()` reports
        // egress "direct", so fallback can never mistake it for masked.
        let client = LaneClient::direct(TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert_eq!(client.egress(), "direct");
        assert!(client.available());
    }

    #[test]
    fn direct_and_masked_clients_are_available() {
        let direct = LaneClient::direct(TEST_IDLE_TIMEOUT, TEST_MAX_IDLE);
        assert!(direct.available());
        let masked = match LaneClient::http_proxy(
            "http://proxy.example.com:8080",
            TEST_CONNECT_TIMEOUT,
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
        let client = LaneClient::socks5(
            "http://exa mple.com:8080",
            TEST_CONNECT_TIMEOUT,
            TEST_IDLE_TIMEOUT,
            TEST_MAX_IDLE,
        );
        assert!(client.is_err());
        let err = match client {
            Err(e) => e,
            Ok(_) => panic!("invalid URL must fail closed"),
        };
        assert!(err.contains("invalid"), "got: {err}");
    }
}
