//! Per-lane connection-pool snapshots.
//!
//! Each lane owns its own Hyper client (connection pool). Lane identity is
//! part of upstream resource identity: connections established through
//! incompatible lanes are never reused across lanes, even when two lanes share
//! an authority.
//!
//! Hot-swap: on each published `RuntimeSnapshot`, the gateway rebuilds the
//! per-lane pools from that snapshot's lane registry. Builds happen off the
//! request hot path (control-plane cadence).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use workflow_runtime::context::{LaneClient, LaneEntry};

/// Connection-pool factory — the only place Hyper clients are built.
pub trait PoolBuilder: Send + Sync {
    /// Build the client (pool) for a lane. The pool key is the lane identity.
    ///
    /// Returns an error when the lane's egress configuration cannot produce a
    /// usable client (masked without a proxy, invalid proxy URL, unknown
    /// egress). Builds happen at publish cadence, off the request hot path,
    /// and pool-build failure must fail closed: a masked lane that cannot
    /// tunnel must never silently become a direct (gateway-IP) client.
    fn build(&self, lane: &LaneEntry) -> Result<Arc<LaneClient>, String>;
}

/// Standard builder: a Hyper legacy client with keep-alive pooling,
/// idle timeout, and a bounded idle pool size.
pub struct HyperPoolBuilder {
    connect_timeout: Duration,
    idle_timeout: Duration,
    max_idle: usize,
}

impl HyperPoolBuilder {
    /// Create a builder with the given timeouts and pool settings.
    pub fn new(connect_timeout: Duration, idle_timeout: Duration, max_idle: usize) -> Self {
        Self {
            connect_timeout,
            idle_timeout,
            max_idle: max_idle.max(1),
        }
    }
}

impl PoolBuilder for HyperPoolBuilder {
    fn build(&self, lane: &LaneEntry) -> Result<Arc<LaneClient>, String> {
        // The pool key is the lane identity itself — the builder settings are
        // global; per-lane differentiation lives in `LanePools` keying.
        //
        // Egress mode selects the connector family:
        //   "direct"                        → plain TCP via HttpConnector
        //   "masked" + http://proxy         → HTTP CONNECT tunnel
        //   "masked" + socks5://proxy       → SOCKS5 tunnel
        //   "masked" without proxy_url      → hard error (fail-closed egress)
        //   "masked" with invalid proxy_url → hard error (fail-closed egress)
        //   unknown values                  → hard error (fail-closed egress)
        //
        // Any error here is a configuration defect, not a runtime fallback:
        // a lane an operator expects to be proxied must never send traffic
        // direct from the gateway IP.
        match lane.egress.as_str() {
            "masked" => {
                let Some(proxy_url) = lane.proxy_url.as_deref() else {
                    return Err(format!(
                        "lane '{}': egress=masked requires a proxy_url",
                        lane.id
                    ));
                };
                LaneClient::from_lane(
                    "masked",
                    Some(proxy_url),
                    self.connect_timeout,
                    self.idle_timeout,
                    self.max_idle,
                )
                .map(Arc::new)
                .map_err(|e| format!("lane '{}': {e}", lane.id))
            }
            "direct" => Ok(Arc::new(LaneClient::direct(
                self.idle_timeout,
                self.max_idle,
            ))),
            other => Err(format!(
                "lane '{}': unknown egress '{other}' (expected 'direct' or 'masked')",
                lane.id
            )),
        }
    }
}

/// A lane's dedicated connection pool.
#[derive(Clone)]
pub struct LaneSnapshot {
    /// Compiled lane entry.
    pub lane: LaneEntry,
    /// The lane's dedicated client.
    pub client: Arc<LaneClient>,
}

/// The per-lane pools for one runtime snapshot, keyed by lane id.
#[derive(Default)]
pub struct LanePools {
    pools: HashMap<String, LaneSnapshot>,
}

impl Clone for LanePools {
    fn clone(&self) -> Self {
        Self {
            pools: self.pools.clone(),
        }
    }
}

impl LanePools {
    /// Build the per-lane pools from a runtime snapshot's lanes.
    ///
    /// Fails if any lane's egress configuration cannot produce a usable
    /// client (masked without proxy, invalid proxy URL, unknown egress).
    /// Pool build runs at publish cadence (off the hot path), and failing
    /// the whole snapshot here guarantees the runtime never serves a lane
    /// with a silently-degraded egress mode.
    pub fn build(
        snapshot: &workflow_runtime::RuntimeSnapshot,
        builder: &dyn PoolBuilder,
    ) -> Result<Self, String> {
        let mut pools = HashMap::new();
        for (id, lane) in snapshot.lanes().iter() {
            let client = builder.build(lane)?;
            pools.insert(
                id.clone(),
                LaneSnapshot {
                    lane: lane.clone(),
                    client,
                },
            );
        }
        Ok(Self { pools })
    }

