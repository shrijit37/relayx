//! OpenAI Chat Completions adapter.
//!
//! Translates between the OpenAI Chat Completions wire format and the canonical
//! model. Handles both streaming and non-streaming requests/responses.
//!
//! # Wire format
//!
//! Request body (`POST /v1/chat/completions`):
//!
//! ```json
//! {
//!   "model": "gpt-4",
//!   "messages": [
//!     {"role": "system", "content": "You are helpful."},
//!     {"role": "user", "content": "Hello!"},
//!     {"role": "assistant", "content": "Hi!", "tool_calls": [...]},
//!     {"role": "tool", "tool_call_id": "call_123", "content": "42"}
//!   ],
//!   "temperature": 0.7,
//!   "max_tokens": 1024,
//!   "stream": true
//! }
//! ```
//!
//! Response body (non-streaming):
//!
//! ```json
//! {
//!   "id": "chatcmpl-abc",
//!   "object": "chat.completion",
//!   "model": "gpt-4",
//!   "choices": [{
//!     "index": 0,
//!     "message": {"role": "assistant", "content": "Hello!"},
//!     "finish_reason": "stop"
//!   }],
//!   "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
//! }
//! ```

mod decode;
mod encode;
mod wire;

pub use decode::{decode_request, decode_response};
pub use encode::{encode_request, encode_response, encode_stream_event};
pub use wire::*;

use crate::canonical::{Protocol, ProtocolCapabilities};

/// Return the protocol capabilities for OpenAI Chat Completions.
pub fn capabilities() -> ProtocolCapabilities {
    ProtocolCapabilities {
        streaming: true,
        tools: true,
        tool_streaming: true,
        multimodal_input: true,
        structured_output: true,
        reasoning: false,
        usage_streaming: true,
        deferred_tools: false,
    }
}

