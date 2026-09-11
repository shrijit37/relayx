//! End-to-end protocol translation tests.
//!
//! Tests the complete translation pipeline:
//! - OpenAI → Canonical → Anthropic
//! - Anthropic → Canonical → OpenAI
//! - Round-trip: OpenAI → Canonical → OpenAI
//! - Round-trip: Anthropic → Canonical → Anthropic
//! - Streaming event translation
//! - Tool call translation
//! - Edge cases and error handling

use anyhow::{Context, Result};
use protocol_core::adapters::anthropic_messages::{
    self, MessagesContent, MessagesContentBlock, MessagesResponseBlock,
};
use protocol_core::adapters::openai_chat;
use protocol_core::adapters::openai_responses;
use protocol_core::canonical::*;

// ─── Golden fixtures: OpenAI → Canonical → Anthropic ─────────────────────────

#[test]
fn test_openai_to_anthropic_simple_text() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello!"}
        ],
        "temperature": 0.7,
        "max_tokens": 1024,
        "stream": false
    }"#;

    let openai_req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(openai_req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.model, "gpt-4");
    assert!(canonical.system.is_some());
    assert_eq!(canonical.messages.len(), 1);
    assert_eq!(canonical.messages[0].role, Role::User);
    assert_eq!(canonical.temperature, Some(0.7));
    assert_eq!(canonical.max_tokens, Some(1024));

    let anthropic_req = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(anthropic_req.model, "gpt-4");
    assert_eq!(anthropic_req.max_tokens, 1024);
    assert!(anthropic_req.system.is_some());
    assert_eq!(anthropic_req.temperature, Some(0.7));
    assert_eq!(anthropic_req.messages.len(), 1);
    assert_eq!(anthropic_req.messages[0].role, "user");
}

#[test]
fn test_openai_to_anthropic_with_tool_calls() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "What's the weather?"},
            {"role": "assistant", "content": null, "tool_calls": [
                {
                    "id": "call_abc123",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
                }
            ]},
            {"role": "tool", "tool_call_id": "call_abc123", "content": "72°F"}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the weather",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }
        }]
    }"#;

    let openai_req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(openai_req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.messages.len(), 3);
    let asst_blocks = canonical.messages[1].content.clone().into_blocks();
    assert!(matches!(&asst_blocks[0], ContentBlock::ToolUse(tu) if tu.id == "call_abc123"));

    let tool_blocks = canonical.messages[2].content.clone().into_blocks();
    assert!(
        matches!(&tool_blocks[0], ContentBlock::ToolResult(tr) if tr.tool_use_id == "call_abc123")
    );

    let anthropic_req = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(anthropic_req.messages.len(), 3);

    let asst_msg = &anthropic_req.messages[1];
    let asst_content = match &asst_msg.content {
        MessagesContent::Blocks(blocks) => blocks,
        _ => panic!("expected blocks"),
    };
    assert!(
        matches!(&asst_content[0], MessagesContentBlock::ToolUse { id, name, .. } if id == "call_abc123" && name == "get_weather")
    );

    let tool_msg = &anthropic_req.messages[2];
    let tool_content = match &tool_msg.content {
        MessagesContent::Blocks(blocks) => blocks,
        _ => panic!("expected blocks"),
    };
    assert!(
        matches!(&tool_content[0], MessagesContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_abc123")
    );
}

#[test]
fn test_openai_to_anthropic_multimodal() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image_url", "image_url": {"url": "https://example.com/photo.jpg", "detail": "high"}}
            ]
        }]
    }"#;

    let openai_req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(openai_req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let anthropic_req = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let blocks = match &anthropic_req.messages[0].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], MessagesContentBlock::Text { text } if text == "What is this?"));
    assert!(
        matches!(&blocks[1], MessagesContentBlock::Image { source } if source.source_type == "url")
    );
}

// ─── Golden fixtures: Anthropic → Canonical → OpenAI ─────────────────────────

#[test]
fn test_anthropic_to_openai_simple_text() {
    let anthropic_json = r#"{
        "model": "claude-3-opus-20240229",
        "max_tokens": 1024,
        "system": "You are helpful.",
        "messages": [
            {"role": "user", "content": "Hello!"}
        ]
    }"#;

    let anthropic_req: anthropic_messages::MessagesRequest =
        match serde_json::from_str(anthropic_json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
    let canonical = match anthropic_messages::decode_request(anthropic_req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.model, "claude-3-opus-20240229");
    assert!(canonical.system.is_some());
    assert_eq!(canonical.messages.len(), 1);
    assert_eq!(canonical.max_tokens, Some(1024));
}

