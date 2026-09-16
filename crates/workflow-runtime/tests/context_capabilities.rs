//! ExecutionContext capability plumbing tests.
//!
//! Covers the Phase-5 contract: an execution context can surface snapshot
//! metadata, lane-aware connection pools, and execution metadata without any
//! of it becoming mutable global state. These are the seams MCP/Skills and
//! per-lane pools attach through later — no scheduler changes required.

use std::sync::Arc;

use workflow_runtime::RuntimeSnapshotBuilder;
use workflow_runtime::context::{
    AsLaneClient, ExecutionContext, LaneClient, LaneEntry, LaneRegistry,
};
use workflow_runtime::milestone::MilestoneReporter;
use workflow_schema::*;

fn lane(id: &str, url: &str) -> LaneEntry {
    LaneEntry {
        id: id.into(),
        base_url: match url::Url::parse(url) {
            Ok(u) => u,
            Err(e) => panic!("invalid lane url: {e}"),
        },
        authorization: None,
        egress: "direct".into(),
        proxy_url: None,
    }
}

fn snapshot_with_lanes() -> Arc<workflow_runtime::RuntimeSnapshot> {
    let mut lanes = LaneRegistry::new();
    lanes.register(lane("lane-a", "http://127.0.0.1:9001"));

    // A passthrough workflow so the plan compiles.
    let wf = Workflow {
        id: "wf".into(),
        name: "wf".into(),
        version: 1,
        nodes: vec![
            Node {
                id: "in".into(),
                kind: NodeKind::Input,
                config: NodeConfig::Input(InputConfig::default()),
                inputs: vec![],
                outputs: vec![PortDef {
                    name: "out".into(),
                    port_type: PortType::Message,
                }],
            },
            Node {
                id: "out".into(),
                kind: NodeKind::Output,
                config: NodeConfig::Output(OutputConfig::default()),
                inputs: vec![PortDef {
                    name: "in".into(),
                    port_type: PortType::Message,
                }],
                outputs: vec![],
            },
        ],
        edges: vec![Edge {
            source_node: "in".into(),
            source_port: "out".into(),
            target_node: "out".into(),
            target_port: "in".into(),
            condition: None,
        }],
    };
    let plan = match workflow_runtime::compile_workflow_with_lanes(
        &wf,
        &[("lane-a".into(), "http://127.0.0.1:9001".into())],
        None,
    ) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };
    Arc::new(
        RuntimeSnapshotBuilder::new(41)
            .with_lanes(Arc::new(lanes))
            .with_plan("wf", plan)
            .build(),
    )
}

/// A lane-client resolver that hands back a stub client for one lane.
#[derive(Clone)]
struct StubLaneClients(Arc<LaneClient>);

impl AsLaneClient for StubLaneClients {
    fn client_for_lane(&self, lane_id: &str) -> Option<Arc<LaneClient>> {
        if lane_id == "lane-a" {
            Some(self.0.clone())
        } else {
            None
        }
    }
}

fn stub_client() -> Arc<LaneClient> {
    Arc::new(LaneClient::from_shared(Arc::new(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(hyper_util::client::legacy::connect::HttpConnector::new()),
    )))
}

#[test]
fn execution_context_carries_snapshot_identity() {
    let snap = snapshot_with_lanes();
    let lanes = snap.lanes_arc();
    let mut ctx = ExecutionContext::new("wf".into(), "run-1".into(), lanes);

    ctx.snapshot = Some(snap.clone());
    ctx.metadata = workflow_runtime::ExecutionMetadata::from_snapshot(&snap, "wf");

    assert_eq!(ctx.metadata.snapshot_version, 41);
    assert!(
        !ctx.metadata.plan_hash.is_empty(),
        "plan hash should be populated"
    );
    assert_eq!(ctx.snapshot.as_ref().map(|s| s.version()), Some(41));
    assert_eq!(ctx.run_id, "run-1");
}

#[test]
fn execution_metadata_from_snapshot_missing_plan_produces_empty_hash() {
    // A snapshot with no plan for 'missing' yields an empty plan hash, not a
    // panic — the default metadata path stays usable.
    let snap = Arc::new(RuntimeSnapshotBuilder::new(7).build());
    let meta = workflow_runtime::ExecutionMetadata::from_snapshot(&snap, "nope");
    assert_eq!(meta.snapshot_version, 7);
    assert_eq!(meta.plan_hash, "");
}

#[test]
fn lane_clients_resolve_per_lane() {
    let snap = snapshot_with_lanes();
    let lanes = snap.lanes_arc();
    let mut ctx = ExecutionContext::new("wf".into(), "run-2".into(), lanes);

    let client = stub_client();
    ctx.lane_clients = Some(Arc::new(StubLaneClients(client.clone())));

    let resolved = ctx
        .lane_clients
        .as_ref()
        .and_then(|lc| lc.client_for_lane("lane-a"));
    assert!(resolved.is_some(), "lane-a should resolve a client");
    let missing = ctx
        .lane_clients
        .as_ref()
        .and_then(|lc| lc.client_for_lane("no-lane"));
    assert!(missing.is_none(), "unknown lane should not resolve");
}

#[test]
fn for_node_preserves_capability_context() {
    let snap = snapshot_with_lanes();
    let lanes = snap.lanes_arc();
    let mut ctx = ExecutionContext::new("wf".into(), "run-3".into(), lanes);
    ctx.snapshot = Some(snap.clone());
    ctx.metadata = workflow_runtime::ExecutionMetadata::from_snapshot(&snap, "wf");

    let child = ctx.for_node("some-node");
    assert_eq!(child.node_id, "some-node");
    assert_eq!(child.metadata.snapshot_version, 41);
    assert_eq!(child.snapshot.as_ref().map(|s| s.version()), Some(41));
    // Reporter is shared (child inherits the same Arc).
    assert!(Arc::ptr_eq(&child.reporter, &ctx.reporter));
}

#[test]
fn default_reporter_is_noop() {
    let snap = snapshot_with_lanes();
    let lanes = snap.lanes_arc();
    let ctx = ExecutionContext::new("wf".into(), "run-4".into(), lanes);
    // No-op by default: does nothing, never panics.
    ctx.reporter.node_completed("llm", Some("out"));
    ctx.reporter.node_failed("llm", "boom");
}

#[test]
fn custom_reporter_receives_milestones() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Count(AtomicUsize);
    impl MilestoneReporter for Count {
        fn node_completed(&self, _id: &str, _p: Option<&str>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn node_failed(&self, _id: &str, _e: &str) {}
    }

    let snap = snapshot_with_lanes();
    let lanes = snap.lanes_arc();
    let mut ctx = ExecutionContext::new("wf".into(), "run-5".into(), lanes);
    let counter = Arc::new(Count(AtomicUsize::new(0)));
    ctx.reporter = counter.clone();

    ctx.reporter.node_completed("n1", None);
    ctx.reporter.node_completed("n2", Some("out"));
    assert_eq!(counter.0.load(Ordering::Relaxed), 2);
}
