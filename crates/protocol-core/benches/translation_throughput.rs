//! Benchmarks for protocol translation overhead.
//!
//! Measures per-operation overhead for:
//! - Request decoding (wire format → canonical)
//! - Response encoding (canonical → wire format)
//! - Stream event encoding (canonical → wire event)
//! - Cross-adapter translation (Protocol A → Canonical → Protocol B)
//!
//! Run with: cargo bench -p protocol-core

use criterion::{Criterion, criterion_group, criterion_main};
use protocol_core::adapters::anthropic_messages;
use protocol_core::adapters::openai_chat;
use protocol_core::adapters::openai_responses;
use protocol_core::canonical::*;
use std::hint::black_box;

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn openai_request_json() -> String {
    r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is the capital of France?"},
            {"role": "assistant", "content": "Paris is the capital of France."},
            {"role": "user", "content": "And what about Germany?"}
        ],
        "temperature": 0.7,
        "max_tokens": 1024,
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the weather for a city",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }
        }],
        "stream": false
    }"#
    .to_string()
}

fn anthropic_request_json() -> String {
    r#"{
        "model": "claude-3-opus-20240229",
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "Hello!"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_abc", "name": "search", "input": {"query": "rust async"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_abc", "content": "Found 10 results"}
            ]}
        ],
        "tools": [{
            "name": "search",
            "description": "Search the web",
            "input_schema": {"type": "object", "properties": {"query": {"type": "string"}}}
        }]
    }"#
    .to_string()
}

fn canonical_response() -> CanonicalResponse {
    CanonicalResponse {
        id: "resp_bench".into(),
        model: "gpt-4".into(),
        content: vec![
            ContentBlock::Reasoning(ReasoningContent {
                thinking: "The user is asking about the capital of France. I should provide a direct and helpful answer.".into(),
                signature: None,
            }),
            ContentBlock::Text(TextContent {
                text: "The capital of France is Paris. It has been the capital since the 10th century and is the largest city in France.".into(),
            }),
        ],
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            input_tokens: Some(150),
            output_tokens: Some(75),
            total_tokens: Some(225),
            ..Default::default()
        }),
        extensions: ProviderExtensions::default(),
    }
}

fn stream_events() -> Vec<CanonicalStreamEvent> {
    vec![
        CanonicalStreamEvent::MessageStart {
            message: StreamMessageInfo {
                id: "msg_bench".into(),
                model: "gpt-4".into(),
                role: Role::Assistant,
            },
        },
        CanonicalStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text(TextContent {
                text: String::new(),
            }),
        },
        CanonicalStreamEvent::TextDelta {
            index: 0,
            text: "The capital of France is Paris.".into(),
        },
        CanonicalStreamEvent::ContentBlockStop { index: 0 },
        CanonicalStreamEvent::MessageDelta {
            stop_reason: Some(FinishReason::Stop),
            usage: None,
        },
        CanonicalStreamEvent::MessageStop,
    ]
}

// ─── Benchmarks ──────────────────────────────────────────────────────────────

fn bench_request_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_decode");

    let openai_json = openai_request_json();

    group.bench_function("openai_chat", |b| {
        b.iter(|| {
            let req: openai_chat::ChatCompletionRequest =
                match serde_json::from_str(black_box(&openai_json)) {
                    Ok(v) => v,
                    Err(e) => panic!("openai deserialization: {e:?}"),
                };
            let _ = match openai_chat::decode_request(req) {
                Ok(v) => v,
                Err(e) => panic!("openai decode: {e:?}"),
            };
        });
    });

    let anthropic_json = anthropic_request_json();

    group.bench_function("anthropic_messages", |b| {
        b.iter(|| {
            let req: anthropic_messages::MessagesRequest =
                match serde_json::from_str(black_box(&anthropic_json)) {
                    Ok(v) => v,
                    Err(e) => panic!("anthropic deserialization: {e:?}"),
                };
            let _ = match anthropic_messages::decode_request(req) {
                Ok(v) => v,
                Err(e) => panic!("anthropic decode: {e:?}"),
            };
        });
    });

    group.finish();
}

fn bench_response_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("response_encode");
    let resp = canonical_response();

    group.bench_function("openai_chat", |b| {
        b.iter(|| {
            let _ = match openai_chat::encode_response(black_box(&resp), 1234567890) {
                Ok(v) => v,
                Err(e) => panic!("openai encode: {e:?}"),
            };
        });
    });

    group.bench_function("anthropic_messages", |b| {
        b.iter(|| {
            let _ = match anthropic_messages::encode_response(black_box(&resp)) {
                Ok(v) => v,
                Err(e) => panic!("anthropic encode: {e:?}"),
            };
        });
    });

    group.bench_function("openai_responses", |b| {
        b.iter(|| {
            let _ = match openai_responses::encode_response(black_box(&resp), 1234567890) {
                Ok(v) => v,
                Err(e) => panic!("responses encode: {e:?}"),
            };
        });
    });

    group.finish();
}

fn bench_stream_event_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_event_encode");
    let events = stream_events();

    for event in &events {
        let label = format!("{:?}", event);
        let short_label = if label.len() > 30 {
            format!("{}…", &label[..30])
        } else {
            label
        };

        group.bench_with_input(format!("openai_chat/{short_label}"), event, |b, event| {
            b.iter(|| {
                let _ = match openai_chat::encode_stream_event(
                    black_box(event),
                    "resp_1",
                    "gpt-4",
                    0,
                ) {
                    Ok(v) => v,
                    Err(e) => panic!("openai stream encode: {e:?}"),
                };
            });
        });

        group.bench_with_input(format!("anthropic/{short_label}"), event, |b, event| {
            b.iter(|| {
                let _ = match anthropic_messages::encode_stream_event(
                    black_box(event),
                    "msg_1",
                    "claude-3",
                ) {
                    Ok(v) => v,
                    Err(e) => panic!("anthropic stream encode: {e:?}"),
                };
            });
        });
    }

    group.finish();
}

fn bench_cross_adapter_translation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_adapter_translation");

    let openai_json = openai_request_json();

    group.bench_function("openai_to_anthropic", |b| {
        b.iter(|| {
            let req: openai_chat::ChatCompletionRequest =
                match serde_json::from_str(black_box(&openai_json)) {
                    Ok(v) => v,
                    Err(e) => panic!("deserialization: {e:?}"),
                };
            let canonical = match openai_chat::decode_request(req) {
                Ok(v) => v,
                Err(e) => panic!("decode: {e:?}"),
            };
            let _ = match anthropic_messages::encode_request(&canonical) {
                Ok(v) => v,
                Err(e) => panic!("encode: {e:?}"),
            };
        });
    });

    let resp = canonical_response();

    group.bench_function("anthropic_response_to_openai", |b| {
        b.iter(|| {
            let anthropic_resp = match anthropic_messages::encode_response(black_box(&resp)) {
                Ok(v) => v,
                Err(e) => panic!("anthropic encode: {e:?}"),
            };
            let canonical = match anthropic_messages::decode_response(anthropic_resp) {
                Ok(v) => v,
                Err(e) => panic!("decode: {e:?}"),
            };
            let _ = match openai_chat::encode_response(&canonical, 0) {
                Ok(v) => v,
                Err(e) => panic!("openai encode: {e:?}"),
            };
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_request_decode,
    bench_response_encode,
    bench_stream_event_encode,
    bench_cross_adapter_translation,
);
criterion_main!(benches);
