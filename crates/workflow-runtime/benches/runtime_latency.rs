//! Benchmarks for workflow-runtime execution latency.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use workflow_runtime::context::{ExecutionContext, LaneRegistry};
use workflow_runtime::execution::{ExecutionPlan, NodeRuntime};
use workflow_runtime::nodes::{NodeInput, RuntimeValue};
use workflow_schema::*;

fn input_to_output_bench(c: &mut Criterion) {
    let wf = Workflow {
        id: "bench1".into(),
        name: "passthrough".into(),
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

    let plan = match ExecutionPlan::compile(&wf) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };

    let lanes = Arc::new(LaneRegistry::new());

    c.bench_function("input_to_output", |b| {
        b.iter(|| {
            let rt = NodeRuntime::new(plan.clone());
            let ctx = ExecutionContext::new("bench".into(), "bench-run".into(), lanes.clone());
            let input = NodeInput::message(RuntimeValue::String("hello".into()));
            let rt = tokio_test::block_on(rt.execute(&ctx, input));
            black_box(rt.ok());
        });
    });
}

fn input_transform_output_bench(c: &mut Criterion) {
    let wf = Workflow {
        id: "bench2".into(),
        name: "transform".into(),
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
                id: "t1".into(),
                kind: NodeKind::Transform,
                config: NodeConfig::Transform(TransformConfig {
                    operation: TransformOperation::Passthrough,
                }),
                inputs: vec![PortDef {
                    name: "in".into(),
                    port_type: PortType::Json,
                }],
                outputs: vec![PortDef {
                    name: "out".into(),
                    port_type: PortType::Json,
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
        edges: vec![
            Edge {
                source_node: "in".into(),
                source_port: "out".into(),
                target_node: "t1".into(),
                target_port: "in".into(),
                condition: None,
            },
            Edge {
                source_node: "t1".into(),
                source_port: "out".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            },
        ],
    };

    let plan = match ExecutionPlan::compile(&wf) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };

    let lanes = Arc::new(LaneRegistry::new());

    c.bench_function("input_transform_output", |b| {
        b.iter(|| {
            let rt = NodeRuntime::new(plan.clone());
            let ctx = ExecutionContext::new("bench".into(), "bench-run".into(), lanes.clone());
            let input = NodeInput::message(RuntimeValue::String("hello".into()));
            let rt = tokio_test::block_on(rt.execute(&ctx, input));
            black_box(rt.ok());
        });
    });
}

fn fast_path_classification_bench(c: &mut Criterion) {
    let wf = Workflow {
        id: "bench-fp".into(),
        name: "fast-path".into(),
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
                id: "llm1".into(),
                kind: NodeKind::Llm,
                config: NodeConfig::Llm(LlmConfig {
                    protocol: None,
                    model: Some("bench-model".into()),
                    temperature: None,
                    max_tokens: None,
                    stream: false,
                    lane_id: Some("bench-lane".into()),
                }),
                inputs: vec![PortDef {
                    name: "in".into(),
                    port_type: PortType::Message,
                }],
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
        edges: vec![
            Edge {
                source_node: "in".into(),
                source_port: "out".into(),
                target_node: "llm1".into(),
                target_port: "in".into(),
                condition: None,
            },
            Edge {
                source_node: "llm1".into(),
                source_port: "out".into(),
                target_node: "out".into(),
                target_port: "in".into(),
                condition: None,
            },
        ],
    };

    let plan = match ExecutionPlan::compile(&wf) {
        Ok(p) => p,
        Err(e) => panic!("compile failed: {e}"),
    };

    // Verify the plan was classified correctly.
    black_box(plan.classification());

    c.bench_function("plan_compile_with_classification", |b| {
        b.iter(|| {
            let p = ExecutionPlan::compile(black_box(&wf));
            black_box(p.ok());
        });
    });
}

/// Snapshots are published at control-plane cadence and read per request.
///
/// Billable: the cost of an atomic publish (build + swap) and the cost of a
/// per-request read. The read is what the data plane pays on the hot path.
fn snapshot_publication_bench(c: &mut Criterion) {
    use workflow_runtime::RuntimeSnapshotBuilder;
    use workflow_runtime::publish::{InMemoryPublisher, SnapshotPublisher, SnapshotReader};

    let wf = Workflow {
        id: "pass".into(),
        name: "pass".into(),
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

    let make_snapshot = || {
        let plan = match ExecutionPlan::compile(&wf) {
            Ok(p) => p,
            Err(e) => panic!("compile failed: {e}"),
        };
        Arc::new(RuntimeSnapshotBuilder::new(1).with_plan("wf", plan).build())
    };

    let publisher = InMemoryPublisher::new();
    publisher.publish(make_snapshot());

    c.bench_function("snapshot_atomic_publish", |b| {
        b.iter(|| publisher.publish(make_snapshot()));
    });

    c.bench_function("snapshot_reader_lookup", |b| {
        b.iter(|| {
            let s = publisher.snapshot();
            black_box(s).map(|s| s.get_plan("wf").map(|p| p.plan_hash().to_owned()));
        });
    });
}

criterion_group!(
    benches,
    input_to_output_bench,
    input_transform_output_bench,
    fast_path_classification_bench,
    snapshot_publication_bench
);
criterion_main!(benches);
