//! Observability metadata plumbing for execution contexts.
//!
//! `MilestoneReporter` carries execution metadata out of the runtime without
//! baking a cyclic runtime→gateway dependency: the gateway implements an
//! extension trait on its `AppState.lane_snapshots`, and the runtime stores
//! it as a type-erased `Arc<dyn MilestoneReporter>`, defaulting to a no-op
//! when no reporter is attached.

/// Receives execution milestones for a node run.
pub trait MilestoneReporter: Send + Sync {
    /// A node completed with an output value and its port name.
    fn node_completed(&self, node_id: &str, output_port: Option<&str>);
    /// A node failed.
    fn node_failed(&self, node_id: &str, error: &str);
}

/// No-op reporter — used when no observing gateway is attached.
pub struct NoopReporter;

impl MilestoneReporter for NoopReporter {
    fn node_completed(&self, _node_id: &str, _output_port: Option<&str>) {}
    fn node_failed(&self, _node_id: &str, _error: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn noop_reporter_is_a_noop() {
        let r = NoopReporter;
        r.node_completed("llm", Some("out"));
        r.node_failed("llm", "boom");
    }

    #[test]
    fn dyn_reporter_dispatch() {
        struct Count(AtomicUsize);
        impl MilestoneReporter for Count {
            fn node_completed(&self, _id: &str, _p: Option<&str>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            fn node_failed(&self, _id: &str, _e: &str) {}
        }

        let reporter = Arc::new(Count(AtomicUsize::new(0)));
        let r: Arc<dyn MilestoneReporter> = reporter.clone();
        r.node_completed("a", None);
        r.node_completed("b", None);
        assert_eq!(reporter.0.load(Ordering::Relaxed), 2);
    }
}
