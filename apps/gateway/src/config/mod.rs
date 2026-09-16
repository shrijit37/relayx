use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use protocol_core::canonical::Protocol;
use serde::Deserialize;
use thiserror::Error;

// ─── Protocol parsing ────────────────────────────────────────────────────────

/// Parse a protocol name from config into a canonical protocol identifier.
///
/// Accepts the short names used in route configs (`anthropic` for shorthand)
/// as well as the canonical display names.
pub fn parse_protocol(name: &str) -> Result<Protocol, ConfigError> {
    match name {
        "openai_chat" | "openai_chat_completions" => Ok(Protocol::OpenAiChatCompletions),
        "anthropic" | "anthropic_messages" => Ok(Protocol::AnthropicMessages),
        "openai_responses" => Ok(Protocol::OpenAiResponses),
        other => Err(ConfigError::Validation(format!(
            "unknown protocol '{other}' (expected 'openai_chat', 'anthropic', or 'openai_responses')"
        ))),
    }
}

/// Default path for each protocol on the upstream.
pub fn protocol_upstream_path(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::OpenAiChatCompletions => "/v1/chat/completions",
        Protocol::AnthropicMessages => "/v1/messages",
        Protocol::OpenAiResponses => "/v1/responses",
    }
}

// ─── Config errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse TOML config: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("config validation failed: {0}")]
    Validation(String),
}

// ─── Raw TOML schema (deserialized directly from file) ─────────────────────

/// Top-level TOML configuration. Deserialized once at startup.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Version tag baked into spans and metrics labels for traceability.
    #[serde(default = "default_snapshot_version")]
    pub snapshot_version: u64,

    pub server: ServerConfig,
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub lanes: Vec<LaneConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Address to bind the main proxy listener (HTTP/1.1).
    #[serde(default = "default_listen_addr")]
    pub listen: SocketAddr,

    /// Address to bind the admin listener (/healthz, /metrics).
    #[serde(default = "default_admin_listen")]
    pub admin_listen: SocketAddr,

    /// Per-request deadline. A request that takes longer is terminated.
    #[serde(default = "default_total_timeout_ms")]
    pub total_timeout_ms: u64,

    /// Graceful shutdown timeout in milliseconds.
    #[serde(default = "default_shutdown_timeout_ms")]
    pub graceful_shutdown_ms: u64,

    /// Shared-secret API key for mutating admin endpoints (/publish, /validate, /run).
    /// The control plane sends `Authorization: Bearer <key>` on every request.
    /// When `None`, the admin listener is unauthenticated (loopback-only).
    #[serde(default)]
    pub admin_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    /// Unique route identifier for logging/metrics.
    pub id: String,

    /// Prefix to match against incoming request paths.
    pub path_prefix: String,

    /// HTTP methods this route accepts (e.g. ["POST"]).
    #[serde(default = "default_methods")]
    pub methods: Vec<String>,

    /// Lane this route forwards to. Required unless `workflow_id` is set.
    #[serde(default)]
    pub lane: String,

    /// Client-facing wire protocol, when the route translates protocols.
    /// Values: "openai_chat", "anthropic", "openai_responses".
    /// Must be set together with `target_protocol` (or neither).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_protocol: Option<String>,

    /// Upstream wire protocol, when the route translates protocols.
    /// Must be set together with `source_protocol` (or neither).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_protocol: Option<String>,

    /// When set, this route executes a compiled workflow instead of proxying
    /// to a lane. The `lane` field is ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LaneConfig {
    /// Unique lane identifier.
    pub id: String,

    /// Upstream base URL.
    pub base_url: String,

    /// Egress mode for this lane: `"direct"` (default; requests leave from
    /// the gateway IP) or `"masked"` (requests egress via `proxy_url`).
    /// `"masked"` without a `proxy_url` is a validation error — a masked
    /// lane must never silently fall back to direct egress.
    #[serde(default = "default_lane_egress")]
    pub egress: String,

    /// Proxy URL for masked egress: `http://host:port` (HTTP CONNECT) or
    /// `socks5://host:port` (SOCKS5). Must be set when `egress == "masked"`.
    #[serde(default)]
    pub proxy_url: Option<String>,

    /// Timeout for establishing a TCP connection to the upstream.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,

    /// Maximum idle time before an upstream connection is closed.
    #[serde(default = "default_idle_timeout_ms")]
    pub idle_timeout_ms: u64,

    /// Maximum time between body frames (for streaming detection).
    #[serde(default = "default_frame_timeout_ms")]
    pub frame_timeout_ms: u64,

    /// Maximum concurrent requests to this lane.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Maximum idle connections held in the pool.
    #[serde(default = "default_max_idle")]
    pub max_idle: usize,
}

