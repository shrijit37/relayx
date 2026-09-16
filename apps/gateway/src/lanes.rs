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
    fn build(&self, lane: &LaneEntry) -> Arc<LaneClient>;
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
    fn build(&self, lane: &LaneEntry) -> Arc<LaneClient> {
        // The pool key is the lane identity itself — the builder settings are
        // global; per-lane differentiation lives in `LanePools` keying.
        //
        // Egress mode selects the connector family:
        //   "direct"                        → plain TCP via HttpConnector
        //   "masked" + http://proxy         → HTTP CONNECT tunnel
        //   "masked" + socks5://proxy       → SOCKS5 tunnel
        //   "masked" without proxy_url      → degrade to direct (logged warning)
        //   unknown values                  → direct (fail-open, per requirement)
        match lane.egress.as_str() {
            "masked" => {
                let Some(proxy_url) = lane.proxy_url.as_deref() else {
                    tracing::warn!(
                        lane = %lane.id,
                        "lane egress=masked but proxy_url is missing; falling back to direct"
                    );
                    return Arc::new(LaneClient::direct(self.idle_timeout, self.max_idle));
                };
                match LaneClient::from_lane(
                    "masked",
                    Some(proxy_url),
                    self.connect_timeout,
                    self.idle_timeout,
                    self.max_idle,
                ) {
                    Ok(client) => Arc::new(client),
                    Err(e) => {
                        tracing::warn!(
                            lane = %lane.id,
                            proxy_url = %proxy_url,
                            error = %e,
                            "invalid proxy_url for masked egress; falling back to direct"
                        );
                        Arc::new(LaneClient::direct(self.idle_timeout, self.max_idle))
                    }
                }
            }
            _ => Arc::new(LaneClient::direct(self.idle_timeout, self.max_idle)),
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
    pub fn build(snapshot: &workflow_runtime::RuntimeSnapshot, builder: &dyn PoolBuilder) -> Self {
        let mut pools = HashMap::new();
        for (id, lane) in snapshot.lanes().iter() {
            let client = builder.build(lane);
            pools.insert(
                id.clone(),
                LaneSnapshot {
                    lane: lane.clone(),
                    client,
                },
            );
        }
        Self { pools }
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
        let pools = LanePools::build(&snap, &builder);

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
        let pools = LanePools::build(&snap, &builder);
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
        let client = builder.build(&lane);
        assert_eq!(client.egress(), "masked");
    }

    #[test]
    fn masked_egress_without_proxy_degrades_to_direct() {
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
        let client = builder.build(&lane);
        assert_eq!(client.egress(), "direct");
    }

    #[test]
    fn masked_egress_with_invalid_proxy_degrades_to_direct() {
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
        let client = builder.build(&lane);
        assert_eq!(client.egress(), "direct");
    }

    #[test]
    fn unknown_egress_builds_direct_client() {
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
        let client = builder.build(&lane);
        assert_eq!(client.egress(), "direct");
    }
}