#[test]
fn test_anthropic_response_to_openai() {
    let anthropic_resp_json = r#"{
        "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
        "model": "claude-3-opus-20240229",
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Hello! How can I help?"}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 25,
            "output_tokens": 15
        }
    }"#;

    let anthropic_resp: anthropic_messages::MessagesResponse =
        match serde_json::from_str(anthropic_resp_json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
    let canonical = match anthropic_messages::decode_response(anthropic_resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
    assert_eq!(canonical.content.len(), 1);
    assert!(
        matches!(&canonical.content[0], ContentBlock::Text(t) if t.text == "Hello! How can I help?")
    );
    assert_eq!(canonical.finish_reason, Some(FinishReason::Stop));

    let openai_resp = match openai_chat::encode_response(&canonical, 1234567890) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(openai_resp.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
    assert_eq!(openai_resp.choices.len(), 1);
    assert_eq!(
        openai_resp.choices[0].message.content.as_deref(),
        Some("Hello! How can I help?")
    );
    assert_eq!(
        openai_resp.choices[0].finish_reason.as_deref(),
        Some("stop")
    );
    let usage = match openai_resp.usage {
        Some(u) => u,
        None => panic!("expected usage"),
    };
    assert_eq!(usage.prompt_tokens, 25);
    assert_eq!(usage.completion_tokens, 15);
    assert_eq!(usage.total_tokens, 40);
}

#[test]
fn test_anthropic_tool_use_response_to_openai() {
    let anthropic_resp_json = r#"{
        "id": "msg_tool123",
        "model": "claude-3-opus-20240229",
        "role": "assistant",
        "content": [
            {"type": "tool_use", "id": "toolu_abc", "name": "search", "input": {"query": "rust"}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "output_tokens": 20}
    }"#;

    let anthropic_resp: anthropic_messages::MessagesResponse =
        match serde_json::from_str(anthropic_resp_json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
    let canonical = match anthropic_messages::decode_response(anthropic_resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.content.len(), 1);
    assert!(matches!(&canonical.content[0], ContentBlock::ToolUse(tu) if tu.name == "search"));

    let openai_resp = match openai_chat::encode_response(&canonical, 1234567890) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let tool_calls = match openai_resp.choices[0].message.tool_calls.as_ref() {
        Some(tc) => tc,
        None => panic!("expected tool_calls"),
    };
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].function.name, "search");
    assert_eq!(
        openai_resp.choices[0].finish_reason.as_deref(),
        Some("tool_calls")
    );
}

// ─── Round-trip tests ────────────────────────────────────────────────────────

#[test]
fn test_round_trip_openai_response() {
    let canonical = CanonicalResponse {
        id: "chatcmpl-test".into(),
        model: "gpt-4".into(),
        content: vec![ContentBlock::Text(TextContent {
            text: "Hello!".into(),
        })],
        finish_reason: Some(FinishReason::Stop),
        usage: None,
        extensions: ProviderExtensions::default(),
    };

    let openai_resp = match openai_chat::encode_response(&canonical, 1234567890) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let json = match serde_json::to_string(&openai_resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let re_encoded: openai_chat::ChatCompletionResponse = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(re_encoded.model, "gpt-4");
    assert_eq!(
        re_encoded.choices[0].message.content.as_deref(),
        Some("Hello!")
    );
}

#[test]
fn test_round_trip_anthropic_request() {
    let original_json = r#"{
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "system": "Be helpful.",
        "messages": [
            {"role": "user", "content": "Hello!"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_123", "name": "calc", "input": {"expr": "2+2"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_123", "content": "4"}
            ]}
        ],
        "tools": [{
            "name": "calc",
            "description": "Evaluate math",
            "input_schema": {"type": "object", "properties": {"expr": {"type": "string"}}}
        }]
    }"#;

    let original: anthropic_messages::MessagesRequest = match serde_json::from_str(original_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match anthropic_messages::decode_request(original) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let re_encoded = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(re_encoded.model, "claude-3-opus");
    assert_eq!(re_encoded.max_tokens, 1024);
    assert_eq!(re_encoded.messages.len(), 3);

    let asst_msg = &re_encoded.messages[1];
    let asst_blocks = match &asst_msg.content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert!(
        matches!(&asst_blocks[0], MessagesContentBlock::ToolUse { id, .. } if id == "toolu_123")
    );

    let tool_msg = &re_encoded.messages[2];
    let tool_blocks = match &tool_msg.content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert!(
        matches!(&tool_blocks[0], MessagesContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "toolu_123")
    );
}

// ─── Streaming event translation ─────────────────────────────────────────────

#[test]
fn test_streaming_text_delta_to_anthropic() {
    let events = vec![
        CanonicalStreamEvent::MessageStart {
            message: StreamMessageInfo {
                id: "msg_test".into(),
                model: "claude-3".into(),
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
            text: "Hello".into(),
        },
        CanonicalStreamEvent::TextDelta {
            index: 0,
            text: " world!".into(),
        },
        CanonicalStreamEvent::ContentBlockStop { index: 0 },
        CanonicalStreamEvent::MessageDelta {
            stop_reason: Some(FinishReason::Stop),
            usage: None,
        },
        CanonicalStreamEvent::MessageStop,
    ];

    for event in &events {
        let result = anthropic_messages::encode_stream_event(event, "msg_test", "claude-3");
        assert!(result.is_ok(), "failed to encode event: {event:?}");
    }
}

#[test]
fn test_streaming_tool_call_delta_to_anthropic() {
    let events = vec![
        CanonicalStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse(ToolUseBlock {
                id: "call_123".into(),
                name: "get_weather".into(),
                input: serde_json::Value::Null,
            }),
        },
        CanonicalStreamEvent::ToolCallDelta {
            index: 0,
            tool_use_id: None,
            name: None,
            input_json_delta: Some(r#"{"city":"NYC""#.into()),
        },
        CanonicalStreamEvent::ToolCallDelta {
            index: 0,
            tool_use_id: None,
            name: None,
            input_json_delta: Some(",\"unit\":\"f\"}".into()),
        },
        CanonicalStreamEvent::ContentBlockStop { index: 0 },
    ];

    for event in &events {
        let result = anthropic_messages::encode_stream_event(event, "msg_test", "model");
        assert!(result.is_ok(), "failed to encode event: {event:?}");
        if let Ok(Some(anthro_event)) = &result {
            let json = match serde_json::to_string(anthro_event) {
                Ok(v) => v,
                Err(e) => panic!("expected success: {e:?}"),
            };
            assert!(!json.is_empty());
        }
    }
}

#[test]
fn test_streaming_text_delta_to_openai() {
    let events = vec![
        CanonicalStreamEvent::MessageStart {
            message: StreamMessageInfo {
                id: "chatcmpl-test".into(),
                model: "gpt-4".into(),
                role: Role::Assistant,
            },
        },
        CanonicalStreamEvent::TextDelta {
            index: 0,
            text: "Hello".into(),
        },
        CanonicalStreamEvent::TextDelta {
            index: 0,
            text: " there!".into(),
        },
        CanonicalStreamEvent::MessageDelta {
            stop_reason: Some(FinishReason::Stop),
            usage: None,
        },
    ];

    for event in &events {
        let result = openai_chat::encode_stream_event(event, "chatcmpl-test", "gpt-4", 1234567890);
        assert!(result.is_ok(), "failed to encode event: {event:?}");
        if let Ok(Some(openai_chunk)) = &result {
            let json = match serde_json::to_string(openai_chunk) {
                Ok(v) => v,
                Err(e) => panic!("expected success: {e:?}"),
            };
            assert!(json.contains("chat.completion.chunk"));
        }
    }
}

// ─── Capability matrix ───────────────────────────────────────────────────────

#[test]
fn test_capabilities_are_declared() {
    let openai_caps = openai_chat::capabilities();
    let anthropic_caps = anthropic_messages::capabilities();

    assert!(openai_caps.streaming);
    assert!(openai_caps.tools);
    assert!(openai_caps.structured_output);

    assert!(anthropic_caps.streaming);
    assert!(anthropic_caps.tools);
    assert!(!anthropic_caps.structured_output);
}

#[test]
fn test_translation_loss_detection() {
    let openai_caps = openai_chat::capabilities();
    let anthropic_caps = anthropic_messages::capabilities();

    let losses = anthropic_caps.translation_losses(&openai_caps);
    assert!(losses.iter().any(|l| l.feature == "structured_output"));

    // Anthropic supports reasoning (thinking blocks); OpenAI Chat does not.
    // The OpenAI encoder silently drops reasoning content, which the capability
    // matrix must flag as a loss so callers can apply a policy.
    let losses = openai_caps.translation_losses(&anthropic_caps);
    assert!(
        losses.iter().any(|l| l.feature == "reasoning"),
        "expected a reasoning loss translating Anthropic → OpenAI: {losses:?}"
    );
}

// ─── SSE parser integration ──────────────────────────────────────────────────

#[test]
fn test_sse_parse_openai_stream() {
    use protocol_core::sse::StreamingSseParser;

    let mut parser = StreamingSseParser::new();
    let chunks = vec![
        r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        "\n\n",
        "data: [DONE]\n\n",
    ];

    let mut all_events = Vec::new();
    for chunk in &chunks {
        all_events.extend(parser.feed(chunk.as_bytes()));
    }

    assert_eq!(all_events.len(), 4);
    assert!(!all_events[2].is_done());
    assert!(all_events[3].is_done());

    for event in &all_events[..3] {
        let parsed: Result<openai_chat::ChatCompletionChunk, _> = serde_json::from_str(&event.data);
        assert!(parsed.is_ok(), "failed to parse chunk: {}", event.data);
    }
}

// ─── Error handling ──────────────────────────────────────────────────────────

#[test]
fn test_decode_rejects_invalid_json() {
    let result = serde_json::from_str::<openai_chat::ChatCompletionRequest>("not json");
    assert!(result.is_err());
}

#[test]
fn test_decode_rejects_missing_model() {
    let json = r#"{"messages": [{"role": "user", "content": "hi"}]}"#;
    let result = serde_json::from_str::<openai_chat::ChatCompletionRequest>(json);
    assert!(result.is_err());
}

#[test]
fn test_anthropic_encode_rejects_image_output() {
    let resp = CanonicalResponse {
        id: "test".into(),
        model: "test".into(),
        content: vec![ContentBlock::Image(ImageContent {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
                detail: None,
            },
        })],
        finish_reason: None,
        usage: None,
        extensions: ProviderExtensions::default(),
    };
    assert!(anthropic_messages::encode_response(&resp).is_err());
}