/// Return the protocol identifier.
pub fn protocol() -> Protocol {
    Protocol::OpenAiChatCompletions
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::*;

    #[test]
    fn test_decode_simple_request() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello!"}
            ]
        }"#;
        let req = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let canonical = match decode_request(req) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };

        assert_eq!(canonical.model, "gpt-4");
        assert!(canonical.system.is_some());
        assert_eq!(canonical.messages.len(), 1);
        assert_eq!(canonical.messages[0].role, Role::User);
    }

    #[test]
    fn test_decode_multimodal_content() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/img.png", "detail": "high"}}
                ]
            }]
        }"#;
        let req = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let canonical = match decode_request(req) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };

        assert_eq!(canonical.messages.len(), 1);
        let blocks = canonical.messages[0].content.clone().into_blocks();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text(t) if t.text == "What is this?"));
        assert!(
            matches!(&blocks[1], ContentBlock::Image(ImageContent { source: ImageSource::Url { url, detail: Some(d), .. } })
            if url == "https://example.com/img.png" && d == "high")
        );
    }

    #[test]
    fn test_decode_tool_calls_in_assistant_message() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [{
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
                }]
            }]
        }"#;
        let req = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let canonical = match decode_request(req) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };

        let blocks = canonical.messages[0].content.clone().into_blocks();
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse(tu) = &blocks[0] {
            assert_eq!(tu.id, "call_abc");
            assert_eq!(tu.name, "get_weather");
            assert_eq!(tu.input["city"], "NYC");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn test_decode_tool_result_message() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_abc",
                "name": "get_weather",
                "content": "{\"temp\": 72}"
            }]
        }"#;
        let req = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let canonical = match decode_request(req) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };

        let blocks = canonical.messages[0].content.clone().into_blocks();
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolResult(tr) = &blocks[0] {
            assert_eq!(tr.tool_use_id, "call_abc");
            assert_eq!(tr.name.as_deref(), Some("get_weather"));
        } else {
            panic!("expected ToolResult block");
        }
    }

    #[test]
    fn test_decode_tool_choice_auto() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": "auto"
        }"#;
        let req = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let canonical = match decode_request(req) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert!(matches!(canonical.tool_choice, Some(ToolChoice::Auto)));
    }

    #[test]
    fn test_encode_simple_response() {
        let resp = CanonicalResponse {
            id: "chatcmpl-123".into(),
            model: "gpt-4".into(),
            content: vec![ContentBlock::Text(TextContent {
                text: "Hello there!".into(),
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

        let openai_resp = match encode_response(&resp, 1234567890) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(openai_resp.id, "chatcmpl-123");
        assert_eq!(openai_resp.choices.len(), 1);
        assert_eq!(
            openai_resp.choices[0].message.content.as_deref(),
            Some("Hello there!")
        );
        assert_eq!(
            openai_resp.choices[0].finish_reason.as_deref(),
            Some("stop")
        );
        let usage = match openai_resp.usage.as_ref() {
            Some(u) => u,
            None => panic!("expected usage"),
        };
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_encode_tool_call_response() {
        let resp = CanonicalResponse {
            id: "chatcmpl-456".into(),
            model: "gpt-4".into(),
            content: vec![ContentBlock::ToolUse(ToolUseBlock {
                id: "call_xyz".into(),
                name: "search".into(),
                input: serde_json::json!({"query": "rust async"}),
            })],
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
            extensions: ProviderExtensions::default(),
        };

        let openai_resp = match encode_response(&resp, 1234567890) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(openai_resp.choices.len(), 1);
        let tool_calls = match openai_resp.choices[0].message.tool_calls.as_ref() {
            Some(tc) => tc,
            None => panic!("expected tool_calls"),
        };
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_xyz");
        assert_eq!(tool_calls[0].function.name, "search");
        assert_eq!(
            openai_resp.choices[0].finish_reason.as_deref(),
            Some("tool_calls")
        );
    }

    /// Encode a full tool-call round-trip: user → assistant (tool_calls) →
    /// tool (results). The tool-role message must appear exactly once, with a
    /// valid `tool_call_id`, and no malformed duplicate.
    #[test]
    fn test_encode_request_tool_result_round_trip() {
        let canonical = CanonicalRequest {
            model: "gpt-4".into(),
            system: Some(SystemInstruction::Text("You are helpful.".into())),
            messages: vec![
                // user
                Message {
                    role: Role::User,
                    content: MessageContent::Text("What's the weather?".into()),
                },
                // assistant with tool_calls
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolUse(ToolUseBlock {
                        id: "call_abc".into(),
                        name: "get_weather".into(),
                        input: serde_json::json!({"city": "NYC"}),
                    })]),
                },
                // tool result
                Message {
                    role: Role::Tool,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult(
                        ToolResultBlock {
                            tool_use_id: "call_abc".into(),
                            name: Some("get_weather".into()),
                            content: ToolResultContent::Text(r#"{"temp":72}"#.into()),
                            is_error: None,
                        },
                    )]),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: vec![],
            tools: vec![ToolDefinition {
                name: "get_weather".into(),
                description: Some("Get the weather for a city".into()),
                input_schema: Some(serde_json::json!({"type": "object"})),
                deferred: None,
                extra: Default::default(),
            }],
            tool_choice: Some(ToolChoice::Auto),
            stream: false,
            response_format: None,
            metadata: None,
            extensions: ProviderExtensions::default(),
        };

        let wire = match encode_request(&canonical) {
            Ok(w) => w,
            Err(e) => panic!("encode should succeed: {e:?}"),
        };
        let messages = &wire.messages;

        // system (1) + user (1) + assistant with tool_calls (1) + tool with tool_call_id (1) = 4
        assert_eq!(
            messages.len(),
            4,
            "expected exactly 4 messages; got {}: {:?}",
            messages.len(),
            messages
                .iter()
                .map(|m| (&m.role, m.tool_call_id.as_deref()))
                .collect::<Vec<_>>()
        );

        // assistant message carries tool_calls
        let assistant = &messages[2];
        assert_eq!(assistant.role, "assistant");
        let tc = match assistant.tool_calls.as_ref() {
            Some(tc) => tc,
            None => panic!("assistant should have tool_calls"),
        };
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_abc");

        // tool message carries the tool_call_id
        let tool_msg = &messages[3];
        assert_eq!(tool_msg.role, "tool");
        assert_eq!(
            tool_msg.tool_call_id.as_deref(),
            Some("call_abc"),
            "tool message must carry tool_call_id"
        );
        let content_text = match tool_msg.content.as_ref().and_then(|c| c.as_str()) {
            Some(s) => s,
            None => panic!("tool message content should be a string"),
        };
        assert!(
            content_text.contains("72"),
            "tool content should carry result payload"
        );
    }

    /// A tool-role message with no tool_call_id must never appear in the
    /// encoded output — that would be an API error from the provider.
    #[test]
    fn test_encode_request_no_malformed_tool_message() {
        let canonical = CanonicalRequest {
            model: "gpt-4".into(),
            system: None,
            messages: vec![Message {
                role: Role::Tool,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult(ToolResultBlock {
                    tool_use_id: "call_777".into(),
                    name: None,
                    content: ToolResultContent::Text("ok".into()),
                    is_error: None,
                })]),
            }],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: vec![],
            stream: false,
            response_format: None,
            metadata: None,
            extensions: ProviderExtensions::default(),
        };

        let wire = match encode_request(&canonical) {
            Ok(w) => w,
            Err(e) => panic!("encode should succeed: {e:?}"),
        };

        // Exactly one tool message, with the correct tool_call_id.
        assert_eq!(
            wire.messages.len(),
            1,
            "tool-role message should appear exactly once"
        );
        assert_eq!(wire.messages[0].role, "tool");
        assert_eq!(
            wire.messages[0].tool_call_id.as_deref(),
            Some("call_777"),
            "tool message must have tool_call_id (no malformed duplicate)"
        );
    }
}
