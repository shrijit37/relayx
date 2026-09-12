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

use workflow_runtime::context::{GatewayHttpClient, LaneEntry};

/// Connection-pool factory — the only place Hyper clients are built.
pub trait PoolBuilder: Send + Sync {
    /// Build the client (pool) for a lane. The pool key is the lane identity.
    fn build(&self, lane: &LaneEntry) -> Arc<GatewayHttpClient>;
}

/// Standard builder: a Hyper legacy client with keep-alive pooling,
/// idle timeout, and a bounded idle pool size.
pub struct HyperPoolBuilder {
    idle_timeout: Duration,
    max_idle: usize,
}

impl HyperPoolBuilder {
    /// Create a builder with the given idle timeout and max idle connections.
    pub fn new(idle_timeout: Duration, max_idle: usize) -> Self {
        Self {
            idle_timeout,
            max_idle: max_idle.max(1),
        }
    }
}

impl PoolBuilder for HyperPoolBuilder {
    fn build(&self, _lane: &LaneEntry) -> Arc<GatewayHttpClient> {
        // The pool key is the lane identity itself — the builder settings are
        // global; per-lane differentiation lives in `LanePools` keying.
        Arc::new(
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .pool_idle_timeout(self.idle_timeout)
                .pool_max_idle_per_host(self.max_idle)
                .retry_canceled_requests(false)
                .build(hyper_util::client::legacy::connect::HttpConnector::new()),
        )
    }
}

/// A lane's dedicated connection pool.
#[derive(Clone)]
pub struct LaneSnapshot {
    /// Compiled lane entry.
    pub lane: LaneEntry,
    /// The lane's dedicated client.
    pub client: Arc<GatewayHttpClient>,
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
        });
        registry.register(LaneEntry {
            id: "lane-b".into(),
            base_url: match Url::parse("http://127.0.0.1:9002") {
                Ok(u) => u,
                Err(e) => panic!("invalid lane url: {e}"),
            },
            authorization: None,
        });
        Arc::new(registry)
    }

    fn snapshot() -> Arc<workflow_runtime::RuntimeSnapshot> {
        Arc::new(RuntimeSnapshotBuilder::new(1).with_lanes(lanes()).build())
    }

    #[test]
    fn pools_are_keyed_by_lane_identity() {
        let snap = snapshot();
        let builder = HyperPoolBuilder::new(Duration::from_secs(90), 16);
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
        let builder = HyperPoolBuilder::new(Duration::from_secs(90), 16);
        let pools = LanePools::build(&snap, &builder);
        assert!(pools.get("lane-missing").is_none());
    }
}