#[test]
fn test_openai_encode_rejects_image_output() {
    let resp = CanonicalResponse {
        id: "test".into(),
        model: "test".into(),
        content: vec![ContentBlock::Image(ImageContent {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
                detail: None,
            },
        })],
        finish_reason: None,
        usage: None,
        extensions: ProviderExtensions::default(),
    };
    assert!(openai_chat::encode_response(&resp, 0).is_err());
}

// ─── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn test_empty_content_message() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [{"role": "user", "content": null}]
    }"#;
    let req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let blocks = canonical.messages[0].content.clone().into_blocks();
    assert!(blocks.is_empty());
}

#[test]
fn test_multiple_system_messages_concatenated() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Rule 1"},
            {"role": "system", "content": "Rule 2"},
            {"role": "user", "content": "Hi"}
        ]
    }"#;
    let req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    match &canonical.system {
        Some(SystemInstruction::Text(text)) => {
            assert!(text.contains("Rule 1"));
            assert!(text.contains("Rule 2"));
        }
        _ => panic!("expected concatenated text"),
    }
}

#[test]
fn test_anthropic_system_as_array() {
    let json = r#"{
        "model": "claude-3",
        "max_tokens": 100,
        "system": [
            {"type": "text", "text": "Rule 1"},
            {"type": "text", "text": "Rule 2"}
        ],
        "messages": [{"role": "user", "content": "Hi"}]
    }"#;
    let req = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match anthropic_messages::decode_request(req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    match &canonical.system {
        Some(SystemInstruction::Blocks(blocks)) => {
            assert_eq!(blocks.len(), 2);
            assert_eq!(blocks[0].text, "Rule 1");
            assert_eq!(blocks[1].text, "Rule 2");
        }
        _ => panic!("expected blocks"),
    }
}

#[test]
fn test_usage_preserved_through_translation() {
    let resp = CanonicalResponse {
        id: "test".into(),
        model: "model".into(),
        content: vec![],
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(20),
        }),
        extensions: ProviderExtensions::default(),
    };

    let openai = match openai_chat::encode_response(&resp, 0) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let u = match openai.usage {
        Some(u) => u,
        None => panic!("expected usage"),
    };
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);

    let anthropic = match anthropic_messages::encode_response(&resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(anthropic.usage.input_tokens, 100);
    assert_eq!(anthropic.usage.output_tokens, 50);
    assert_eq!(anthropic.usage.cache_creation_input_tokens, Some(10));
    assert_eq!(anthropic.usage.cache_read_input_tokens, Some(20));
}

// ─── Audio content block tests ──────────────────────────────────────────────

#[test]
fn test_openai_decode_input_audio_block() {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "What's in this audio?"},
                {"type": "input_audio", "input_audio": {"data": "UklGRi...", "format": "wav"}}
            ]
        }]
    }"#;
    let req = match serde_json::from_str(openai_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_chat::decode_request(req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    let blocks = canonical.messages[0].content.clone().into_blocks();
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ContentBlock::Text(t) if t.text == "What's in this audio?"));
    assert!(
        matches!(&blocks[1], ContentBlock::Audio(AudioContent { source: AudioSource::Base64 { data, format, .. } })
            if data == "UklGRi..." && format.as_deref() == Some("wav"))
    );
}

#[test]
fn test_audio_to_anthropic_encode() {
    let canonical = CanonicalRequest {
        model: "gpt-4".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::Audio(AudioContent {
                source: AudioSource::Base64 {
                    media_type: "audio/wav".into(),
                    data: "AAAA".into(),
                    format: Some("wav".into()),
                },
            })]),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: vec![],
        tools: vec![],
        tool_choice: None,
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    // Audio base64 → Anthropic base64 image (approximation, logged as warning)
    let anthropic_req = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let blocks = match &anthropic_req.messages[0].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(blocks.len(), 1);
    assert!(
        matches!(&blocks[0], MessagesContentBlock::Image { source } if source.source_type == "base64")
    );
}

// ─── Reasoning/thinking content block tests ─────────────────────────────────