    /// The pool for a lane id, if that lane is in the snapshot.
    pub fn get(&self, lane_id: &str) -> Option<&LaneSnapshot> {
        self.pools.get(lane_id)
    }

    /// Number of lane pools.
    pub fn len(&self) -> usize {
        self.pools.len()
    }

    /// Whether there are no lane pools.
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }
}

impl workflow_runtime::AsLaneClient for LanePools {
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<LaneClient>> {
        self.get(lane_id).map(|snapshot| snapshot.client.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;
    use workflow_runtime::RuntimeSnapshotBuilder;
    use workflow_runtime::context::LaneRegistry;

    fn lanes() -> Arc<LaneRegistry> {
        let mut registry = LaneRegistry::new();
        registry.register(LaneEntry {
            id: "lane-a".into(),
            base_url: match Url::parse("http://127.0.0.1:9001") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "direct".into(),
            proxy_url: None,
        });
        registry.register(LaneEntry {
            id: "lane-b".into(),
            base_url: match Url::parse("http://127.0.0.1:9002") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "direct".into(),
            proxy_url: None,
        });
        Arc::new(registry)
    }

    fn snapshot() -> Arc<workflow_runtime::RuntimeSnapshot> {
        Arc::new(RuntimeSnapshotBuilder::new(1).with_lanes(lanes()).build())
    }

    #[test]
    fn pools_are_keyed_by_lane_identity() {
        let snap = snapshot();
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let pools = match LanePools::build(&snap, &builder) {
            Ok(p) => p,
            Err(e) => panic!("direct lanes should build pools: {e}"),
        };

        assert_eq!(pools.len(), 2);
        let a = match pools.get("lane-a") {
            Some(s) => s,
            None => panic!("lane-a pool missing"),
        };
        let b = match pools.get("lane-b") {
            Some(s) => s,
            None => panic!("lane-b pool missing"),
        };

        // Distinct lane → distinct pool instance even with identical builder
        // settings: no cross-lane connection reuse is possible.
        assert!(!Arc::ptr_eq(&a.client, &b.client));
    }

    #[test]
    fn unknown_lane_has_no_pool() {
        let snap = snapshot();
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let pools = match LanePools::build(&snap, &builder) {
            Ok(p) => p,
            Err(e) => panic!("direct lanes should build pools: {e}"),
        };
        assert!(pools.get("lane-missing").is_none());
    }

    #[test]
    fn masked_egress_with_valid_proxy_builds_lane_client() {
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let lane = LaneEntry {
            id: "lane-masked".into(),
            base_url: match Url::parse("https://api.example.com/v1") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "masked".into(),
            proxy_url: Some("http://proxy.example.com:8080".into()),
        };
        let client = match builder.build(&lane) {
            Ok(c) => c,
            Err(e) => panic!("masked lane with a proxy should build: {e}"),
        };
        assert_eq!(client.egress(), "masked");
    }

    #[test]
    fn masked_egress_without_proxy_is_a_hard_error() {
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let lane = LaneEntry {
            id: "lane-masked-noproxy".into(),
            base_url: match Url::parse("https://api.example.com/v1") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "masked".into(),
            proxy_url: None,
        };
        let err = match builder.build(&lane) {
            Ok(c) => panic!(
                "masked without proxy must fail closed, got client: {}",
                c.egress()
            ),
            Err(e) => e,
        };
        assert!(
            err.contains("requires a proxy_url"),
            "error should name the missing proxy, got: {err}"
        );
    }

    #[test]
    fn masked_egress_with_invalid_proxy_is_a_hard_error() {
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let lane = LaneEntry {
            id: "lane-masked-badproxy".into(),
            base_url: match Url::parse("https://api.example.com/v1") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "masked".into(),
            proxy_url: Some("http://exa mple.com:8080".into()),
        };
        if builder.build(&lane).is_ok() {
            panic!("invalid proxy URL must fail closed");
        }
    }

    #[test]
    fn unknown_egress_is_a_hard_error() {
        let builder = HyperPoolBuilder::new(Duration::from_secs(5), Duration::from_secs(90), 16);
        let lane = LaneEntry {
            id: "lane-future".into(),
            base_url: match Url::parse("https://api.example.com/v1") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
            egress: "some_future_value".into(),
            proxy_url: None,
        };
        let err = match builder.build(&lane) {
            Ok(c) => panic!(
                "unknown egress must fail closed, got client: {}",
                c.egress()
            ),
            Err(e) => e,
        };
        assert!(
            err.contains("unknown egress"),
            "error should name the egress, got: {err}"
        );
    }
}
