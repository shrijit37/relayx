//! Snapshot publication — the control-plane → data-plane seam.
//!
//! A `SnapshotPublisher` accepts an immutable [`RuntimeSnapshot`] and makes
//! it visible to data-plane workers. The only in-memory implementation is
//! [`InMemoryPublisher`], which stores the current snapshot in an `ArcSwap`
//! so publication is lock-free and readers observe one coherent snapshot.
//!
//! Contract: a snapshot is never mutated after publication. A new version
//! replaces the old via atomic swap; a request that acquired a snapshot keeps
//! it for the duration of its execution even if a newer version lands.

use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::snapshot::RuntimeSnapshot;

/// Snapshot publication abstraction: accepts a new immutable snapshot.
///
/// Implementations control where the snapshot lives (currently in-process;
/// a future control plane could push snapshots over the network).
pub trait SnapshotPublisher: Send + Sync {
    /// Publish a new snapshot, atomically replacing the previous one.
    fn publish(&self, snapshot: Arc<RuntimeSnapshot>);
}

/// Reads the active snapshot on the data plane.
pub trait SnapshotReader: Send + Sync {
    /// The current snapshot, if one has been published.
    fn snapshot(&self) -> Option<Arc<RuntimeSnapshot>>;
}

/// In-memory snapshot store with lock-free atomic hot-swap.
pub struct InMemoryPublisher {
    current: ArcSwap<Option<Arc<RuntimeSnapshot>>>,
}

impl Default for InMemoryPublisher {
    fn default() -> Self {
        Self {
            current: ArcSwap::from(Arc::new(None)),
        }
    }
}

impl InMemoryPublisher {
    /// Create an empty publisher with no snapshot yet.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SnapshotPublisher for InMemoryPublisher {
    fn publish(&self, snapshot: Arc<RuntimeSnapshot>) {
        // Atomic swap: readers already holding the previous `Arc` keep it.
        self.current.store(Arc::new(Some(snapshot)));
    }
}

impl SnapshotReader for InMemoryPublisher {
    fn snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        // `load_full` clones the inner `Arc` — lock-free, no waiters.
        self.current.load_full().as_ref().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ExecutionPlan;
    use workflow_schema::*;

    fn input_node() -> Node {
        Node {
            id: "in".into(),
            kind: NodeKind::Input,
            config: NodeConfig::Input(InputConfig::default()),
            inputs: vec![],
            outputs: vec![PortDef {
                name: "out".into(),
                port_type: PortType::Message,
            }],
        }
    }

    fn output_node() -> Node {
        Node {
            id: "out".into(),
            kind: NodeKind::Output,
            config: NodeConfig::Output(OutputConfig::default()),
            inputs: vec![PortDef {
                name: "in".into(),
                port_type: PortType::Message,
            }],
            outputs: vec![],
        }
    }

    fn simple_workflow(id: &str) -> Workflow {
        Workflow {
            id: id.into(),
            name: id.into(),
            version: 1,
            nodes: vec![input_node(), output_node()],
            edges: vec![Edge {
                source_node: "in".into(),
                source_port: "out".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            }],
        }
    }

    fn plan(workflow_id: &str, version: u64) -> ExecutionPlan {
        let mut wf = simple_workflow(workflow_id);
        wf.version = version;
        match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("plan compile failed: {e}"),
        }
    }

    #[test]
    fn reader_is_none_before_first_publish() {
        let p = InMemoryPublisher::new();
        assert!(p.snapshot().is_none());
    }

    #[test]
    fn publish_then_read() {
        let p = InMemoryPublisher::new();

        let v1 = Arc::new(
            crate::snapshot::RuntimeSnapshotBuilder::new(1)
                .with_plan("wf", plan("wf", 1))
                .build(),
        );
        p.publish(v1.clone());

        let read = match p.snapshot() {
            Some(s) => s,
            None => panic!("snapshot published"),
        };
        assert_eq!(read.version(), 1);
        assert!(read.get_plan("wf").is_some());
    }

    #[test]
    fn publish_replaces_snapshot() {
        let p = InMemoryPublisher::new();

        let v1 = Arc::new(
            crate::snapshot::RuntimeSnapshotBuilder::new(1)
                .with_plan("wf", plan("wf", 1))
                .build(),
        );
        p.publish(v1);

        let v2 = Arc::new(
            crate::snapshot::RuntimeSnapshotBuilder::new(2)
                .with_plan("wf", plan("wf", 2))
                .build(),
        );
        p.publish(v2);

        let read = match p.snapshot() {
            Some(s) => s,
            None => panic!("snapshot published"),
        };
        assert_eq!(read.version(), 2);
        assert!(read.get_plan("wf").is_some());
    }
}