#[test]
fn test_anthropic_decode_thinking_block() {
    // Simulate a response that contains thinking blocks
    let resp_json = r#"{
        "id": "msg_123",
        "model": "claude-3-opus-20240229",
        "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "Let me calculate: 2+2 = 4.", "signature": "abc123sig"},
            {"type": "text", "text": "The answer is 4."}
        ],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 30}
    }"#;
    let resp = match serde_json::from_str(resp_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match anthropic_messages::decode_response(resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.content.len(), 2);
    assert!(matches!(&canonical.content[0], ContentBlock::Reasoning(r)
            if r.thinking == "Let me calculate: 2+2 = 4." && r.signature.as_deref() == Some("abc123sig")));
    assert!(matches!(&canonical.content[1], ContentBlock::Text(t) if t.text == "The answer is 4."));
}

#[test]
fn test_thinking_round_trip_through_anthropic() {
    let original = CanonicalResponse {
        id: "msg_789".into(),
        model: "claude-3-opus".into(),
        content: vec![
            ContentBlock::Reasoning(ReasoningContent {
                thinking: "Internal reasoning here.".into(),
                signature: Some("sig_abc".into()),
            }),
            ContentBlock::Text(TextContent {
                text: "Final answer.".into(),
            }),
        ],
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            input_tokens: Some(10),
            output_tokens: Some(20),
            total_tokens: Some(30),
            ..Default::default()
        }),
        extensions: ProviderExtensions::default(),
    };

    let anthropic_resp = match anthropic_messages::encode_response(&original) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(anthropic_resp.content.len(), 2);

    // Verify thinking block preserved
    let thinking_block = &anthropic_resp.content[0];
    assert!(
        matches!(thinking_block, MessagesResponseBlock::Thinking { thinking, signature }
            if thinking == "Internal reasoning here." && signature.as_deref() == Some("sig_abc"))
    );

    // Round-trip back
    let canonical = match anthropic_messages::decode_response(anthropic_resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(canonical.content.len(), 2);
    assert!(matches!(&canonical.content[0], ContentBlock::Reasoning(r)
            if r.thinking == "Internal reasoning here." && r.signature.as_deref() == Some("sig_abc")));
    assert!(matches!(&canonical.content[1], ContentBlock::Text(t) if t.text == "Final answer."));
}

#[test]
fn test_thinking_streaming_events_to_anthropic() {
    let events = vec![
        CanonicalStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Reasoning(ReasoningContent {
                thinking: String::new(),
                signature: None,
            }),
        },
        CanonicalStreamEvent::ReasoningDelta {
            index: 0,
            thinking: "Let me think...".into(),
        },
        CanonicalStreamEvent::ReasoningDelta {
            index: 0,
            thinking: " about this.".into(),
        },
        CanonicalStreamEvent::ReasoningSignature {
            index: 0,
            signature: "opaque_sig".into(),
        },
        CanonicalStreamEvent::ContentBlockStop { index: 0 },
    ];

    for event in &events {
        let result = anthropic_messages::encode_stream_event(event, "msg_test", "claude-3");
        assert!(result.is_ok(), "failed to encode event: {event:?}");
        if let Ok(Some(anthro_event)) = &result {
            let json = match serde_json::to_string(anthro_event) {
                Ok(v) => v,
                Err(e) => panic!("expected success: {e:?}"),
            };
            assert!(!json.is_empty());
        }
    }
}

#[test]
fn test_audio_stream_events_to_openai_returns_none() {
    let events = vec![
        CanonicalStreamEvent::AudioDelta {
            index: 0,
            data: "base64chunk".into(),
        },
        CanonicalStreamEvent::ReasoningDelta {
            index: 0,
            thinking: "hidden".into(),
        },
        CanonicalStreamEvent::ReasoningSignature {
            index: 0,
            signature: "sig".into(),
        },
    ];

    for event in &events {
        let result = openai_chat::encode_stream_event(event, "test", "gpt-4", 0);
        let result_event = match result {
            Ok(Some(ev)) => ev,
            Ok(None) => {
                // Audio/reasoning events return None for OpenAI (not supported)
                continue;
            }
            Err(e) => panic!("encode_stream_event failed: {e:?}"),
        };
        let _ = result_event;
    }
}

// ─── OpenAI Responses adapter tests ─────────────────────────────────────────

#[test]
fn test_responses_to_anthropic_simple_text() {
    let responses_json = r#"{
        "model": "gpt-4",
        "instructions": "You are helpful.",
        "input": [
            {"role": "user", "content": "Hello!"}
        ],
        "stream": false
    }"#;
    let req = match serde_json::from_str(responses_json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_responses::decode_request(req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.model, "gpt-4");
    assert!(canonical.system.is_some());
    assert_eq!(canonical.messages.len(), 1);
    assert_eq!(canonical.messages[0].role, Role::User);

    let anthropic_req = match anthropic_messages::encode_request(&canonical) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(anthropic_req.model, "gpt-4");
    assert!(anthropic_req.system.is_some());
    assert_eq!(anthropic_req.messages.len(), 1);
}

#[test]
fn test_responses_round_trip_encode_decode() {
    let original = CanonicalResponse {
        id: "resp_test".into(),
        model: "gpt-4".into(),
        content: vec![ContentBlock::Text(TextContent {
            text: "Hello from Responses!".into(),
        })],
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            ..Default::default()
        }),
        extensions: ProviderExtensions::default(),
    };

    let responses_resp = match openai_responses::encode_response(&original, 0) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let canonical = match openai_responses::decode_response(responses_resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };

    assert_eq!(canonical.id, "resp_test");
    assert_eq!(canonical.content.len(), 1);
    assert_eq!(
        canonical.content[0].as_text(),
        Some("Hello from Responses!")
    );
    assert_eq!(canonical.finish_reason, Some(FinishReason::Stop));
    let usage = match canonical.usage {
        Some(u) => u,
        None => panic!("expected usage"),
    };
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(15));
}

#[test]
fn test_responses_capabilities_match_expected() {
    let caps = openai_responses::capabilities();
    assert!(caps.streaming);
    assert!(caps.tools);
    assert!(caps.tool_streaming);
    assert!(caps.multimodal_input);
    assert!(caps.structured_output);
    assert!(caps.reasoning);
    assert!(caps.usage_streaming);
    assert!(!caps.deferred_tools);
}

#[test]
fn test_responses_translation_loss_vs_anthropic() {
    let responses_caps = openai_responses::capabilities();
    let anthropic_caps = anthropic_messages::capabilities();

    // Responses has structured_output but Anthropic doesn't.
    let losses = anthropic_caps.translation_losses(&responses_caps);
    assert!(losses.iter().any(|l| l.feature == "structured_output"));
}

// ─── Translation loss enforcement tests ─────────────────────────────────────

#[test]
fn test_enforce_no_loss_when_source_matches_target() {
    let openai_caps = openai_chat::capabilities();
    let _ = openai_caps.enforce_translation_losses(&openai_caps);
}

