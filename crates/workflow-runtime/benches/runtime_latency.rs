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

criterion_group!(benches, input_to_output_bench, input_transform_output_bench);
criterion_main!(benches);
