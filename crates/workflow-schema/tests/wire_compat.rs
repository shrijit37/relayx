//! Wire-shape compatibility: the web serializer (`apps/web/src/lib/workflow`)
//! must emit JSON this crate can deserialize. Fixtures mirror its output.
//!
//! Guard rails: if the web serializer changes a config shape, this test
//! fails — keeping the "one canonical workflow" contract honest on the wire.

use workflow_schema::{FallbackStrategy, Node, NodeConfig, NodeKind, Workflow};

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

#[test]
fn fallback_default_retry_on_is_429() {
    // A fallback fixture without retry_on (as old editor output would emit)
    // must FAIL OVER on 429 by default — the runtime reads `[429]`.
    let v: Node =
        serde_json::from_value(fallback_fixture()).expect("fallback node must deserialize");
    let config = match v.config {
        NodeConfig::Fallback(c) => c,
        _ => panic!("expected fallback config"),
    };
    assert_eq!(config.retry_on, vec![429]);
    assert_eq!(config.strategy, FallbackStrategy::Sequential);
}

#[test]
fn fallback_explicit_retry_on_round_trips() {
    let v: Node = serde_json::from_value(serde_json::json!({
        "id": "fb1", "kind": "fallback",
        "config": {
            "kind": "fallback",
            "providers": [{"lane_id": "anthropic-primary", "model": "claude-sonnet"}],
            "rounds": 1,
            "strategy": "round_robin",
            "retry_on": [429, 503]
        },
        "inputs": [], "outputs": []
    }))
    .expect("fallback node must deserialize");
    // Match by reference so `v` is still owned for serialization below.
    let config = match &v.config {
        NodeConfig::Fallback(c) => c,
        _ => panic!("expected fallback config"),
    };
    assert_eq!(config.retry_on, vec![429, 503]);
    assert_eq!(config.strategy, FallbackStrategy::RoundRobin);
    // And the value survives serialization (the web → gateway wire path).
    let json = serde_json::to_value(&v).expect("fallback node must serialize");
    assert_eq!(json["config"]["retry_on"], serde_json::json!([429, 503]));
    assert_eq!(json["config"]["strategy"], serde_json::json!("round_robin"));
}

#[test]
fn retry_retry_on_round_trips() {
    let v: Node = serde_json::from_value(serde_json::json!({
        "id": "r1", "kind": "retry",
        "config": {
            "kind": "retry",
            "max_attempts": 3,
            "delay_ms": 500,
            "on_timeout": true,
            "on_provider_error": false,
            "retry_on": [429, 503],
            "target": {
                "kind": "llm", "protocol": "openai_chat", "model": "gpt-4",
                "stream": true, "lane_id": "openai-direct"
            }
        },
        "inputs": [], "outputs": []
    }))
    .expect("retry node must deserialize");
    let config = match v.config {
        NodeConfig::Retry(c) => c,
        _ => panic!("expected retry config"),
    };
    assert_eq!(config.retry_on, vec![429, 503]);
    assert!(!config.on_provider_error);
}

#[test]
fn retry_without_retry_on_defaults_to_429() {
    let v: Node = serde_json::from_value(serde_json::json!({
        "id": "r1", "kind": "retry",
        "config": {
            "kind": "retry",
            "max_attempts": 2,
            "delay_ms": 1000,
            "on_timeout": true,
            "on_provider_error": true,
            "target": {
                "kind": "llm", "stream": true, "lane_id": "openai-direct"
            }
        },
        "inputs": [], "outputs": []
    }))
    .expect("retry node must deserialize");
    let config = match v.config {
        NodeConfig::Retry(c) => c,
        _ => panic!("expected retry config"),
    };
    // The serde default keeps legacy/gap-filled retry configs retrying 429.
    assert_eq!(config.retry_on, vec![429]);
}