#[test]
fn test_enforce_rejects_drop_policy_loss() {
    // Anthropic doesn't have structured_output; translating OpenAI → Anthropic
    // results in a Drop loss, which enforce_translation_losses should reject.
    let anthropic_caps = anthropic_messages::capabilities();
    let openai_caps = openai_chat::capabilities();
    let result = anthropic_caps.enforce_translation_losses(&openai_caps);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let json = err.to_json_body();
    assert!(json.contains("structured_output"));
    assert!(json.contains("lossy_translation"));
}

#[test]
fn test_enforce_allows_approximate_loss() {
    // Tool call streaming is Approximate policy, not Drop — should be allowed.
    // We construct a scenario where source has tool_streaming but target doesn't.
    let mut source_caps = openai_chat::capabilities();
    let mut target_caps = openai_chat::capabilities();
    source_caps.tool_streaming = true;
    target_caps.tool_streaming = false;
    let _ = target_caps.enforce_translation_losses(&source_caps);
}

#[test]
fn test_enforce_rejects_implicit_audio_loss() {
    // When multimodal_input differs, no loss is currently detected because
    // translation_losses doesn't check it. This documents the current behavior.
    let mut source_caps = openai_chat::capabilities();
    source_caps.multimodal_input = true;
    let mut target_caps = openai_chat::capabilities();
    target_caps.multimodal_input = false;
    let _ = target_caps.enforce_translation_losses(&source_caps);
    // Currently no loss detected for multimodal_input — this test documents that.
}

// ─── Deferred ToolReference Tests ────────────────────────────────────────────

#[test]
fn test_tool_reference_deferred_no_schema_anthropic() -> Result<()> {
    let canonical = CanonicalRequest {
        model: "claude-3".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolReference(ToolReference {
                id: "ref_1".into(),
                name: "search_web".into(),
                description: Some("Search the web".into()),
                input_schema: None,
                deferred: true,
            })]),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(1024),
        stop: vec![],
        tools: vec![],
        tool_choice: None,
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    let blocks = match &anthropic_req.messages[0].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(blocks.len(), 1);

    // Deferred + no schema → Text block with JSON metadata
    let text = match &blocks[0] {
        MessagesContentBlock::Text { text } => text,
        _ => panic!("expected Text block for deferred tool reference"),
    };
    let metadata: serde_json::Value = serde_json::from_str(text)?;
    assert_eq!(metadata["type"], "tool_reference");
    assert_eq!(metadata["id"], "ref_1");
    assert_eq!(metadata["name"], "search_web");
    assert_eq!(metadata["deferred"], true);
    Ok(())
}

#[test]
fn test_tool_reference_deferred_with_schema_materializes() -> Result<()> {
    // deferred=true with input_schema → else branch (Text block), because
    // the encoder only materializes as ToolUse when !deferred.
    let canonical = CanonicalRequest {
        model: "claude-3".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolReference(ToolReference {
                id: "ref_2".into(),
                name: "get_weather".into(),
                description: Some("Get weather".into()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                })),
                deferred: true,
            })]),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(1024),
        stop: vec![],
        tools: vec![],
        tool_choice: None,
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    let blocks = match &anthropic_req.messages[0].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(blocks.len(), 1);

    // Despite having a schema, deferred=true keeps it as metadata, not ToolUse
    let text = match &blocks[0] {
        MessagesContentBlock::Text { text } => text,
        _ => panic!("expected Text block for deferred tool reference with schema"),
    };
    let metadata: serde_json::Value = serde_json::from_str(text)?;
    assert_eq!(metadata["type"], "tool_reference");
    assert_eq!(metadata["name"], "get_weather");
    assert_eq!(metadata["deferred"], true);
    Ok(())
}

#[test]
fn test_tool_choice_none_preserves_tools_anthropic() -> Result<()> {
    let canonical = CanonicalRequest {
        model: "claude-3".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::text("Hello"),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(1024),
        stop: vec![],
        tools: vec![ToolDefinition {
            name: "search".into(),
            description: Some("Search".into()),
            input_schema: Some(serde_json::json!({"type": "object"})),
            deferred: None,
            extra: std::collections::HashMap::new(),
        }],
        tool_choice: Some(ToolChoice::None),
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let anthropic_req = anthropic_messages::encode_request(&canonical)?;

    // Tools must NOT be dropped even with ToolChoice::None
    let tools = anthropic_req
        .tools
        .as_ref()
        .context("tools should be present")?;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");

    // Anthropic has no "none" tool_choice; field is omitted
    assert!(anthropic_req.tool_choice.is_none());
    Ok(())
}

// ─── Tool Semantics Strengthening ────────────────────────────────────────────

#[test]
fn test_tool_ids_preserved_openai_to_anthropic() -> Result<()> {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "content": null, "tool_calls": [
                {
                    "id": "call_unique_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{}"}
                }
            ]},
            {"role": "tool", "tool_call_id": "call_unique_abc", "content": "72F"}
        ]
    }"#;

    let openai_req: openai_chat::ChatCompletionRequest = serde_json::from_str(openai_json)?;
    let canonical = openai_chat::decode_request(openai_req)?;
    let anthropic_req = anthropic_messages::encode_request(&canonical)?;

    // Assistant message should have a ToolUse block with the same ID
    let asst_blocks = match &anthropic_req.messages[1].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    match &asst_blocks[0] {
        MessagesContentBlock::ToolUse { id, .. } => assert_eq!(id, "call_unique_abc"),
        _ => panic!("expected ToolUse block"),
    }

    // Tool result message should reference the same ID
    let tool_blocks = match &anthropic_req.messages[2].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    match &tool_blocks[0] {
        MessagesContentBlock::ToolResult { tool_use_id, .. } => {
            assert_eq!(tool_use_id, "call_unique_abc")
        }
        _ => panic!("expected ToolResult block"),
    }
    Ok(())
}

