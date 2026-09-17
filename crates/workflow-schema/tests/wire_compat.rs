//! Wire-shape compatibility: the web serializer (`apps/web/src/lib/workflow`)
//! must emit JSON this crate can deserialize. Fixtures mirror its output.
//!
//! Guard rails: if the web serializer changes a config shape, this test
//! fails — keeping the "one canonical workflow" contract honest on the wire.

use workflow_schema::{FallbackStrategy, Node, NodeConfig, NodeKind, PortType, Workflow};

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

/// A7: the web serializer now emits optional per-entry `protocol` (mirroring
/// the frontend FallbackEntryConfig — serializer.ts providers branch).
fn fallback_with_protocol_fixture() -> serde_json::Value {
    serde_json::json!({
        "id": "fb2", "kind": "fallback",
        "config": {
            "kind": "fallback",
            "providers": [
                {"lane_id": "anthropic-primary", "model": "claude-sonnet", "protocol": "anthropic"},
                {"lane_id": "openai-direct", "model": "gpt-4o", "protocol": "openai_chat"}
            ],
            "rounds": 2,
            "strategy": "round_robin",
            "retry_on": []
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
fn web_fallback_protocol_and_explicit_empty_retry_on_round_trip() {
    // Every entry carries lane_id + model + optional protocol; strategy and
    // an EXPLICIT [] retry_on must survive the wire (231170e semantics:
    // absent key → Rust default [429]; explicit [] → rotation disabled).
    let v: Node = serde_json::from_value(fallback_with_protocol_fixture())
        .expect("fallback with protocol must deserialize");
    let config = match &v.config {
        NodeConfig::Fallback(c) => c,
        _ => panic!("expected fallback config"),
    };
    assert_eq!(config.providers.len(), 2);
    assert_eq!(config.providers[0].protocol.as_deref(), Some("anthropic"));
    assert_eq!(config.providers[1].protocol.as_deref(), Some("openai_chat"));
    assert_eq!(config.providers[1].model, "gpt-4o");
    assert_eq!(config.strategy, FallbackStrategy::RoundRobin);
    assert!(
        config.retry_on.is_empty(),
        "explicit [] must disable rotation"
    );
    // The values survive serialization (the web → gateway wire path).
    let json = serde_json::to_value(&v).expect("fallback node must serialize");
    assert_eq!(
        json["config"]["providers"][0]["protocol"],
        serde_json::json!("anthropic")
    );
    assert_eq!(
        json["config"]["providers"][1]["protocol"],
        serde_json::json!("openai_chat")
    );
    assert_eq!(json["config"]["retry_on"], serde_json::json!([]));
    // An omitted protocol stays omitted (skip_serializing_if) — mixed chains
    // (some entries with, some without) are valid.
    let mixed: Node = serde_json::from_value(serde_json::json!({
        "id": "fb3", "kind": "fallback",
        "config": {
            "kind": "fallback",
            "providers": [
                {"lane_id": "anthropic-primary", "model": "claude-sonnet"},
                {"lane_id": "openai-direct", "model": "gpt-4o", "protocol": "openai_responses"}
            ],
            "rounds": 1
        },
        "inputs": [], "outputs": []
    }))
    .expect("mixed fallback must deserialize");
    let mixed_config = match &mixed.config {
        NodeConfig::Fallback(c) => c,
        _ => panic!("expected fallback config"),
    };
    assert!(mixed_config.providers[0].protocol.is_none());
    assert_eq!(
        mixed_config.providers[1].protocol.as_deref(),
        Some("openai_responses")
    );
}

#[test]
fn web_input_output_editor_metadata_round_trips() {
    // C15: the web serializer emits value/variables/description on input and
    // value on output (serializer.ts input/output branches). Before the Rust
    // struct extension these keys silently vanished on the wire; they must
    // round-trip losslessly now.
    let input_json = serde_json::json!({
        "id": "in1", "kind": "input",
        "config": {
            "kind": "input",
            "input_type": "json",
            "value": { "hello": "world" },
            "description": "Accepts arbitrary JSON payloads",
            "variables": [
                { "name": "user_id", "type": "string", "required": true },
                { "name": "count", "type": "number", "description": "batch size" },
            ]
        },
        "inputs": [], "outputs": []
    });
    let v: Node =
        serde_json::from_value(input_json).expect("input node with metadata must deserialize");
    let cfg = match &v.config {
        NodeConfig::Input(c) => c,
        _ => panic!("expected input config"),
    };
    assert_eq!(cfg.input_type, PortType::Json);
    assert_eq!(cfg.value, Some(serde_json::json!({ "hello": "world" })));
    assert_eq!(
        cfg.description.as_deref(),
        Some("Accepts arbitrary JSON payloads")
    );
    let vars = cfg
        .variables
        .as_ref()
        .expect("input variables must be present");
    assert_eq!(vars.len(), 2);
    assert_eq!(vars[0].name, "user_id");
    assert_eq!(vars[0].r#type, "string");
    assert_eq!(vars[0].required, Some(true));
    assert_eq!(vars[1].description.as_deref(), Some("batch size"));

    // Re-serialize and compare value-equality (serde may reorder keys).
    let json = serde_json::to_value(&v).expect("input node must serialize");
    assert_eq!(
        json["config"]["value"],
        serde_json::json!({ "hello": "world" })
    );
    assert_eq!(
        json["config"]["description"],
        serde_json::json!("Accepts arbitrary JSON payloads")
    );
    assert_eq!(
        json["config"]["variables"][0]["required"],
        serde_json::json!(true)
    );
    assert_eq!(
        json["config"]["variables"][1]["description"],
        serde_json::json!("batch size")
    );

    // Output node with a `value` key (editor metadata) must not be dropped.
    let output_json = serde_json::json!({
        "id": "out1", "kind": "output",
        "config": { "kind": "output", "output_type": "message", "value": "final transcript" },
        "inputs": [], "outputs": []
    });
    let ov: Node =
        serde_json::from_value(output_json).expect("output node with metadata must deserialize");
    let oc = match &ov.config {
        NodeConfig::Output(c) => c,
        _ => panic!("expected output config"),
    };
    assert_eq!(oc.value, Some(serde_json::json!("final transcript")));
    let ojson = serde_json::to_value(&ov).expect("output node must serialize");
    assert_eq!(
        ojson["config"]["value"],
        serde_json::json!("final transcript")
    );

    // Empty editor metadata must serialize WITHOUT the optional keys
    // (skip_serializing_if) — compact wire, stable plan hashes.
    let plain: Node = serde_json::from_value(serde_json::json!({
        "id": "in2", "kind": "input",
        "config": { "kind": "input", "input_type": "message" },
        "inputs": [], "outputs": []
    }))
    .expect("plain input must deserialize");
    let plain_json = serde_json::to_value(&plain).expect("plain input must serialize");
    assert!(plain_json["config"].get("value").is_none());
    assert!(plain_json["config"].get("description").is_none());
    assert!(plain_json["config"].get("variables").is_none());
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
