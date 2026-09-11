//! Anthropic Messages adapter.
//!
//! Translates between the Anthropic Messages wire format and the canonical model.
//!
//! # Key differences from OpenAI
//!
//! - System instructions are a top-level field, not a message.
//! - Messages use content block arrays (not a union of string/array for content).
//! - Tool definitions use `input_schema` (not `parameters`).
//! - Tool calls are content blocks within assistant messages.
//! - Tool results are content blocks within user messages.
//! - Streaming uses `content_block_start`/`content_block_delta`/`content_block_stop`.
//! - `stop_reason` instead of `finish_reason`.

use crate::canonical::*;
use crate::error::ProtocolEngineError;

// ─── Wire types (Anthropic Messages) ─────────────────────────────────────────

/// Top-level Anthropic Messages request body.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<serde_json::Value>,
    pub messages: Vec<MessagesMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// An Anthropic message.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MessagesMessage {
    pub role: String,
    pub content: MessagesContent,
}

/// Content of an Anthropic message — always an array of content blocks.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum MessagesContent {
    /// Simple text (string shorthand).
    Text(String),
    /// Array of content blocks.
    Blocks(Vec<MessagesContentBlock>),
}

/// A single content block in an Anthropic message.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagesContentBlock {
    Text {
        text: String,
    },
    Image {
        source: MessagesImageSource,
    },
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Anthropic image source.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MessagesImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Anthropic tool definition.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AnthropicToolDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Anthropic tool choice control.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

// ─── Response wire types ─────────────────────────────────────────────────────

/// Top-level Anthropic Messages response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessagesResponse {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<MessagesResponseBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: MessagesUsage,
}

/// A content block in the response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagesResponseBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Anthropic usage information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessagesUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

// ─── Streaming wire types ────────────────────────────────────────────────────

/// An Anthropic streaming event.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagesStreamEvent {
    MessageStart {
        message: MessagesStreamMessageInfo,
    },
    ContentBlockStart {
        index: usize,
        content_block: MessagesResponseBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: MessagesDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessagesMessageDelta,
        usage: MessagesUsage,
    },
    Ping,
    MessageStop,
    Error {
        error: MessagesErrorDetail,
    },
}

/// Metadata about the message at stream start.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessagesStreamMessageInfo {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<serde_json::Value>,
    pub stop_reason: Option<String>,
    pub usage: MessagesUsage,
}

/// A delta within a content block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagesDelta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
    InputJsonDelta { partial_json: String },
    SignatureDelta { signature: String },
}

/// Message-level delta (stop reason).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessagesMessageDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

/// Error detail in streaming.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessagesErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// ─── Adapter: Request encoding (canonical → Anthropic) ───────────────────────

/// Encode a canonical request into an Anthropic Messages request.
pub fn encode_request(req: &CanonicalRequest) -> Result<MessagesRequest, ProtocolEngineError> {
    let system = encode_system_instruction(&req.system);
    let messages = encode_request_messages(&req.messages);
    let (tools, tool_choice) = encode_tools_and_choice(&req.tools, &req.tool_choice);

    if req.response_format.is_some() {
        tracing::info!(
            "response_format specified but Anthropic has no direct equivalent; \
             structured output will be handled via system instructions"
        );
    }

    Ok(MessagesRequest {
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(4096),
        system,
        messages,
        temperature: req.temperature,
        top_p: req.top_p,
        stop_sequences: if req.stop.is_empty() {
            None
        } else {
            Some(req.stop.clone())
        },
        tools,
        tool_choice,
        stream: req.stream,
        metadata: req.metadata.clone(),
        extra: Default::default(),
    })
}

/// Encode a canonical system instruction into Anthropic's system field format.
fn encode_system_instruction(system: &Option<SystemInstruction>) -> Option<serde_json::Value> {
    system.as_ref().map(|s| match s {
        SystemInstruction::Text(text) => serde_json::Value::String(text.clone()),
        SystemInstruction::Blocks(blocks) => {
            let json_blocks: Vec<serde_json::Value> = blocks
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "type": "text",
                        "text": b.text
                    })
                })
                .collect();
            serde_json::Value::Array(json_blocks)
        }
    })
}