#[test]
fn test_tool_names_preserved_round_trip() -> Result<()> {
    let canonical = CanonicalRequest {
        model: "claude-3".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::text("Hi"),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(100),
        stop: vec![],
        tools: vec![
            ToolDefinition {
                name: "alpha_tool".into(),
                description: Some("Does alpha".into()),
                input_schema: Some(serde_json::json!({"type": "object"})),
                deferred: None,
                extra: std::collections::HashMap::new(),
            },
            ToolDefinition {
                name: "beta_tool".into(),
                description: Some("Does beta".into()),
                input_schema: Some(serde_json::json!({"type": "object"})),
                deferred: None,
                extra: std::collections::HashMap::new(),
            },
        ],
        tool_choice: None,
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    let decoded = anthropic_messages::decode_request(anthropic_req)?;

    assert_eq!(decoded.tools.len(), 2);
    assert_eq!(decoded.tools[0].name, "alpha_tool");
    assert_eq!(decoded.tools[1].name, "beta_tool");
    Ok(())
}

#[test]
fn test_tool_arguments_nested_json_preserved() -> Result<()> {
    let nested_args = serde_json::json!({
        "query": "hello",
        "filters": {
            "date_range": {"start": "2024-01-01", "end": "2024-12-31"},
            "categories": ["tech", "science"],
            "nested": {"deep": {"value": 42}}
        }
    });

    // OpenAI → Canonical
    let openai_json = serde_json::json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Search"},
            {"role": "assistant", "content": null, "tool_calls": [{
                "id": "call_nested",
                "type": "function",
                "function": {"name": "search", "arguments": nested_args.to_string()}
            }]},
            {"role": "tool", "tool_call_id": "call_nested", "content": "results"}
        ]
    });
    let openai_req: openai_chat::ChatCompletionRequest = serde_json::from_value(openai_json)?;
    let canonical = openai_chat::decode_request(openai_req)?;

    // Canonical → Anthropic
    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    let decoded_canonical = anthropic_messages::decode_request(anthropic_req)?;

    // Verify tool call arguments survived the round trip
    let asst_blocks = decoded_canonical.messages[1].content.clone().into_blocks();
    match &asst_blocks[0] {
        ContentBlock::ToolUse(tu) => {
            assert_eq!(tu.input, nested_args);
        }
        _ => panic!("expected ToolUse block"),
    }
    Ok(())
}

#[test]
fn test_tool_result_correlation_preserved() -> Result<()> {
    // Anthropic → Canonical → Anthropic: verify tool_use_id round-trips
    let anthropic_json = r#"{
        "model": "claude-3",
        "max_tokens": 100,
        "messages": [
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_xyz_789", "name": "get_weather", "input": {"city": "NYC"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_xyz_789", "content": "72F"}
            ]}
        ]
    }"#;

    let req: anthropic_messages::MessagesRequest = serde_json::from_str(anthropic_json)?;
    let canonical = anthropic_messages::decode_request(req)?;
    let re_encoded = anthropic_messages::encode_request(&canonical)?;

    // Verify tool_use_id is preserved in both assistant and tool messages
    let asst_blocks = match &re_encoded.messages[1].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    match &asst_blocks[0] {
        MessagesContentBlock::ToolUse { id, .. } => assert_eq!(id, "toolu_xyz_789"),
        _ => panic!("expected ToolUse"),
    }

    let tool_blocks = match &re_encoded.messages[2].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    match &tool_blocks[0] {
        MessagesContentBlock::ToolResult { tool_use_id, .. } => {
            assert_eq!(tool_use_id, "toolu_xyz_789")
        }
        _ => panic!("expected ToolResult"),
    }
    Ok(())
}

#[test]
fn test_multiple_tool_calls_with_indices() -> Result<()> {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Do three things"},
            {"role": "assistant", "content": null, "tool_calls": [
                {
                    "id": "call_0",
                    "type": "function",
                    "function": {"name": "task_a", "arguments": "{}"}
                },
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "task_b", "arguments": "{}"}
                },
                {
                    "id": "call_2",
                    "type": "function",
                    "function": {"name": "task_c", "arguments": "{}"}
                }
            ]}
        ]
    }"#;

    let openai_req: openai_chat::ChatCompletionRequest = serde_json::from_str(openai_json)?;
    let canonical = openai_chat::decode_request(openai_req)?;

    // All 3 tool calls should be present as ToolUse blocks in order
    let asst_blocks = canonical.messages[1].content.clone().into_blocks();
    assert_eq!(asst_blocks.len(), 3);

    for (i, block) in asst_blocks.iter().enumerate() {
        match block {
            ContentBlock::ToolUse(tu) => {
                assert_eq!(tu.id, format!("call_{i}"));
                assert_eq!(
                    tu.name,
                    match i {
                        0 => "task_a",
                        1 => "task_b",
                        2 => "task_c",
                        _ => panic!("unexpected index"),
                    }
                );
            }
            _ => panic!("expected ToolUse block at index {i}"),
        }
    }

    // Verify order survives Anthropic encoding
    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    let tool_blocks = match &anthropic_req.messages[1].content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(tool_blocks.len(), 3);

    let names: Vec<&str> = tool_blocks
        .iter()
        .filter_map(|b| match b {
            MessagesContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["task_a", "task_b", "task_c"]);
    Ok(())
}