/// Serde default for `LaneConfig::egress`: a TOML lane without an explicit
/// egress value is direct (gateway IP, no proxy).
fn default_lane_egress() -> String {
    "direct".into()
}

// ─── Defaults ───────────────────────────────────────────────────────────────

fn default_snapshot_version() -> u64 {
    1
}
fn default_listen_addr() -> SocketAddr {
    ([0, 0, 0, 0], 8080).into()
}
fn default_admin_listen() -> SocketAddr {
    ([127, 0, 0, 1], 9090).into()
}
fn default_total_timeout_ms() -> u64 {
    120_000
}
fn default_shutdown_timeout_ms() -> u64 {
    5_000
}
fn default_methods() -> Vec<String> {
    vec!["POST".into()]
}
fn default_connect_timeout_ms() -> u64 {
    5_000
}
fn default_idle_timeout_ms() -> u64 {
    90_000
}
fn default_frame_timeout_ms() -> u64 {
    60_000
}
fn default_max_concurrent() -> usize {
    128
}
fn default_max_idle() -> usize {
    64
}

// ─── Parsed / compiled types (hot-path immutable snapshot) ──────────────────

/// A pre-parsed route entry for fast prefix matching.
#[derive(Debug, Clone)]
pub struct CompiledRoute {
    pub id: String,
    pub path_prefix: String,
    pub methods: Vec<http::Method>,
    pub lane_id: String,
    /// When set, this route translates between `source_protocol` (client side)
    /// and `target_protocol` (upstream side). Absence means pure passthrough.
    pub source_protocol: Option<protocol_core::canonical::Protocol>,
    pub target_protocol: Option<protocol_core::canonical::Protocol>,
    /// When set, this route executes a compiled workflow instead of proxying.
    pub workflow_id: Option<String>,
}

/// A fully parsed lane with pre-cased headers ready for forwarding.
#[derive(Debug, Clone)]
pub struct CompiledLane {
    pub id: String,
    pub base_url: url::Url,
    pub egress: String,
    pub proxy_url: Option<String>,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub frame_timeout: Duration,
    pub max_concurrent: usize,
    pub max_idle: usize,
}

/// Immutable snapshot of the gateway configuration.
/// Built once at startup and never mutated.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    version: u64,
    routes: Vec<CompiledRoute>,
    lanes: HashMap<String, Arc<CompiledLane>>,
}

impl ConfigSnapshot {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn routes(&self) -> &[CompiledRoute] {
        &self.routes
    }

    pub fn lookup_lane(&self, lane_id: &str) -> Option<&Arc<CompiledLane>> {
        self.lanes.get(lane_id)
    }

    /// Find the first matching route for a given method and path.
    ///
    /// The lane is `None` for workflow routes (which execute a compiled plan
    /// instead of proxying to a lane).
    pub fn match_route(
        &self,
        method: &http::Method,
        path: &str,
    ) -> Option<(&CompiledRoute, Option<&Arc<CompiledLane>>)> {
        self.routes.iter().find_map(|route| {
            if route.methods.iter().any(|m| m == method) && path.starts_with(&route.path_prefix) {
                if route.workflow_id.is_some() {
                    Some((route, None))
                } else {
                    let lane = self.lanes.get(&route.lane_id)?;
                    Some((route, Some(lane)))
                }
            } else {
                None
            }
        })
    }
}