/// Encode canonical messages into Anthropic messages.
fn encode_request_messages(messages: &[Message]) -> Vec<MessagesMessage> {
    let mut result = Vec::with_capacity(messages.len());

    for msg in messages {
        match msg.role {
            Role::System => {
                tracing::warn!("system message in messages array; should be in system field");
            }
            Role::User | Role::Assistant => {
                let blocks = msg.content.clone().into_blocks();
                let anthro_blocks = encode_content_blocks(&blocks);
                let role = match msg.role {
                    Role::User => "user",
                    _ => "assistant",
                };
                result.push(MessagesMessage {
                    role: role.into(),
                    content: MessagesContent::Blocks(anthro_blocks),
                });
            }
            Role::Tool => {
                let tool_msg = encode_tool_result_message(&msg.content);
                if let Some(m) = tool_msg {
                    result.push(m);
                }
            }
        }
    }

    result
}

/// Encode a canonical tool result message into an Anthropic user message.
fn encode_tool_result_message(content: &MessageContent) -> Option<MessagesMessage> {
    let blocks = content.clone().into_blocks();
    let tool_result_blocks: Vec<MessagesContentBlock> = blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlock::ToolResult(tr) = b {
                let tool_content = match &tr.content {
                    ToolResultContent::Text(s) => Some(serde_json::Value::String(s.clone())),
                    ToolResultContent::Blocks(blocks) => {
                        let texts: Vec<String> = blocks
                            .iter()
                            .filter_map(|b| b.as_text().map(|s| s.to_owned()))
                            .collect();
                        Some(serde_json::Value::String(texts.join("\n")))
                    }
                };
                Some(MessagesContentBlock::ToolResult {
                    tool_use_id: tr.tool_use_id.clone(),
                    content: tool_content,
                    is_error: tr.is_error,
                })
            } else {
                None
            }
        })
        .collect();

    if tool_result_blocks.is_empty() {
        None
    } else {
        Some(MessagesMessage {
            role: "user".into(),
            content: MessagesContent::Blocks(tool_result_blocks),
        })
    }
}

/// Encode tool definitions and tool choice into Anthropic format.
///
/// Preserves tool definitions even when `ToolChoice::None` is set.
/// Anthropic has no direct `none` tool_choice; omitting the field defaults
/// to auto-selection. The tools are kept so they're available if the
/// downstream prompt instructs the model not to use them.
fn encode_tools_and_choice(
    tools: &[ToolDefinition],
    tool_choice: &Option<ToolChoice>,
) -> (
    Option<Vec<AnthropicToolDefinition>>,
    Option<AnthropicToolChoice>,
) {
    let tools = if tools.is_empty() {
        None
    } else {
        Some(
            tools
                .iter()
                .map(|t| AnthropicToolDefinition {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t
                        .input_schema
                        .clone()
                        .unwrap_or(serde_json::json!({"type": "object"})),
                })
                .collect(),
        )
    };

    match tool_choice.as_ref() {
        // Anthropic has no `none` tool_choice — keep tools, omit the field.
        Some(ToolChoice::None) => (tools, None),
        Some(tc) => {
            let anthro_tc = match tc {
                ToolChoice::Auto => AnthropicToolChoice::Auto,
                ToolChoice::Required => AnthropicToolChoice::Any,
                ToolChoice::None => unreachable!(),
                ToolChoice::Named { name } => AnthropicToolChoice::Tool { name: name.clone() },
            };
            (tools, Some(anthro_tc))
        }
        None => (tools, None),
    }
}

