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

use crate::canonical::*;
use crate::error::ProtocolEngineError;

// ─── Wire types (OpenAI Chat Completions) ────────────────────────────────────

/// Top-level OpenAI Chat Completions request body.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ChatResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Catch-all for provider-specific fields.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// An OpenAI chat message.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call in an OpenAI message.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ChatFunctionCall,
}

/// Function call data within a tool call.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A tool definition in the request.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ChatFunctionDefinition,
}

/// Function definition within a tool.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatFunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Strict mode (OpenAI-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Tool choice control.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum ChatToolChoice {
    String(String),
    Object(ChatToolChoiceObject),
}

/// Tool choice object for named tool selection.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatToolChoiceObject {
    #[serde(rename = "type")]
    pub choice_type: String,
    pub function: ChatToolChoiceFunction,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatToolChoiceFunction {
    pub name: String,
}

/// Response format specification.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ChatResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

// ─── Response wire types ─────────────────────────────────────────────────────

/// Top-level OpenAI Chat Completions response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

/// A single choice in the response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatResponseMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// A response message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatResponseMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCallResponse>>,
}

/// A tool call in the response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatToolCallResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ChatFunctionCallResponse,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatFunctionCallResponse {
    pub name: String,
    pub arguments: String,
}

/// Usage information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ─── Streaming wire types ────────────────────────────────────────────────────

/// A streaming chunk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

/// A choice within a streaming chunk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The delta (incremental change) in a streaming chunk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatDeltaToolCall>>,
}

/// A tool call delta in streaming.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatDeltaToolCall {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ChatDeltaFunction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatDeltaFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ─── Adapter: Request decoding ───────────────────────────────────────────────

/// Decode an OpenAI Chat Completions request into a canonical request.
pub fn decode_request(req: ChatCompletionRequest) -> Result<CanonicalRequest, ProtocolEngineError> {
    let (messages, system) = decode_request_messages(req.messages)?;

    let tools = req
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.function.name,
            description: t.function.description,
            input_schema: t.function.parameters,
            deferred: None,
            extra: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "strict".into(),
                    serde_json::Value::Bool(t.function.strict.unwrap_or(false)),
                );
                m
            },
        })
        .collect();

    let tool_choice = req.tool_choice.map(decode_tool_choice);

    let response_format = req.response_format.map(|rf| ResponseFormat {
        format_type: rf.format_type,
        json_schema: rf.json_schema,
    });

    let extensions = ProviderExtensions {
        openai: if req.extra.is_empty() {
            None
        } else {
            Some(serde_json::to_value(&req.extra).unwrap_or_default())
        },
        anthropic: None,
    };

    Ok(CanonicalRequest {
        model: req.model,
        messages,
        system,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        stop: req.stop.unwrap_or_default(),
        tools,
        tool_choice,
        stream: req.stream,
        response_format,
        metadata: req.metadata,
        extensions,
    })
}