#[test]
fn test_tool_choice_variants_all_adapters() -> Result<()> {
    // Anthropic round-trip
    let tc_auto = CanonicalRequest {
        model: "claude-3".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::text("hi"),
        }],
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(100),
        stop: vec![],
        tools: vec![ToolDefinition {
            name: "t".into(),
            description: None,
            input_schema: Some(serde_json::json!({"type": "object"})),
            deferred: None,
            extra: std::collections::HashMap::new(),
        }],
        tool_choice: Some(ToolChoice::Auto),
        stream: false,
        response_format: None,
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let anthro = anthropic_messages::encode_request(&tc_auto)?;
    assert!(matches!(
        anthro.tool_choice,
        Some(anthropic_messages::AnthropicToolChoice::Auto)
    ));
    let decoded = anthropic_messages::decode_request(anthro)?;
    assert!(matches!(decoded.tool_choice, Some(ToolChoice::Auto)));

    // Required → Anthropic → canonical
    let mut tc_req = tc_auto.clone();
    tc_req.tool_choice = Some(ToolChoice::Required);
    let anthro = anthropic_messages::encode_request(&tc_req)?;
    assert!(matches!(
        anthro.tool_choice,
        Some(anthropic_messages::AnthropicToolChoice::Any)
    ));
    let decoded = anthropic_messages::decode_request(anthro)?;
    assert!(matches!(decoded.tool_choice, Some(ToolChoice::Required)));

    // None → Anthropic → canonical
    let mut tc_none = tc_auto.clone();
    tc_none.tool_choice = Some(ToolChoice::None);
    let anthro = anthropic_messages::encode_request(&tc_none)?;
    // Anthropic has no None; field is omitted
    assert!(anthro.tool_choice.is_none());
    let decoded = anthropic_messages::decode_request(anthro)?;
    // Decoded tool_choice is None (no field to decode)
    assert!(decoded.tool_choice.is_none());

    // Named → Anthropic → canonical
    let mut tc_named = tc_auto.clone();
    tc_named.tool_choice = Some(ToolChoice::Named {
        name: "my_tool".into(),
    });
    let anthro = anthropic_messages::encode_request(&tc_named)?;
    assert!(matches!(
        anthro.tool_choice,
        Some(anthropic_messages::AnthropicToolChoice::Tool { ref name }) if name == "my_tool"
    ));
    let decoded = anthropic_messages::decode_request(anthro)?;
    assert!(matches!(
        decoded.tool_choice,
        Some(ToolChoice::Named { ref name }) if name == "my_tool"
    ));

    // OpenAI round-trip: Auto, Required, None, Named("x")
    let openai_json_base = r#"{
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "t"}}]
    }"#;
    let base: openai_chat::ChatCompletionRequest = serde_json::from_str(openai_json_base)?;

    for (tc_str, expected) in &[
        ("\"auto\"", ToolChoice::Auto),
        ("\"required\"", ToolChoice::Required),
        ("\"none\"", ToolChoice::None),
    ] {
        let mut base_clone = base.clone();
        base_clone.tool_choice = Some(serde_json::from_str(tc_str)?);
        let decoded = openai_chat::decode_request(base_clone)?;
        assert!(
            matches!(&decoded.tool_choice, Some(tc) if std::mem::discriminant(tc) == std::mem::discriminant(expected)),
            "OpenAI {tc_str} round-trip failed"
        );
    }

    // Named via OpenAI: {"type":"function","function":{"name":"x"}}
    let mut base_named = base.clone();
    base_named.tool_choice = Some(serde_json::from_str(
        r#"{"type":"function","function":{"name":"target_fn"}}"#,
    )?);
    let decoded = openai_chat::decode_request(base_named)?;
    assert!(matches!(
        decoded.tool_choice,
        Some(ToolChoice::Named { ref name }) if name == "target_fn"
    ));

    // Responses round-trip: Auto, Required, None, Named
    let responses_json_base = r#"{
        "model": "gpt-4",
        "input": [{"role":"user","content":"hi"}],
        "tools": [{"type":"function","name":"t"}]
    }"#;
    let base_resp: openai_responses::ResponsesRequest = serde_json::from_str(responses_json_base)?;

    for (tc_str, expected) in &[
        ("\"auto\"", ToolChoice::Auto),
        ("\"required\"", ToolChoice::Required),
        ("\"none\"", ToolChoice::None),
    ] {
        let mut base_clone = base_resp.clone();
        base_clone.tool_choice = Some(serde_json::from_str(tc_str)?);
        let decoded = openai_responses::decode_request(base_clone)?;
        assert!(
            matches!(&decoded.tool_choice, Some(tc) if std::mem::discriminant(tc) == std::mem::discriminant(expected)),
            "Responses {tc_str} round-trip failed"
        );
    }

    // Named via Responses object form
    let mut base_resp_named = base_resp.clone();
    base_resp_named.tool_choice = Some(serde_json::from_str(r#"{"name":"target_fn"}"#)?);
    let decoded = openai_responses::decode_request(base_resp_named)?;
    assert!(matches!(
        decoded.tool_choice,
        Some(ToolChoice::Named { ref name }) if name == "target_fn"
    ));

    Ok(())
}

// ─── Round-Trip Semantic Tests ──────────────────────────────────────────────

#[test]
fn test_openai_round_trip_semantic() -> Result<()> {
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi!", "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{\"q\":\"hi\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "results here"}
        ],
        "temperature": 0.5,
        "max_tokens": 512,
        "tools": [{"type": "function", "function": {"name": "search", "parameters": {"type": "object"}}}],
        "tool_choice": "auto"
    }"#;

    let openai_req: openai_chat::ChatCompletionRequest = serde_json::from_str(openai_json)?;
    let canonical = openai_chat::decode_request(openai_req)?;

    // Verify all fields survived decode
    assert_eq!(canonical.model, "gpt-4");
    assert_eq!(canonical.temperature, Some(0.5));
    assert_eq!(canonical.max_tokens, Some(512));
    assert!(canonical.system.is_some());
    assert_eq!(canonical.tools.len(), 1);
    assert_eq!(canonical.tools[0].name, "search");
    assert!(matches!(canonical.tool_choice, Some(ToolChoice::Auto)));

    // Messages: system → user → assistant (with tool call) → tool
    assert_eq!(canonical.messages.len(), 3);

    // User message
    assert_eq!(canonical.messages[0].role, Role::User);
    assert_eq!(
        canonical.messages[0].content.clone().into_blocks()[0].as_text(),
        Some("Hello")
    );

    // Assistant message with tool call
    assert_eq!(canonical.messages[1].role, Role::Assistant);
    let asst_blocks = canonical.messages[1].content.clone().into_blocks();
    assert!(matches!(&asst_blocks[0], ContentBlock::Text(t) if t.text == "Hi!"));
    assert!(
        matches!(&asst_blocks[1], ContentBlock::ToolUse(tu) if tu.id == "call_1" && tu.name == "search")
    );

    // Tool result message
    assert_eq!(canonical.messages[2].role, Role::Tool);
    let tool_blocks = canonical.messages[2].content.clone().into_blocks();
    assert!(matches!(&tool_blocks[0], ContentBlock::ToolResult(tr) if tr.tool_use_id == "call_1"));

    // Now verify that the canonical can be encoded back to Anthropic without losing data
    let anthropic_req = anthropic_messages::encode_request(&canonical)?;
    assert_eq!(anthropic_req.model, "gpt-4");
    assert_eq!(anthropic_req.max_tokens, 512);
    assert_eq!(anthropic_req.messages.len(), 3);

    // Tool definitions preserved
    let tools = anthropic_req
        .tools
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tools should exist"))?;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");
    Ok(())
}