/// Encode canonical content blocks into Anthropic content blocks.
fn encode_content_blocks(blocks: &[ContentBlock]) -> Vec<MessagesContentBlock> {
    let mut result = Vec::with_capacity(blocks.len());

    for block in blocks {
        match block {
            ContentBlock::Text(t) => {
                result.push(MessagesContentBlock::Text {
                    text: t.text.clone(),
                });
            }
            ContentBlock::Image(img) => {
                let source = match &img.source {
                    ImageSource::Url { url, .. } => MessagesImageSource {
                        source_type: "url".into(),
                        media_type: None,
                        data: None,
                        url: Some(url.clone()),
                    },
                    ImageSource::Base64 { media_type, data } => MessagesImageSource {
                        source_type: "base64".into(),
                        media_type: Some(media_type.clone()),
                        data: Some(data.clone()),
                        url: None,
                    },
                };
                result.push(MessagesContentBlock::Image { source });
            }
            ContentBlock::Audio(audio) => {
                // Anthropic doesn't support audio input; approximate as base64 source
                // when possible, otherwise log and skip.
                match &audio.source {
                    AudioSource::Base64 {
                        media_type, data, ..
                    } => {
                        result.push(MessagesContentBlock::Image {
                            source: MessagesImageSource {
                                source_type: "base64".into(),
                                media_type: Some(media_type.clone()),
                                data: Some(data.clone()),
                                url: None,
                            },
                        });
                    }
                    AudioSource::Url { url } => {
                        tracing::warn!(
                            url = %url,
                            "audio URL cannot be represented in Anthropic; skipping"
                        );
                    }
                }
            }
            ContentBlock::Reasoning(thinking) => {
                result.push(MessagesContentBlock::Thinking {
                    thinking: thinking.thinking.clone(),
                    signature: thinking.signature.clone(),
                });
            }
            ContentBlock::ToolUse(tu) => {
                result.push(MessagesContentBlock::ToolUse {
                    id: tu.id.clone(),
                    name: tu.name.clone(),
                    input: tu.input.clone(),
                });
            }
            ContentBlock::ToolResult(tr) => {
                let content = match &tr.content {
                    ToolResultContent::Text(s) => Some(serde_json::Value::String(s.clone())),
                    ToolResultContent::Blocks(blocks) => {
                        let texts: Vec<String> = blocks
                            .iter()
                            .filter_map(|b| b.as_text().map(|s| s.to_owned()))
                            .collect();
                        if texts.is_empty() {
                            None
                        } else {
                            Some(serde_json::Value::String(texts.join("\n")))
                        }
                    }
                };
                result.push(MessagesContentBlock::ToolResult {
                    tool_use_id: tr.tool_use_id.clone(),
                    content,
                    is_error: tr.is_error,
                });
            }
            ContentBlock::ToolReference(tr) => {
                // Anthropic Messages API doesn't have deferred tool references.
                // Materialize as a ToolUse block when a schema is available;
                // otherwise preserve as extension metadata for downstream use.
                if tr.input_schema.is_some() && !tr.deferred {
                    tracing::info!(
                        tool_id = %tr.id,
                        tool_name = %tr.name,
                        "materializing non-deferred tool reference as ToolUse"
                    );
                    result.push(MessagesContentBlock::ToolUse {
                        id: tr.id.clone(),
                        name: tr.name.clone(),
                        input: tr
                            .input_schema
                            .clone()
                            .unwrap_or(serde_json::json!({"type": "object"})),
                    });
                } else {
                    tracing::warn!(
                        tool_id = %tr.id,
                        tool_name = %tr.name,
                        deferred = %tr.deferred,
                        "deferred tool reference cannot be materialized in Anthropic; \
                         preserving as extension metadata"
                    );
                    // Include as a structured tool definition in a Text block so the
                    // information is preserved in the conversation.
                    let metadata = serde_json::json!({
                        "type": "tool_reference",
                        "id": tr.id,
                        "name": tr.name,
                        "description": tr.description,
                        "deferred": tr.deferred,
                    });
                    result.push(MessagesContentBlock::Text {
                        text: serde_json::to_string(&metadata)
                            .unwrap_or_else(|_| format!("[tool reference: {}]", tr.name)),
                    });
                }
            }
        }
    }

    result
}

// ─── Adapter: Response decoding (Anthropic → canonical) ──────────────────────

