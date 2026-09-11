//! Canonical model and error type tests.
//!
//! Tests serialization round-trips, type construction, capability methods,
//! and error module coverage.

use protocol_core::canonical::*;
use protocol_core::error::ProtocolEngineError;

// ─── Canonical type serde round-trips ────────────────────────────────────────

#[test]
fn test_canonical_request_serde_round_trip() {
    let req = CanonicalRequest {
        model: "gpt-4".into(),
        messages: vec![
            Message {
                role: Role::System,
                content: MessageContent::text("Be helpful."),
            },
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text(TextContent {
                        text: "Hello!".into(),
                    }),
                    ContentBlock::Image(ImageContent {
                        source: ImageSource::Url {
                            url: "https://example.com/img.png".into(),
                            detail: Some("high".into()),
                        },
                    }),
                    ContentBlock::Audio(AudioContent {
                        source: AudioSource::Base64 {
                            media_type: "audio/wav".into(),
                            data: "AAAA".into(),
                            format: Some("wav".into()),
                        },
                    }),
                    ContentBlock::Reasoning(ReasoningContent {
                        thinking: "Internal thought.".into(),
                        signature: Some("sig123".into()),
                    }),
                ]),
            },
        ],
        system: Some(SystemInstruction::Text("Be helpful.".into())),
        temperature: Some(0.7),
        top_p: None,
        max_tokens: Some(1024),
        stop: vec!["STOP".into()],
        tools: vec![ToolDefinition {
            name: "search".into(),
            description: Some("Search the web".into()),
            input_schema: Some(serde_json::json!({"type": "object"})),
            deferred: None,
            extra: Default::default(),
        }],
        tool_choice: Some(ToolChoice::Auto),
        stream: true,
        response_format: Some(ResponseFormat {
            format_type: "json_object".into(),
            json_schema: None,
        }),
        metadata: None,
        extensions: ProviderExtensions::default(),
    };

    let json = match serde_json::to_string(&req) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let decoded: CanonicalRequest = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(decoded.model, "gpt-4");
    assert_eq!(decoded.messages.len(), 2);
    assert!(decoded.stream);
}

#[test]
fn test_canonical_response_serde_round_trip() {
    let resp = CanonicalResponse {
        id: "resp_1".into(),
        model: "gpt-4".into(),
        content: vec![
            ContentBlock::Text(TextContent {
                text: "Hello!".into(),
            }),
            ContentBlock::Reasoning(ReasoningContent {
                thinking: "Thought process.".into(),
                signature: None,
            }),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "call_1".into(),
                name: "search".into(),
                input: serde_json::json!({"q": "test"}),
            }),
        ],
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage::default()),
        extensions: ProviderExtensions::default(),
    };

    let json = match serde_json::to_string(&resp) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    let decoded: CanonicalResponse = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(decoded.content.len(), 3);
    assert_eq!(decoded.content[0].as_text(), Some("Hello!"));
}

#[test]
fn test_stream_event_serde_round_trip() {
    let events = vec![
        CanonicalStreamEvent::MessageStart {
            message: StreamMessageInfo {
                id: "msg_1".into(),
                model: "gpt-4".into(),
                role: Role::Assistant,
            },
        },
        CanonicalStreamEvent::TextDelta {
            index: 0,
            text: "Hello".into(),
        },
        CanonicalStreamEvent::AudioDelta {
            index: 1,
            data: "base64audio".into(),
        },
        CanonicalStreamEvent::ReasoningDelta {
            index: 0,
            thinking: "Thinking...".into(),
        },
        CanonicalStreamEvent::ReasoningSignature {
            index: 0,
            signature: "sig_abc".into(),
        },
        CanonicalStreamEvent::ToolCallDelta {
            index: 0,
            tool_use_id: Some("call_1".into()),
            name: Some("search".into()),
            input_json_delta: None,
        },
        CanonicalStreamEvent::MessageStop,
        CanonicalStreamEvent::Ping,
    ];

    for event in &events {
        let json = match serde_json::to_string(event) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let decoded: CanonicalStreamEvent = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let re_encoded = match serde_json::to_string(&decoded) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(json, re_encoded);
    }
}

// ─── Capability tests ────────────────────────────────────────────────────────

#[test]
fn test_capabilities_default_all_false() {
    let caps = ProtocolCapabilities::default();
    assert!(!caps.streaming);
    assert!(!caps.tools);
    assert!(!caps.tool_streaming);
    assert!(!caps.multimodal_input);
    assert!(!caps.structured_output);
    assert!(!caps.reasoning);
    assert!(!caps.usage_streaming);
    assert!(!caps.deferred_tools);
}

#[test]
fn test_translation_losses_all_branches() {
    let mut source = ProtocolCapabilities::default();
    let target = ProtocolCapabilities::default();

    source.tool_streaming = true;
    source.structured_output = true;
    source.reasoning = true;
    source.deferred_tools = true;

    let losses = target.translation_losses(&source);
    assert_eq!(losses.len(), 4);
    assert!(losses.iter().any(|l| l.feature == "tool_call_streaming"));
    assert!(losses.iter().any(|l| l.feature == "structured_output"));
    assert!(losses.iter().any(|l| l.feature == "reasoning"));
    assert!(losses.iter().any(|l| l.feature == "deferred_tools"));
}