impl GatewayConfig {
    /// Load and parse the gateway config from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: GatewayConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Parse the gateway config from a TOML string.
    /// Used by tests and benchmarks to build configs inline.
    pub fn from_toml_str(content: &str) -> Result<Self, ConfigError> {
        let config: GatewayConfig = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    /// Compile the raw config into a hot-path snapshot.
    pub fn compile(&self) -> Result<ConfigSnapshot, ConfigError> {
        let mut lanes = HashMap::new();
        for lane_cfg in &self.lanes {
            let base_url = url::Url::parse(&lane_cfg.base_url).map_err(|e| {
                ConfigError::Validation(format!(
                    "lane '{}': invalid base_url '{}': {}",
                    lane_cfg.id, lane_cfg.base_url, e
                ))
            })?;

            // Fail-closed egress: a lane configured `masked` without a proxy
            // URL (or with an unknown egress value) must reject the config —
            // never silently become a direct lane and leak the gateway IP.
            match lane_cfg.egress.as_str() {
                "direct" => {}
                "masked" => {
                    if lane_cfg.proxy_url.is_none() {
                        return Err(ConfigError::Validation(format!(
                            "lane '{}': egress=masked requires a proxy_url (http://… or socks5://…)",
                            lane_cfg.id
                        )));
                    }
                }
                other => {
                    return Err(ConfigError::Validation(format!(
                        "lane '{}': unknown egress '{other}' (expected 'direct' or 'masked')",
                        lane_cfg.id
                    )));
                }
            }

            lanes.insert(
                lane_cfg.id.clone(),
                Arc::new(CompiledLane {
                    id: lane_cfg.id.clone(),
                    base_url,
                    egress: lane_cfg.egress.clone(),
                    proxy_url: lane_cfg.proxy_url.clone(),
                    connect_timeout: Duration::from_millis(lane_cfg.connect_timeout_ms),
                    idle_timeout: Duration::from_millis(lane_cfg.idle_timeout_ms),
                    frame_timeout: Duration::from_millis(lane_cfg.frame_timeout_ms),
                    max_concurrent: lane_cfg.max_concurrent,
                    max_idle: lane_cfg.max_idle,
                }),
            );
        }

        let mut routes = Vec::new();
        for route_cfg in &self.routes {
            // A workflow route doesn't use a lane; skip the exists check.
            if route_cfg.workflow_id.is_none() && !lanes.contains_key(&route_cfg.lane) {
                return Err(ConfigError::Validation(format!(
                    "route '{}': references unknown lane '{}'",
                    route_cfg.id, route_cfg.lane
                )));
            }

            let methods: Vec<http::Method> = route_cfg
                .methods
                .iter()
                .map(|m| {
                    m.parse::<http::Method>().map_err(|_| {
                        ConfigError::Validation(format!(
                            "route '{}': invalid HTTP method '{}'",
                            route_cfg.id, m
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            routes.push(CompiledRoute {
                id: route_cfg.id.clone(),
                path_prefix: route_cfg.path_prefix.clone(),
                methods,
                lane_id: route_cfg.lane.clone(),
                source_protocol: route_cfg
                    .source_protocol
                    .as_deref()
                    .map(parse_protocol)
                    .transpose()?,
                target_protocol: route_cfg
                    .target_protocol
                    .as_deref()
                    .map(parse_protocol)
                    .transpose()?,
                workflow_id: route_cfg.workflow_id.clone(),
            });
        }

        Ok(ConfigSnapshot {
            version: self.snapshot_version,
            routes,
            lanes,
        })
    }

    pub fn snapshot_version(&self) -> u64 {
        self.snapshot_version
    }

    pub fn routes(&self) -> &[RouteConfig] {
        &self.routes
    }

    pub fn lanes(&self) -> &[LaneConfig] {
        &self.lanes
    }

    pub fn server(&self) -> &ServerConfig {
        &self.server
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.routes.is_empty() {
            return Err(ConfigError::Validation(
                "at least one route must be defined".into(),
            ));
        }
        if self.lanes.is_empty() && self.routes.iter().any(|r| r.workflow_id.is_none()) {
            return Err(ConfigError::Validation(
                "at least one lane must be defined".into(),
            ));
        }
        // Check for duplicate route IDs and validate path_prefix.
        let mut seen = std::collections::HashSet::new();
        for route in &self.routes {
            if !seen.insert(&route.id) {
                return Err(ConfigError::Validation(format!(
                    "duplicate route id: '{}'",
                    route.id
                )));
            }
            if route.path_prefix.is_empty() {
                return Err(ConfigError::Validation(format!(
                    "route '{}': path_prefix must not be empty",
                    route.id
                )));
            }
            if !route.path_prefix.starts_with('/') {
                return Err(ConfigError::Validation(format!(
                    "route '{}': path_prefix must start with '/'",
                    route.id
                )));
            }
            // Protocol translation fields must be set together.
            match (&route.source_protocol, &route.target_protocol) {
                (Some(_), None) => {
                    return Err(ConfigError::Validation(format!(
                        "route '{}': source_protocol set without target_protocol",
                        route.id
                    )));
                }
                (None, Some(_)) => {
                    return Err(ConfigError::Validation(format!(
                        "route '{}': target_protocol set without source_protocol",
                        route.id
                    )));
                }
                (Some(src), Some(tgt)) => {
                    // Validate both names parse.
                    parse_protocol(src)?;
                    parse_protocol(tgt)?;
                }
                (None, None) => {}
            }
        }
        // Check for duplicate lane IDs.
        let mut seen = std::collections::HashSet::new();
        for lane in &self.lanes {
            if !seen.insert(&lane.id) {
                return Err(ConfigError::Validation(format!(
                    "duplicate lane id: '{}'",
                    lane.id
                )));
            }
        }
        // Validate lane URL parses.
        for lane in &self.lanes {
            url::Url::parse(&lane.base_url).map_err(|e| {
                ConfigError::Validation(format!(
                    "lane '{}': invalid base_url '{}': {}",
                    lane.id, lane.base_url, e
                ))
            })?;
        }
        Ok(())
    }
}

use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() -> anyhow::Result<()> {
        let toml_str = r#"
snapshot_version = 1

[server]
listen = "127.0.0.1:8080"
admin_listen = "127.0.0.1:9090"
total_timeout_ms = 30000
graceful_shutdown_ms = 5000

[[routes]]
id = "test-route"
path_prefix = "/v1/test"
methods = ["POST"]
lane = "test-lane"

[[lanes]]
id = "test-lane"
base_url = "http://127.0.0.1:9000"
connect_timeout_ms = 2000
idle_timeout_ms = 30000
frame_timeout_ms = 10000
max_concurrent = 64
max_idle = 32
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        assert_eq!(config.snapshot_version, 1);
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.lanes.len(), 1);

        // Compile and verify.
        let snapshot = config.compile()?;
        assert_eq!(snapshot.version(), 1);
        Ok(())
    }

    #[test]
    fn test_validation_rejects_missing_routes() -> anyhow::Result<()> {
        // An explicitly empty `routes` list parses successfully; validation is
        // what rejects it (a config may not deploy with zero routes).
        let toml_str = r#"
routes = []

[server]
listen = "127.0.0.1:8080"

[[lanes]]
id = "lane-1"
base_url = "http://127.0.0.1:9000"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("at least one route"));
        Ok(())
    }

    #[test]
    fn test_validation_rejects_unknown_lane() -> anyhow::Result<()> {
        let toml_str = r#"
[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "r1"
path_prefix = "/v1/"
lane = "nonexistent"

[[lanes]]
id = "lane-1"
base_url = "http://127.0.0.1:9000"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let snapshot = config.compile();
        assert!(snapshot.is_err());
        Ok(())
    }

    #[test]
    fn test_route_matching() -> anyhow::Result<()> {
        let toml_str = r#"
snapshot_version = 2

[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "anthropic"
path_prefix = "/v1/messages"
methods = ["POST"]
lane = "anthropic"

[[routes]]
id = "openai"
path_prefix = "/v1/chat/completions"
methods = ["POST"]
lane = "openai"

[[lanes]]
id = "anthropic"
base_url = "http://127.0.0.1:8101"

[[lanes]]
id = "openai"
base_url = "http://127.0.0.1:8102"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let snapshot = config.compile()?;

        // Match Anthropic
        let (route, lane) = snapshot
            .match_route(&http::Method::POST, "/v1/messages")
            .ok_or_else(|| anyhow::anyhow!("expected a route match for /v1/messages"))?;
        assert_eq!(route.id, "anthropic");
        let lane = lane.ok_or_else(|| anyhow::anyhow!("expected a lane for proxy route"))?;
        assert_eq!(lane.id, "anthropic");

        // Match OpenAI
        let (route, lane) = snapshot
            .match_route(&http::Method::POST, "/v1/chat/completions")
            .ok_or_else(|| anyhow::anyhow!("expected a route match for /v1/chat/completions"))?;
        assert_eq!(route.id, "openai");
        let lane = lane.ok_or_else(|| anyhow::anyhow!("expected a lane for proxy route"))?;
        assert_eq!(lane.id, "openai");

        // No match on GET
        assert!(
            snapshot
                .match_route(&http::Method::GET, "/v1/messages")
                .is_none()
        );

        // No match on unknown path
        assert!(
            snapshot
                .match_route(&http::Method::POST, "/v1/unknown")
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn masked_lane_without_proxy_is_rejected() -> anyhow::Result<()> {
        let toml_str = r#"
[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "r1"
path_prefix = "/v1/"
lane = "masked-lane"

[[lanes]]
id = "masked-lane"
base_url = "https://api.example.com/v1"
egress = "masked"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let err = config.compile().unwrap_err();
        assert!(
            err.to_string()
                .contains("egress=masked requires a proxy_url"),
            "masked lane without proxy must be rejected, got: {err}"
        );
        Ok(())
    }

    #[test]
    fn unknown_lane_egress_is_rejected() -> anyhow::Result<()> {
        let toml_str = r#"
[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "r1"
path_prefix = "/v1/"
lane = "weird-lane"

[[lanes]]
id = "weird-lane"
base_url = "https://api.example.com/v1"
egress = "some_future_value"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let err = config.compile().unwrap_err();
        assert!(
            err.to_string().contains("unknown egress"),
            "unknown egress must be rejected, got: {err}"
        );
        Ok(())
    }

    #[test]
    fn masked_lane_with_proxy_compiles() -> anyhow::Result<()> {
        let toml_str = r#"
[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "r1"
path_prefix = "/v1/"
lane = "masked-lane"

[[lanes]]
id = "masked-lane"
base_url = "https://api.example.com/v1"
egress = "masked"
proxy_url = "socks5://proxy.example.com:1080"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let snapshot = config.compile()?;
        let lane = snapshot
            .lookup_lane("masked-lane")
            .ok_or_else(|| anyhow::anyhow!("masked-lane missing"))?;
        assert_eq!(lane.egress, "masked");
        assert_eq!(
            lane.proxy_url.as_deref(),
            Some("socks5://proxy.example.com:1080")
        );
        Ok(())
    }

    #[test]
    fn lane_egress_defaults_to_direct() -> anyhow::Result<()> {
        let toml_str = r#"
[server]
listen = "127.0.0.1:8080"

[[routes]]
id = "r1"
path_prefix = "/v1/"
lane = "plain-lane"

[[lanes]]
id = "plain-lane"
base_url = "https://api.example.com/v1"
"#;
        let config: GatewayConfig = toml::from_str(toml_str)?;
        let snapshot = config.compile()?;
        let lane = snapshot
            .lookup_lane("plain-lane")
            .ok_or_else(|| anyhow::anyhow!("plain-lane missing"))?;
        assert_eq!(lane.egress, "direct");
        assert!(lane.proxy_url.is_none());
        Ok(())
    }
}