/// Decode an Anthropic Messages response into a canonical response.
pub fn decode_response(resp: MessagesResponse) -> Result<CanonicalResponse, ProtocolEngineError> {
    let mut content = Vec::with_capacity(resp.content.len());

    for block in resp.content {
        match block {
            MessagesResponseBlock::Text { text } => {
                content.push(ContentBlock::Text(TextContent { text }));
            }
            MessagesResponseBlock::Thinking {
                thinking,
                signature,
            } => {
                content.push(ContentBlock::Reasoning(ReasoningContent {
                    thinking,
                    signature,
                }));
            }
            MessagesResponseBlock::ToolUse { id, name, input } => {
                content.push(ContentBlock::ToolUse(ToolUseBlock { id, name, input }));
            }
        }
    }

    let finish_reason = resp.stop_reason.as_deref().map(decode_stop_reason);

    let usage = Some(Usage {
        input_tokens: Some(resp.usage.input_tokens),
        output_tokens: Some(resp.usage.output_tokens),
        total_tokens: Some(resp.usage.input_tokens + resp.usage.output_tokens),
        cache_creation_input_tokens: resp.usage.cache_creation_input_tokens,
        cache_read_input_tokens: resp.usage.cache_read_input_tokens,
    });

    let extensions = ProviderExtensions {
        anthropic: resp
            .stop_sequence
            .map(|s| serde_json::json!({"stop_sequence": s})),
        openai: None,
    };

    Ok(CanonicalResponse {
        id: resp.id,
        model: resp.model,
        content,
        finish_reason,
        usage,
        extensions,
    })
}

/// Map an Anthropic stop reason to a canonical finish reason.
fn decode_stop_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" => FinishReason::Stop,
        "stop_sequence" => FinishReason::StopSequence,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Other(reason.to_owned()),
    }
}

// ─── Adapter: Stream event translation ───────────────────────────────────────

/// Translate a canonical stream event to Anthropic Messages streaming events.
///
/// Returns `None` for events that have no Anthropic equivalent.
pub fn encode_stream_event(
    event: &CanonicalStreamEvent,
    response_id: &str,
    model: &str,
) -> Result<Option<MessagesStreamEvent>, ProtocolEngineError> {
    match event {
        CanonicalStreamEvent::MessageStart { message: _ } => {
            Ok(Some(MessagesStreamEvent::MessageStart {
                message: MessagesStreamMessageInfo {
                    id: response_id.into(),
                    model: model.into(),
                    role: "assistant".into(),
                    content: vec![],
                    stop_reason: None,
                    usage: MessagesUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    },
                },
            }))
        }
        CanonicalStreamEvent::ContentBlockStart {
            index,
            content_block,
        } => match content_block {
            ContentBlock::Text(t) => Ok(Some(MessagesStreamEvent::ContentBlockStart {
                index: *index,
                content_block: MessagesResponseBlock::Text {
                    text: t.text.clone(),
                },
            })),
            ContentBlock::ToolUse(tu) => Ok(Some(MessagesStreamEvent::ContentBlockStart {
                index: *index,
                content_block: MessagesResponseBlock::ToolUse {
                    id: tu.id.clone(),
                    name: tu.name.clone(),
                    input: tu.input.clone(),
                },
            })),
            ContentBlock::Reasoning(thinking) => Ok(Some(MessagesStreamEvent::ContentBlockStart {
                index: *index,
                content_block: MessagesResponseBlock::Thinking {
                    thinking: thinking.thinking.clone(),
                    signature: thinking.signature.clone(),
                },
            })),
            ContentBlock::Audio(_) => Ok(None),
            _ => Ok(None),
        },
        CanonicalStreamEvent::TextDelta { index, text } => {
            Ok(Some(MessagesStreamEvent::ContentBlockDelta {
                index: *index,
                delta: MessagesDelta::TextDelta { text: text.clone() },
            }))
        }
        CanonicalStreamEvent::ToolCallDelta {
            index,
            input_json_delta,
            ..
        } => {
            if let Some(delta) = input_json_delta {
                Ok(Some(MessagesStreamEvent::ContentBlockDelta {
                    index: *index,
                    delta: MessagesDelta::InputJsonDelta {
                        partial_json: delta.clone(),
                    },
                }))
            } else {
                Ok(None)
            }
        }
        CanonicalStreamEvent::AudioDelta { .. } => {
            // Anthropic Messages API does not support audio streaming.
            Ok(None)
        }
        CanonicalStreamEvent::ReasoningDelta { index, thinking } => {
            Ok(Some(MessagesStreamEvent::ContentBlockDelta {
                index: *index,
                delta: MessagesDelta::ThinkingDelta {
                    thinking: thinking.clone(),
                },
            }))
        }
        CanonicalStreamEvent::ReasoningSignature { index, signature } => {
            Ok(Some(MessagesStreamEvent::ContentBlockDelta {
                index: *index,
                delta: MessagesDelta::SignatureDelta {
                    signature: signature.clone(),
                },
            }))
        }
        CanonicalStreamEvent::ContentBlockStop { index } => {
            Ok(Some(MessagesStreamEvent::ContentBlockStop {
                index: *index,
            }))
        }
        CanonicalStreamEvent::MessageDelta { stop_reason, .. } => {
            let stop_reason_str = stop_reason.as_ref().map(encode_stop_reason);
            Ok(Some(MessagesStreamEvent::MessageDelta {
                delta: MessagesMessageDelta {
                    stop_reason: stop_reason_str,
                    stop_sequence: None,
                },
                usage: MessagesUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            }))
        }
        CanonicalStreamEvent::Usage { usage } => Ok(Some(MessagesStreamEvent::MessageDelta {
            delta: MessagesMessageDelta {
                stop_reason: None,
                stop_sequence: None,
            },
            usage: MessagesUsage {
                input_tokens: usage.input_tokens.unwrap_or(0),
                output_tokens: usage.output_tokens.unwrap_or(0),
                cache_creation_input_tokens: usage.cache_creation_input_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens,
            },
        })),
        CanonicalStreamEvent::MessageStop => Ok(Some(MessagesStreamEvent::MessageStop)),
        CanonicalStreamEvent::Ping => Ok(Some(MessagesStreamEvent::Ping)),
        CanonicalStreamEvent::Error { message, .. } => Ok(Some(MessagesStreamEvent::Error {
            error: MessagesErrorDetail {
                error_type: "api_error".into(),
                message: message.clone(),
            },
        })),
    }
}

