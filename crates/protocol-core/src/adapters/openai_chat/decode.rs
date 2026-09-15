//! OpenAI Chat Completions adapter: request and response decoding.

use crate::canonical::*;
use crate::error::ProtocolEngineError;

use super::wire::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatToolChoice};

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

/// Decode an OpenAI Chat Completions response into a canonical response.
pub fn decode_response(
    resp: &ChatCompletionResponse,
) -> Result<CanonicalResponse, ProtocolEngineError> {
    let mut content = Vec::new();

    for choice in &resp.choices {
        if let Some(text) = &choice.message.content
            && !text.is_empty()
        {
            content.push(ContentBlock::Text(TextContent { text: text.clone() }));
        }
        if let Some(tcs) = &choice.message.tool_calls {
            for tc in tcs {
                let input = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                content.push(ContentBlock::ToolUse(ToolUseBlock {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                }));
            }
        }
    }

    let finish_reason = resp.choices.first().and_then(|c| {
        c.finish_reason.as_ref().map(|r| match r.as_str() {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            other => FinishReason::Other(other.to_owned()),
        })
    });

    let usage = resp.usage.as_ref().map(|u| Usage {
        input_tokens: Some(u.prompt_tokens),
        output_tokens: Some(u.completion_tokens),
        total_tokens: Some(u.total_tokens),
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    });

    Ok(CanonicalResponse {
        id: resp.id.clone(),
        model: resp.model.clone(),
        content,
        finish_reason,
        usage,
        extensions: ProviderExtensions::default(),
    })
}

// ─── Internal helpers ────────────────────────────────────────────────────────

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
