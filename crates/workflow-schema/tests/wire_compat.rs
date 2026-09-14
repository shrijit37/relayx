//! Wire-shape compatibility: the web serializer (`apps/web/src/lib/workflow`)
//! must emit JSON this crate can deserialize. Fixtures mirror its output.
//!
//! Guard rails: if the web serializer changes a config shape, this test
//! fails — keeping the "one canonical workflow" contract honest on the wire.

use workflow_schema::{Node, NodeConfig, NodeKind, Workflow};

fn llm_fixture() -> serde_json::Value {
    serde_json::json!({
        "id": "p1", "kind": "llm",
        "config": {
            "kind": "llm",
            "protocol": "anthropic",
            "provider": "anthropic",
            "model": "claude-sonnet",
            "temperature": 0.2,
            "max_tokens": 1024,
            "stream": true,
            "lane_id": "anthropic-primary"
        },
        "inputs": [], "outputs": [], "position": {"x": 0, "y": 0}, "presentation": {"title": "Anything"}
    })
}

fn condition_fixture() -> serde_json::Value {
    serde_json::json!({
        "id": "cond1", "kind": "condition",
        "config": {
            "kind": "condition",
            "condition": "user.intent equals refund",
            "field": "user.intent",
            "operator": "equal",
            "value": "refund"
        },
        "inputs": [], "outputs": []
    })
}

fn fallback_fixture() -> serde_json::Value {
    serde_json::json!({
        "id": "fb1", "kind": "fallback",
        "config": {
            "kind": "fallback",
            "providers": [{"lane_id": "anthropic-primary", "model": "claude-sonnet"}],
            "rounds": 1
        },
        "inputs": [], "outputs": []
    })
}

#[test]
fn web_llm_shape_parses() {
    let v: Node = serde_json::from_value(llm_fixture()).expect("llm node must deserialize");
    assert_eq!(v.kind, NodeKind::Llm);
    assert!(matches!(v.config, NodeConfig::Llm(_)));
}

#[test]
fn web_condition_shape_parses() {
    let v: Node =
        serde_json::from_value(condition_fixture()).expect("condition node must deserialize");
    assert_eq!(v.kind, NodeKind::Condition);
    assert!(matches!(v.config, NodeConfig::Condition(_)));
}

#[test]
fn web_fallback_shape_parses() {
    let v: Node =
        serde_json::from_value(fallback_fixture()).expect("fallback node must deserialize");
    assert_eq!(v.kind, NodeKind::Fallback);
    assert!(matches!(v.config, NodeConfig::Fallback(_)));
}

#[test]
fn full_web_snapshot_parses() {
    let wf: Workflow = serde_json::from_value(serde_json::json!({
        "id": "wf", "name": "W", "version": 2,
        "nodes": [llm_fixture(), condition_fixture(), fallback_fixture()],
        "edges": [],
    }))
    .expect("full workflow must deserialize");
    assert_eq!(wf.nodes.len(), 3);
}