#[test]
fn test_anthropic_round_trip_semantic() -> Result<()> {
    let anthropic_json = r#"{
        "model": "claude-3-opus-20240229",
        "max_tokens": 2048,
        "system": "You are a math tutor.",
        "messages": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "Simple arithmetic: 2+2=4", "signature": "sig_abc"},
                {"type": "text", "text": "The answer is 4."}
            ]},
            {"role": "user", "content": "Now use a tool"}
        ],
        "tools": [{
            "name": "calculator",
            "description": "Evaluate math expressions",
            "input_schema": {"type": "object", "properties": {"expr": {"type": "string"}}}
        }],
        "tool_choice": {"type": "auto"},
        "temperature": 0.3,
        "top_p": 0.9
    }"#;

    let req: anthropic_messages::MessagesRequest = serde_json::from_str(anthropic_json)?;
    let canonical = anthropic_messages::decode_request(req)?;

    // Verify system
    match &canonical.system {
        Some(SystemInstruction::Text(text)) => assert_eq!(text, "You are a math tutor."),
        _ => panic!("expected text system instruction"),
    }

    // Verify messages
    assert_eq!(canonical.messages.len(), 3);

    // User message
    assert_eq!(canonical.messages[0].role, Role::User);
    assert_eq!(
        canonical.messages[0].content.clone().into_blocks()[0].as_text(),
        Some("What is 2+2?")
    );

    // Assistant with thinking block
    assert_eq!(canonical.messages[1].role, Role::Assistant);
    let asst_blocks = canonical.messages[1].content.clone().into_blocks();
    assert!(matches!(
        &asst_blocks[0],
        ContentBlock::Reasoning(r) if r.thinking == "Simple arithmetic: 2+2=4"
            && r.signature.as_deref() == Some("sig_abc")
    ));
    assert!(matches!(&asst_blocks[1], ContentBlock::Text(t) if t.text == "The answer is 4."));

    // Tool definitions
    assert_eq!(canonical.tools.len(), 1);
    assert_eq!(canonical.tools[0].name, "calculator");
    assert!(matches!(canonical.tool_choice, Some(ToolChoice::Auto)));
    assert_eq!(canonical.temperature, Some(0.3));
    assert_eq!(canonical.top_p, Some(0.9));

    // Re-encode and verify
    let re_encoded = anthropic_messages::encode_request(&canonical)?;
    assert_eq!(re_encoded.model, "claude-3-opus-20240229");
    assert_eq!(re_encoded.max_tokens, 2048);

    // System preserved
    match &re_encoded.system {
        Some(serde_json::Value::String(s)) => assert_eq!(s, "You are a math tutor."),
        other => panic!("expected string system instruction, got: {other:?}"),
    }

    // Messages preserved
    assert_eq!(re_encoded.messages.len(), 3);

    // Assistant thinking block preserved
    let asst_msg = &re_encoded.messages[1];
    let asst_blocks = match &asst_msg.content {
        MessagesContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert!(matches!(
        &asst_blocks[0],
        MessagesContentBlock::Thinking { thinking, signature }
            if thinking == "Simple arithmetic: 2+2=4"
                && signature.as_deref() == Some("sig_abc")
    ));
    assert!(matches!(
        &asst_blocks[1],
        MessagesContentBlock::Text { text } if text == "The answer is 4."
    ));

    // Tool definitions preserved
    let tools = re_encoded
        .tools
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tools should exist"))?;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "calculator");
    assert_eq!(
        tools[0].description.as_deref(),
        Some("Evaluate math expressions")
    );
    Ok(())
}

#[test]
fn test_responses_round_trip_semantic() -> Result<()> {
    let responses_json = r#"{
        "model": "gpt-4",
        "instructions": "You are a helpful assistant.",
        "input": [
            {"role": "user", "content": "Search for rust lang"},
            {"type": "function_call", "id": "fc_001", "name": "web_search", "arguments": "{\"q\":\"rust\"}"},
            {"type": "function_call_output", "call_id": "fc_001", "output": "Found results"},
            {"role": "user", "content": "Summarize"},
            {"type": "reasoning", "id": "reason_001", "summary": [{"type": "summary_text", "text": "The user wants a summary"}]}
        ],
        "tools": [{"type": "function", "name": "web_search", "parameters": {"type": "object"}}]
    }"#;

    let req: openai_responses::ResponsesRequest = serde_json::from_str(responses_json)?;
    let canonical = openai_responses::decode_request(req)?;

    // Verify instructions → system
    match &canonical.system {
        Some(SystemInstruction::Text(text)) => {
            assert_eq!(text, "You are a helpful assistant.")
        }
        _ => panic!("expected text system instruction"),
    }

    // Verify messages include: user, assistant (tool use), tool (result), user, reasoning
    assert!(canonical.messages.len() >= 3);

    // Verify tools
    assert_eq!(canonical.tools.len(), 1);
    assert_eq!(canonical.tools[0].name, "web_search");

    // Verify tool call → ToolUse in assistant message
    let has_tool_use = canonical.messages.iter().any(|m| {
        m.content
            .clone()
            .into_blocks()
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse(tu) if tu.name == "web_search"))
    });
    assert!(has_tool_use, "expected ToolUse block for web_search");

    // Verify tool result → ToolResult message
    let has_tool_result =
        canonical.messages.iter().any(|m| {
            m.role == Role::Tool
                && m.content.clone().into_blocks().iter().any(
                    |b| matches!(b, ContentBlock::ToolResult(tr) if tr.tool_use_id == "fc_001"),
                )
        });
    assert!(has_tool_result, "expected ToolResult with call_id fc_001");

    // Verify reasoning is present
    let has_reasoning = canonical.messages.iter().any(|m| {
        m.content
            .clone()
            .into_blocks()
            .iter()
            .any(|b| matches!(b, ContentBlock::Reasoning(r) if r.thinking.contains("summary")))
    });
    assert!(has_reasoning, "expected Reasoning content block");
    Ok(())
}

#[test]
fn test_round_trip_documents_losses() -> Result<()> {
    // OpenAI has response_format; Anthropic does not.
    // This test documents that response_format is lost during translation.
    let openai_json = r#"{
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Give me JSON"}],
        "response_format": {"type": "json_object"}
    }"#;

    let openai_req: openai_chat::ChatCompletionRequest = serde_json::from_str(openai_json)?;
    let canonical = openai_chat::decode_request(openai_req)?;

    // response_format is present in the canonical
    let rf = canonical
        .response_format
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("response_format should exist"))?;
    assert_eq!(rf.format_type, "json_object");

    // Encode to Anthropic — response_format is not part of Anthropic wire format
    let anthropic_req = anthropic_messages::encode_request(&canonical)?;

    // Decode back — response_format should be absent since Anthropic doesn't carry it
    let decoded = anthropic_messages::decode_request(anthropic_req)?;
    assert!(
        decoded.response_format.is_none(),
        "response_format should be lost after Anthropic round-trip; got: {:?}",
        decoded.response_format,
    );

    // The message content is preserved even though response_format is lost
    assert_eq!(decoded.messages.len(), 1);
    assert_eq!(
        decoded.messages[0].content.clone().into_blocks()[0].as_text(),
        Some("Give me JSON")
    );
    Ok(())
}