/// Decode OpenAI messages into canonical messages and system instruction.
fn decode_request_messages(
    msgs: Vec<ChatMessage>,
) -> Result<(Vec<Message>, Option<SystemInstruction>), ProtocolEngineError> {
    let mut messages = Vec::with_capacity(msgs.len());
    let mut system: Option<SystemInstruction> = None;

    for msg in msgs {
        match msg.role.as_str() {
            "system" | "developer" => {
                let text = extract_text_content(&msg.content).ok_or_else(|| {
                    ProtocolEngineError::InvalidPayload {
                        message: "system message must have text content".into(),
                    }
                })?;
                match &mut system {
                    Some(SystemInstruction::Text(existing)) => {
                        existing.push_str("\n\n");
                        existing.push_str(&text);
                    }
                    Some(SystemInstruction::Blocks(blocks)) => {
                        blocks.push(SystemBlock {
                            block_type: "text".into(),
                            text,
                        });
                    }
                    None => {
                        system = Some(SystemInstruction::Text(text));
                    }
                }
            }
            "user" => {
                let content = decode_content_blocks(&msg.content)?;
                messages.push(Message {
                    role: Role::User,
                    content: MessageContent::Blocks(content),
                });
            }
            "assistant" => {
                let mut blocks = decode_content_blocks(&msg.content)?;
                if let Some(tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let input =
                            serde_json::from_str(&tc.function.arguments).unwrap_or_else(|e| {
                                tracing::warn!(
                                    tool_call_id = %tc.id,
                                    error = %e,
                                    "malformed tool arguments; substituting empty object"
                                );
                                serde_json::Value::Object(Default::default())
                            });
                        blocks.push(ContentBlock::ToolUse(ToolUseBlock {
                            id: tc.id,
                            name: tc.function.name,
                            input,
                        }));
                    }
                }
                messages.push(Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(blocks),
                });
            }
            "tool" => {
                let content = msg
                    .content
                    .map(|c| extract_text_from_value(&c).unwrap_or_default())
                    .unwrap_or_default();
                let tool_use_id =
                    msg.tool_call_id
                        .ok_or_else(|| ProtocolEngineError::InvalidPayload {
                            message: "tool message missing tool_call_id".into(),
                        })?;
                messages.push(Message {
                    role: Role::Tool,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult(
                        ToolResultBlock {
                            tool_use_id,
                            name: msg.name,
                            content: ToolResultContent::Text(content),
                            is_error: None,
                        },
                    )]),
                });
            }
            other => {
                tracing::warn!(
                    role = other,
                    "unknown OpenAI message role, treating as user"
                );
                let content = decode_content_blocks(&msg.content)?;
                messages.push(Message {
                    role: Role::User,
                    content: MessageContent::Blocks(content),
                });
            }
        }
    }

    Ok((messages, system))
}

/// Decode content from an OpenAI message content field.
///
/// OpenAI content can be:
/// - `null` (no content)
/// - a string: `"hello"`
/// - an array of content blocks: `[{"type": "text", "text": "hi"}, ...]`
fn decode_content_blocks(
    content: &Option<serde_json::Value>,
) -> Result<Vec<ContentBlock>, ProtocolEngineError> {
    let value = match content {
        Some(v) if !v.is_null() => v,
        _ => return Ok(vec![]),
    };

    match value {
        serde_json::Value::String(s) => {
            Ok(vec![ContentBlock::Text(TextContent { text: s.clone() })])
        }
        serde_json::Value::Array(blocks) => {
            let mut result = Vec::with_capacity(blocks.len());
            for block in blocks {
                result.push(decode_content_block(block)?);
            }
            Ok(result)
        }
        other => {
            // Unexpected content type — treat as text.
            tracing::warn!(content = %other, "unexpected content type, treating as text");
            Ok(vec![ContentBlock::Text(TextContent {
                text: other.to_string(),
            })])
        }
    }
}

/// Decode a single OpenAI content block.
fn decode_content_block(value: &serde_json::Value) -> Result<ContentBlock, ProtocolEngineError> {
    let block_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    match block_type {
        "text" => {
            let text = value
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            Ok(ContentBlock::Text(TextContent { text }))
        }
        "image_url" => {
            let url_obj =
                value
                    .get("image_url")
                    .ok_or_else(|| ProtocolEngineError::InvalidPayload {
                        message: "image_url block missing image_url field".into(),
                    })?;
            let url = url_obj
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let detail = url_obj
                .get("detail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            Ok(ContentBlock::Image(ImageContent {
                source: ImageSource::Url { url, detail },
            }))
        }
        "input_audio" => {
            let audio_obj =
                value
                    .get("input_audio")
                    .ok_or_else(|| ProtocolEngineError::InvalidPayload {
                        message: "input_audio block missing input_audio field".into(),
                    })?;
            let data = audio_obj
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let format = audio_obj
                .get("format")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            let media_type = format
                .as_deref()
                .map(|f| format!("audio/{f}"))
                .unwrap_or_else(|| "application/octet-stream".into());
            Ok(ContentBlock::Audio(AudioContent {
                source: AudioSource::Base64 {
                    media_type,
                    data,
                    format,
                },
            }))
        }
        other => {
            tracing::warn!(block_type = other, "unknown content block type");
            Ok(ContentBlock::Text(TextContent {
                text: value.to_string(),
            }))
        }
    }
}