/// Map a canonical finish reason to an Anthropic stop reason.
fn encode_stop_reason(reason: &FinishReason) -> String {
    match reason {
        FinishReason::Stop => "end_turn",
        FinishReason::StopSequence => "stop_sequence",
        FinishReason::Length => "max_tokens",
        FinishReason::ToolCalls => "tool_use",
        FinishReason::ContentFilter => "end_turn",
        FinishReason::Error => "end_turn",
        FinishReason::Other(s) => s,
    }
    .into()
}

// ─── Protocol info ───────────────────────────────────────────────────────────

/// Return the protocol capabilities for Anthropic Messages.
pub fn capabilities() -> ProtocolCapabilities {
    ProtocolCapabilities {
        streaming: true,
        tools: true,
        tool_streaming: true,
        multimodal_input: true,
        structured_output: false,
        reasoning: true,
        usage_streaming: true,
        deferred_tools: false,
    }
}

/// Return the protocol identifier.
pub fn protocol() -> Protocol {
    Protocol::AnthropicMessages
}

// ─── Decoding requests (Anthropic → canonical) ───────────────────────────────

/// Decode an Anthropic Messages request into a canonical request.
pub fn decode_request(req: MessagesRequest) -> Result<CanonicalRequest, ProtocolEngineError> {
    // Decode system instruction.
    let system = req.system.map(|s| match &s {
        serde_json::Value::String(text) => SystemInstruction::Text(text.clone()),
        serde_json::Value::Array(blocks) => {
            let sys_blocks: Vec<SystemBlock> = blocks
                .iter()
                .filter_map(|b| {
                    b.get("text")
                        .and_then(|t| t.as_str())
                        .map(|text| SystemBlock {
                            block_type: "text".into(),
                            text: text.to_owned(),
                        })
                })
                .collect();
            SystemInstruction::Blocks(sys_blocks)
        }
        _ => SystemInstruction::Text(s.to_string()),
    });

    // Decode messages.
    let mut messages = Vec::with_capacity(req.messages.len());

    for msg in &req.messages {
        let role = match msg.role.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => {
                tracing::warn!(role = other, "unknown Anthropic message role");
                Role::User
            }
        };

        let blocks = match &msg.content {
            MessagesContent::Text(text) => {
                vec![ContentBlock::Text(TextContent { text: text.clone() })]
            }
            MessagesContent::Blocks(anthro_blocks) => decode_content_blocks(anthro_blocks)?,
        };

        messages.push(Message {
            role,
            content: MessageContent::Blocks(blocks),
        });
    }

    // Decode tools.
    let tools = req
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: Some(t.input_schema),
            deferred: None,
            extra: Default::default(),
        })
        .collect();

    // Decode tool choice.
    let tool_choice = req.tool_choice.as_ref().map(|tc| match tc {
        AnthropicToolChoice::Auto => ToolChoice::Auto,
        AnthropicToolChoice::Any => ToolChoice::Required,
        AnthropicToolChoice::Tool { name } => ToolChoice::Named { name: name.clone() },
    });

    let stop = req.stop_sequences.unwrap_or_default();

    let extensions = ProviderExtensions {
        anthropic: req
            .extra
            .is_empty()
            .then(|| serde_json::to_value(&req.extra).ok())
            .flatten(),
        openai: None,
    };

    Ok(CanonicalRequest {
        model: req.model,
        messages,
        system,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: Some(req.max_tokens),
        stop,
        tools,
        tool_choice,
        stream: req.stream,
        response_format: None,
        metadata: req.metadata,
        extensions,
    })
}

