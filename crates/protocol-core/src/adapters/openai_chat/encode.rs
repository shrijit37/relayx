//! OpenAI Chat Completions adapter: request, response, and stream encoding.

use crate::canonical::*;
use crate::error::ProtocolEngineError;

use super::wire::*;

/// Encode a canonical request into an OpenAI Chat Completions wire request.
pub fn encode_request(
    req: &CanonicalRequest,
) -> Result<ChatCompletionRequest, ProtocolEngineError> {
    let mut messages = Vec::new();

    // System instruction → system message.
    if let Some(sys) = &req.system {
        let text = match sys {
            crate::canonical::SystemInstruction::Text(t) => t.clone(),
            crate::canonical::SystemInstruction::Blocks(blocks) => {
                let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
                texts.join("\n")
            }
        };
        messages.push(ChatMessage {
            role: "system".into(),
            content: Some(serde_json::Value::String(text)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &req.messages {
        // Skip Tool-role messages here: OpenAI requires `tool_call_id` on
        // tool-role messages, which only the second loop below emits correctly.
        // `encode_message` ignores ToolResult blocks, so passing a Tool-role
        // message through here would emit a malformed message with no
        // `tool_call_id`.
        if msg.role == crate::canonical::Role::Tool {
            continue;
        }
        let (role_str, content, tool_calls) = encode_message(msg)?;
        messages.push(ChatMessage {
            role: role_str,
            content,
            name: None,
            tool_calls,
            tool_call_id: None,
        });
    }

    // Tool results: OpenAI uses separate `tool` role messages (with `tool_call_id`).
    for msg in &req.messages {
        if msg.role == crate::canonical::Role::Tool {
            let blocks = msg.content.clone().into_blocks();
            for block in blocks {
                if let crate::canonical::ContentBlock::ToolResult(tr) = block {
                    let content = match &tr.content {
                        crate::canonical::ToolResultContent::Text(s) => {
                            Some(serde_json::Value::String(s.clone()))
                        }
                        crate::canonical::ToolResultContent::Blocks(blocks) => {
                            let texts: Vec<String> = blocks
                                .iter()
                                .filter_map(|b| b.as_text().map(|s| s.to_owned()))
                                .collect();
                            Some(serde_json::Value::String(texts.join("\n")))
                        }
                    };
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content,
                        name: None,
                        tool_calls: None,
                        tool_call_id: Some(tr.tool_use_id),
                    });
                }
            }
        }
    }

    let tools: Vec<ChatToolDefinition> = req
        .tools
        .iter()
        .map(|t| ChatToolDefinition {
            tool_type: "function".into(),
            function: ChatFunctionDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
                strict: t.extra.get("strict").and_then(|v| v.as_bool()),
            },
        })
        .collect();

    let tool_choice = encode_tool_choice(&req.tool_choice);

    let response_format = req.response_format.as_ref().map(|rf| ChatResponseFormat {
        format_type: rf.format_type.clone(),
        json_schema: rf.json_schema.clone(),
    });

    Ok(ChatCompletionRequest {
        model: req.model.clone(),
        messages,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        stream: req.stream,
        stop: if req.stop.is_empty() {
            None
        } else {
            Some(req.stop.clone())
        },
        tools: if tools.is_empty() { None } else { Some(tools) },
        tool_choice,
        response_format,
        metadata: req.metadata.clone(),
        extra: Default::default(),
    })
}

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
            status: None,
        }),
        CanonicalStreamEvent::Ping => Ok(None),
    }
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Encoded chat message fields: (role, content, tool_calls).
type EncodedChatMessage = (String, Option<serde_json::Value>, Option<Vec<ChatToolCall>>);

/// Encode a single canonical message into OpenAI Chat fields.
fn encode_message(
    msg: &crate::canonical::Message,
) -> Result<EncodedChatMessage, ProtocolEngineError> {
    let role_str = match msg.role {
        crate::canonical::Role::User => "user",
        crate::canonical::Role::Assistant => "assistant",
        crate::canonical::Role::Tool => "tool",
        crate::canonical::Role::System => "system",
    };

    let blocks = msg.content.clone().into_blocks();
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ChatToolCall> = Vec::new();

    for block in blocks {
        match block {
            crate::canonical::ContentBlock::Text(t) => text_parts.push(t.text),
            crate::canonical::ContentBlock::ToolUse(tu) => {
                let arguments = serde_json::to_string(&tu.input).map_err(|e| {
                    ProtocolEngineError::TranslationFailure {
                        message: format!("failed to serialize tool arguments: {e}"),
                    }
                })?;
                tool_calls.push(ChatToolCall {
                    id: tu.id,
                    call_type: "function".into(),
                    function: ChatFunctionCall {
                        name: tu.name,
                        arguments,
                    },
                });
            }
            crate::canonical::ContentBlock::ToolResult(_)
            | crate::canonical::ContentBlock::Image(_)
            | crate::canonical::ContentBlock::Audio(_)
            | crate::canonical::ContentBlock::Reasoning(_)
            | crate::canonical::ContentBlock::ToolReference(_) => {}
        }
    }

    let content: Option<serde_json::Value> = if text_parts.is_empty() && tool_calls.is_empty() {
        None
    } else if text_parts.is_empty() {
        Some(serde_json::Value::Null)
    } else {
        Some(serde_json::Value::String(text_parts.join("\n")))
    };

    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    Ok((role_str.into(), content, tool_calls_opt))
}

/// Encode a canonical ToolChoice into OpenAI Chat tool_choice.
fn encode_tool_choice(tc: &Option<crate::canonical::ToolChoice>) -> Option<ChatToolChoice> {
    tc.as_ref().map(|tc| match tc {
        crate::canonical::ToolChoice::Auto => ChatToolChoice::String("auto".into()),
        crate::canonical::ToolChoice::Required => ChatToolChoice::String("required".into()),
        crate::canonical::ToolChoice::None => ChatToolChoice::String("none".into()),
        crate::canonical::ToolChoice::Named { name } => {
            ChatToolChoice::Object(ChatToolChoiceObject {
                choice_type: "function".into(),
                function: ChatToolChoiceFunction { name: name.clone() },
            })
        }
    })
}

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