/// Extract text from an OpenAI content value.
fn extract_text_content(content: &Option<serde_json::Value>) -> Option<String> {
    match content {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(blocks)) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Extract text from a JSON value (best effort).
fn extract_text_from_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Decode an OpenAI tool choice into a canonical ToolChoice.
fn decode_tool_choice(tc: ChatToolChoice) -> ToolChoice {
    match tc {
        ChatToolChoice::String(s) => match s.as_str() {
            "auto" => ToolChoice::Auto,
            "required" => ToolChoice::Required,
            "none" => ToolChoice::None,
            other => ToolChoice::Named {
                name: other.to_owned(),
            },
        },
        ChatToolChoice::Object(obj) => ToolChoice::Named {
            name: obj.function.name,
        },
    }
}

// ─── Adapter: Response encoding ──────────────────────────────────────────────

/// Encode a canonical response into an OpenAI Chat Completions response.
pub fn encode_response(
    resp: &CanonicalResponse,
    created: u64,
) -> Result<ChatCompletionResponse, ProtocolEngineError> {
    let mut text_content = String::new();
    let mut tool_calls = Vec::new();

    for block in &resp.content {
        match block {
            ContentBlock::Text(t) => {
                if !text_content.is_empty() {
                    text_content.push('\n');
                }
                text_content.push_str(&t.text);
            }
            ContentBlock::ToolUse(tu) => {
                let arguments = serde_json::to_string(&tu.input).map_err(|e| {
                    ProtocolEngineError::TranslationFailure {
                        message: format!("failed to serialize tool arguments: {e}"),
                    }
                })?;
                tool_calls.push(ChatToolCallResponse {
                    id: tu.id.clone(),
                    call_type: "function".into(),
                    function: ChatFunctionCallResponse {
                        name: tu.name.clone(),
                        arguments,
                    },
                });
            }
            ContentBlock::Image(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "image_output".into(),
                    reason: "OpenAI Chat Completions does not support image output".into(),
                });
            }
            ContentBlock::Audio(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "audio_output".into(),
                    reason: "OpenAI Chat Completions does not support audio output".into(),
                });
            }
            ContentBlock::Reasoning(_) => {
                // Reasoning blocks are provider-internal; not represented in output.
            }
            ContentBlock::ToolResult(_) => {
                // Tool results don't appear in assistant responses.
            }
            ContentBlock::ToolReference(_) => {
                // Deferred references don't appear in responses.
            }
        }
    }

    let finish_reason_str = resp.finish_reason.as_ref().map(finish_reason_to_openai);

    let message = ChatResponseMessage {
        role: "assistant".into(),
        content: if text_content.is_empty() {
            None
        } else {
            Some(text_content)
        },
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    };

    let usage = resp.usage.as_ref().map(usage_to_chat);

    Ok(ChatCompletionResponse {
        id: resp.id.clone(),
        object: "chat.completion".into(),
        created,
        model: resp.model.clone(),
        choices: vec![ChatChoice {
            index: 0,
            message,
            finish_reason: finish_reason_str.map(|s| s.into()),
        }],
        usage,
    })
}

// ─── Adapter: Stream event translation ───────────────────────────────────────