/// Decode Anthropic content blocks into canonical content blocks.
fn decode_content_blocks(
    blocks: &[MessagesContentBlock],
) -> Result<Vec<ContentBlock>, ProtocolEngineError> {
    let mut result = Vec::with_capacity(blocks.len());

    for block in blocks {
        match block {
            MessagesContentBlock::Text { text } => {
                result.push(ContentBlock::Text(TextContent { text: text.clone() }));
            }
            MessagesContentBlock::Image { source } => {
                let img_source = match source.source_type.as_str() {
                    "url" => ImageSource::Url {
                        url: source.url.clone().unwrap_or_default(),
                        detail: None,
                    },
                    "base64" => ImageSource::Base64 {
                        media_type: source.media_type.clone().unwrap_or_default(),
                        data: source.data.clone().unwrap_or_default(),
                    },
                    other => {
                        return Err(ProtocolEngineError::InvalidPayload {
                            message: format!("unknown image source type: {other}"),
                        });
                    }
                };
                result.push(ContentBlock::Image(ImageContent { source: img_source }));
            }
            MessagesContentBlock::Thinking {
                thinking,
                signature,
            } => {
                result.push(ContentBlock::Reasoning(ReasoningContent {
                    thinking: thinking.clone(),
                    signature: signature.clone(),
                }));
            }
            MessagesContentBlock::ToolUse { id, name, input } => {
                result.push(ContentBlock::ToolUse(ToolUseBlock {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }));
            }
            MessagesContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let tool_content = match content {
                    Some(serde_json::Value::String(s)) => ToolResultContent::Text(s.clone()),
                    Some(serde_json::Value::Array(blocks)) => {
                        // Anthropic tool_result content can be an array of blocks.
                        // Flatten to text.
                        let texts: Vec<String> = blocks
                            .iter()
                            .filter_map(|b| {
                                b.get("text").and_then(|t| t.as_str()).map(|s| s.to_owned())
                            })
                            .collect();
                        ToolResultContent::Text(texts.join("\n"))
                    }
                    Some(other) => ToolResultContent::Text(other.to_string()),
                    None => ToolResultContent::Text(String::new()),
                };
                result.push(ContentBlock::ToolResult(ToolResultBlock {
                    tool_use_id: tool_use_id.clone(),
                    name: None,
                    content: tool_content,
                    is_error: *is_error,
                }));
            }
        }
    }

    Ok(result)
}

// ─── Response encoding (canonical → Anthropic) ───────────────────────────────