#[test]
fn test_loss_policy_variants() {
    let policies = [
        LossPolicy::Reject,
        LossPolicy::Warn,
        LossPolicy::Drop,
        LossPolicy::Approximate,
        LossPolicy::EncodeAsExtension,
    ];
    for policy in &policies {
        let json = match serde_json::to_string(policy) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let decoded: LossPolicy = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(*policy, decoded);
    }
}

// ─── Error module tests ──────────────────────────────────────────────────────

#[test]
fn test_error_status_codes() {
    let cases = vec![
        (
            ProtocolEngineError::UnsupportedProtocol {
                protocol: "test".into(),
            },
            400,
        ),
        (
            ProtocolEngineError::InvalidPayload {
                message: "bad".into(),
            },
            400,
        ),
        (
            ProtocolEngineError::UnsupportedFeature {
                feature: "x".into(),
                reason: "y".into(),
            },
            501,
        ),
        (
            ProtocolEngineError::InvalidStreamEvent {
                message: "bad".into(),
            },
            502,
        ),
        (
            ProtocolEngineError::TranslationFailure {
                message: "fail".into(),
            },
            502,
        ),
        (
            ProtocolEngineError::LossyTranslation {
                feature: "x".into(),
                reason: "y".into(),
                policy: LossPolicy::Reject,
            },
            400,
        ),
        (
            ProtocolEngineError::LossyTranslation {
                feature: "x".into(),
                reason: "y".into(),
                policy: LossPolicy::Warn,
            },
            200,
        ),
        (
            ProtocolEngineError::ProviderError {
                message: "upstream".into(),
            },
            502,
        ),
        (ProtocolEngineError::Internal("bug".into()), 500),
    ];

    for (error, expected_status) in cases {
        let status = error.status_code();
        assert_eq!(status.as_u16(), expected_status, "for error: {error}");
    }
}

#[test]
fn test_error_categories() {
    let cases = vec![
        (
            ProtocolEngineError::UnsupportedProtocol {
                protocol: "test".into(),
            },
            "unsupported_protocol",
        ),
        (
            ProtocolEngineError::InvalidPayload {
                message: "bad".into(),
            },
            "invalid_payload",
        ),
        (
            ProtocolEngineError::UnsupportedFeature {
                feature: "x".into(),
                reason: "y".into(),
            },
            "unsupported_feature",
        ),
        (
            ProtocolEngineError::InvalidStreamEvent {
                message: "bad".into(),
            },
            "invalid_stream_event",
        ),
        (
            ProtocolEngineError::TranslationFailure {
                message: "fail".into(),
            },
            "translation_failure",
        ),
        (
            ProtocolEngineError::ProviderError {
                message: "upstream".into(),
            },
            "provider_error",
        ),
        (ProtocolEngineError::Internal("bug".into()), "internal"),
    ];

    for (error, expected_category) in cases {
        assert_eq!(error.category(), expected_category, "for error: {error}");
    }
}

#[test]
fn test_error_json_body() {
    let error = ProtocolEngineError::InvalidPayload {
        message: "bad input".into(),
    };
    let body = error.to_json_body();
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(parsed["error"]["type"], "invalid_payload");
    let message = match parsed["error"]["message"].as_str() {
        Some(s) => s,
        None => panic!("expected message string"),
    };
    assert!(message.contains("bad input"));
    assert_eq!(parsed["error"]["status"], 400);
}

// ─── Protocol Display tests ──────────────────────────────────────────────────

#[test]
fn test_protocol_display() {
    assert_eq!(
        Protocol::OpenAiChatCompletions.to_string(),
        "openai_chat_completions"
    );
    assert_eq!(
        Protocol::AnthropicMessages.to_string(),
        "anthropic_messages"
    );
    assert_eq!(Protocol::OpenAiResponses.to_string(), "openai_responses");
}

#[test]
fn test_finish_reason_display() {
    assert_eq!(FinishReason::Stop.to_string(), "stop");
    assert_eq!(FinishReason::Length.to_string(), "length");
    assert_eq!(FinishReason::ToolCalls.to_string(), "tool_calls");
    assert_eq!(FinishReason::Other("custom".into()).to_string(), "custom");
}

// ─── MessageContent helper tests ─────────────────────────────────────────────

#[test]
fn test_message_content_helpers() {
    let text = MessageContent::text("hello");
    assert!(!text.is_empty());
    assert_eq!(text.clone().into_blocks().len(), 1);

    let blocks = MessageContent::Blocks(vec![]);
    assert!(blocks.is_empty());
}

#[test]
fn test_tool_reference_preserves_fields() {
    let tr = ToolReference {
        id: "ref_1".into(),
        name: "search".into(),
        description: Some("Search tool".into()),
        input_schema: Some(serde_json::json!({"type": "object"})),
        deferred: true,
    };
    let json = match serde_json::to_string(&tr) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert!(json.contains("ref_1"));
    assert!(json.contains("search"));
    assert!(json.contains("true")); // deferred = true

    let decoded: ToolReference = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => panic!("expected success: {e:?}"),
    };
    assert_eq!(decoded.id, "ref_1");
    assert!(decoded.deferred);
    assert!(decoded.input_schema.is_some());
}