/// Translate a canonical stream event to an OpenAI Chat Completions streaming chunk.
pub fn encode_stream_event(
    event: &CanonicalStreamEvent,
    response_id: &str,
    model: &str,
    created: u64,
) -> Result<Option<ChatCompletionChunk>, ProtocolEngineError> {
    match event {
        CanonicalStreamEvent::MessageStart { .. } => {
            // OpenAI doesn't have an explicit message_start event.
            // The first chunk carries the role.
            Ok(Some(ChatCompletionChunk {
                id: response_id.into(),
                object: "chat.completion.chunk".into(),
                created,
                model: model.into(),
                choices: vec![ChatChunkChoice {
                    index: 0,
                    delta: ChatDelta {
                        role: Some("assistant".into()),
                        content: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            }))
        }
        CanonicalStreamEvent::ContentBlockStart { content_block, .. } => {
            match content_block {
                ContentBlock::ToolUse(tu) => {
                    // OpenAI sends tool call info in the first delta.
                    Ok(Some(ChatCompletionChunk {
                        id: response_id.into(),
                        object: "chat.completion.chunk".into(),
                        created,
                        model: model.into(),
                        choices: vec![ChatChunkChoice {
                            index: 0,
                            delta: ChatDelta {
                                role: None,
                                content: None,
                                tool_calls: Some(vec![ChatDeltaToolCall {
                                    index: 0,
                                    id: Some(tu.id.clone()),
                                    call_type: Some("function".into()),
                                    function: Some(ChatDeltaFunction {
                                        name: Some(tu.name.clone()),
                                        arguments: None,
                                    }),
                                }]),
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    }))
                }
                _ => {
                    // Text content blocks don't need an explicit start in OpenAI.
                    Ok(None)
                }
            }
        }
        CanonicalStreamEvent::TextDelta { text, .. } => Ok(Some(ChatCompletionChunk {
            id: response_id.into(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.into(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatDelta {
                    role: None,
                    content: Some(text.clone()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        })),
        CanonicalStreamEvent::ToolCallDelta {
            tool_use_id,
            name,
            input_json_delta,
            index,
        } => Ok(Some(ChatCompletionChunk {
            id: response_id.into(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.into(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![ChatDeltaToolCall {
                        index: *index as u32,
                        id: tool_use_id.clone(),
                        call_type: Some("function".into()),
                        function: Some(ChatDeltaFunction {
                            name: name.clone(),
                            arguments: input_json_delta.clone(),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
        })),
        CanonicalStreamEvent::ContentBlockStop { .. } => {
            // OpenAI doesn't have an explicit content_block_stop.
            Ok(None)
        }
        CanonicalStreamEvent::MessageDelta { stop_reason, usage } => {
            let finish_reason = stop_reason.as_ref().map(finish_reason_to_openai);

            let chat_usage = usage.as_ref().map(usage_to_chat);

            Ok(Some(ChatCompletionChunk {
                id: response_id.into(),
                object: "chat.completion.chunk".into(),
                created,
                model: model.into(),
                choices: vec![ChatChunkChoice {
                    index: 0,
                    delta: ChatDelta {
                        role: None,
                        content: None,
                        tool_calls: None,
                    },
                    finish_reason: finish_reason.map(|s| s.into()),
                }],
                usage: chat_usage,
            }))
        }
        CanonicalStreamEvent::Usage { usage } => Ok(Some(ChatCompletionChunk {
            id: response_id.into(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.into(),
            choices: vec![],
            usage: Some(usage_to_chat(usage)),
        })),
        CanonicalStreamEvent::AudioDelta { .. } => {
            // OpenAI Chat Completions doesn't have audio streaming in chunks.
            Ok(None)
        }
        CanonicalStreamEvent::ReasoningDelta { .. } => {
            // Reasoning tokens are not exposed in Chat Completions output.
            Ok(None)
        }
        CanonicalStreamEvent::ReasoningSignature { .. } => {
            // Signatures are not used in OpenAI Chat Completions.
            Ok(None)
        }
        CanonicalStreamEvent::MessageStop => {
            // OpenAI doesn't have an explicit message_stop.
            // The [DONE] sentinel is sent separately.
            Ok(None)
        }
        CanonicalStreamEvent::Error { message, .. } => Err(ProtocolEngineError::ProviderError {
            message: message.clone(),
        }),
        CanonicalStreamEvent::Ping => Ok(None),
    }
}

// ─── Protocol info ───────────────────────────────────────────────────────────

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

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Map a canonical `FinishReason` to an OpenAI finish_reason string.
fn finish_reason_to_openai(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::StopSequence => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Error => "stop",
        FinishReason::Other(_) => "stop",
    }
}

/// Convert canonical `Usage` to OpenAI `ChatUsage`.
fn usage_to_chat(usage: &Usage) -> ChatUsage {
    ChatUsage {
        prompt_tokens: usage.input_tokens.unwrap_or(0),
        completion_tokens: usage.output_tokens.unwrap_or(0),
        total_tokens: usage.total_tokens.unwrap_or(0),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