/// Encode a canonical response into an Anthropic Messages response.
pub fn encode_response(resp: &CanonicalResponse) -> Result<MessagesResponse, ProtocolEngineError> {
    let mut content = Vec::with_capacity(resp.content.len());

    for block in &resp.content {
        match block {
            ContentBlock::Text(t) => {
                content.push(MessagesResponseBlock::Text {
                    text: t.text.clone(),
                });
            }
            ContentBlock::ToolUse(tu) => {
                content.push(MessagesResponseBlock::ToolUse {
                    id: tu.id.clone(),
                    name: tu.name.clone(),
                    input: tu.input.clone(),
                });
            }
            ContentBlock::Reasoning(thinking) => {
                content.push(MessagesResponseBlock::Thinking {
                    thinking: thinking.thinking.clone(),
                    signature: thinking.signature.clone(),
                });
            }
            ContentBlock::Image(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "image_output".into(),
                    reason: "Anthropic Messages does not support image output".into(),
                });
            }
            ContentBlock::ToolResult(_) | ContentBlock::ToolReference(_) => {
                // These don't appear in model responses.
            }
            ContentBlock::Audio(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "audio_output".into(),
                    reason: "Anthropic Messages does not support audio output".into(),
                });
            }
        }
    }

    let stop_reason = resp.finish_reason.as_ref().map(encode_stop_reason);

    let usage = resp
        .usage
        .as_ref()
        .map(|u| MessagesUsage {
            input_tokens: u.input_tokens.unwrap_or(0),
            output_tokens: u.output_tokens.unwrap_or(0),
            cache_creation_input_tokens: u.cache_creation_input_tokens,
            cache_read_input_tokens: u.cache_read_input_tokens,
        })
        .unwrap_or(MessagesUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        });

    let stop_sequence = resp
        .extensions
        .anthropic
        .as_ref()
        .and_then(|v| v.get("stop_sequence"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    Ok(MessagesResponse {
        id: resp.id.clone(),
        model: resp.model.clone(),
        role: "assistant".into(),
        content,
        stop_reason,
        stop_sequence,
        usage,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_simple_request() {
        let json = r#"{
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "messages": [
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

        assert_eq!(canonical.model, "claude-3-opus");
        assert!(canonical.system.is_some());
        assert_eq!(canonical.messages.len(), 1);
        assert_eq!(canonical.messages[0].role, Role::User);
    }

    #[test]
    fn test_decode_tool_use_in_assistant() {
        let json = r#"{
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_abc", "name": "get_weather", "input": {"city": "NYC"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_abc", "content": "72°F"}
                ]}
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

        assert_eq!(canonical.messages.len(), 3);

        // Assistant message with tool_use.
        let asst_blocks = canonical.messages[1].content.clone().into_blocks();
        assert_eq!(asst_blocks.len(), 1);
        assert!(matches!(&asst_blocks[0], ContentBlock::ToolUse(tu) if tu.id == "toolu_abc"));

        // User message with tool_result.
        let user_blocks = canonical.messages[2].content.clone().into_blocks();
        assert_eq!(user_blocks.len(), 1);
        assert!(
            matches!(&user_blocks[0], ContentBlock::ToolResult(tr) if tr.tool_use_id == "toolu_abc")
        );
    }

    #[test]
    fn test_encode_request_round_trip() {
        let canonical = CanonicalRequest {
            model: "claude-3-opus".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::text("Hello!"),
            }],
            system: Some(SystemInstruction::Text("Be helpful.".into())),
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(1024),
            stop: vec![],
            tools: vec![ToolDefinition {
                name: "search".into(),
                description: Some("Search the web".into()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {"query": {"type": "string"}}
                })),
                deferred: None,
                extra: Default::default(),
            }],
            tool_choice: Some(ToolChoice::Auto),
            stream: false,
            response_format: None,
            metadata: None,
            extensions: ProviderExtensions::default(),
        };

        let anthropic_req = match encode_request(&canonical) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(anthropic_req.model, "claude-3-opus");
        assert_eq!(anthropic_req.max_tokens, 1024);
        assert!(anthropic_req.system.is_some());
        assert_eq!(anthropic_req.messages.len(), 1);
        let tools = match anthropic_req.tools {
            Some(t) => t,
            None => panic!("expected tools"),
        };
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");
    }

    #[test]
    fn test_encode_response_round_trip() {
        let resp = CanonicalResponse {
            id: "msg_123".into(),
            model: "claude-3-opus".into(),
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

        let anthropic_resp = match encode_response(&resp) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(anthropic_resp.id, "msg_123");
        assert_eq!(anthropic_resp.content.len(), 1);
        assert_eq!(anthropic_resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(anthropic_resp.usage.input_tokens, 10);
    }

    #[test]
    fn test_capabilities() {
        let caps = capabilities();
        assert!(caps.streaming);
        assert!(caps.tools);
        assert!(caps.tool_streaming);
        assert!(caps.multimodal_input);
        assert!(!caps.structured_output);
    }
}
