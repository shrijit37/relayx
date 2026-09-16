//! OpenAI Responses API adapter.
//!
//! Translates between the OpenAI Responses wire format and the canonical model.
//!
//! # Wire format
//!
//! The Responses API differs from Chat Completions in important ways:
//! - Uses `input` instead of `messages`.
//! - Input items have a flat structure: `{role, content: [...]}`.
//! - Supports `instructions` as a top-level field (system prompt).
//! - Content parts use `input_text`, `input_image`, `input_audio`, `output_text`.
//! - Tools are flat: `{type: "function", name, description, parameters, strict}`.
//! - Streaming events have distinct types: `response.created`,
//!   `response.output_item.added`, `response.output_text.delta`, etc.
//! - The response object contains `output` items (not `choices[].message`).
//! - Finish reason is `status` on the response object.

use crate::canonical::*;
use crate::error::ProtocolEngineError;

// ─── Wire types (OpenAI Responses) ──────────────────────────────────────────

/// Top-level OpenAI Responses request body.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ResponsesToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Catch-all for provider-specific fields.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// A tool definition in the Responses API.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    /// Extra fields for non-function tools (web_search, file_search, etc).
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// Tool choice in the Responses API.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum ResponsesToolChoice {
    String(String),
    Object(serde_json::Value),
}

/// A response item — the core output unit of the Responses API.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesItem {
    Message {
        id: String,
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ResponsesContentPart>>,
    },
    FunctionCall {
        id: String,
        #[serde(rename = "call_id")]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "call_id")]
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    Reasoning {
        id: String,
        summary: Option<Vec<ReasoningSummary>>,
    },
    #[serde(untagged)]
    Other(serde_json::Value),
}

/// Reasoning summary part in a Responses item.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ReasoningSummary {
    #[serde(rename = "type")]
    pub summary_type: String,
    pub text: String,
}

/// A content part within a message item.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesContentPart {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
    },
    InputImage {
        image_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    InputAudio {
        #[serde(skip_serializing_if = "Option::is_none")]
        input_audio: Option<serde_json::Value>,
    },
    Refusal {
        refusal: String,
    },
    #[serde(untagged)]
    Other(serde_json::Value),
}

/// Top-level OpenAI Responses response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub output: Vec<ResponsesItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponsesUsage>,
}

/// Usage information in a Responses response.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ResponsesUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens_details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<serde_json::Value>,
}

// ─── Streaming wire types ────────────────────────────────────────────────────

/// A streaming event from the Responses API.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesStreamEvent {
    ResponseCreated {
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<serde_json::Value>,
    },
    ResponseInProgress {
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<serde_json::Value>,
    },
    ResponseOutputItemAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item: Option<ResponsesItem>,
    },
    ResponseContentPartAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<ResponsesContentPart>,
    },
    ResponseOutputTextDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
    },
    ResponseOutputTextDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    ResponseFunctionCallArgumentsDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
    },
    ResponseFunctionCallArgumentsDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<String>,
    },
    ResponseCompleted {
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<ResponsesResponse>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<serde_json::Value>,
    },
    #[serde(untagged)]
    Other(serde_json::Value),
}

/// Map a Responses API status to a canonical finish reason.
fn decode_status(status: &str) -> FinishReason {
    match status {
        "completed" => FinishReason::Stop,
        "incomplete" => FinishReason::Length,
        "failed" => FinishReason::Error,
        "cancelled" => FinishReason::Error,
        _ => FinishReason::Other(status.to_owned()),
    }
}

/// Encode a canonical request into a Responses API wire request.
///
/// This is the inverse of [`decode_request`]. Messages map to `input` items;
/// system instructions map to `instructions`; tools and tool_choice are
/// translated to Responses-native types.
pub fn encode_request(req: &CanonicalRequest) -> Result<ResponsesRequest, ProtocolEngineError> {
    // System instructions → instructions field.
    let instructions = req.system.as_ref().map(|s| match s {
        crate::canonical::SystemInstruction::Text(t) => serde_json::Value::String(t.clone()),
        crate::canonical::SystemInstruction::Blocks(blocks) => {
            let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
            serde_json::Value::String(texts.join("\n"))
        }
    });

    // Messages → input items.
    let mut input: Vec<serde_json::Value> = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            crate::canonical::Role::User => "user",
            crate::canonical::Role::Assistant => "assistant",
            crate::canonical::Role::System => "system",
            crate::canonical::Role::Tool => "user",
        };
        let blocks = msg.content.clone().into_blocks();
        let mut parts: Vec<serde_json::Value> = Vec::new();
        for block in blocks {
            match block {
                crate::canonical::ContentBlock::Text(t) => {
                    parts.push(serde_json::json!({"type": "input_text", "text": t.text}));
                }
                crate::canonical::ContentBlock::Image(img) => {
                    let url = match &img.source {
                        crate::canonical::ImageSource::Url { url, .. } => url.clone(),
                        crate::canonical::ImageSource::Base64 {
                            media_type, data, ..
                        } => {
                            format!("data:{media_type};base64,{data}")
                        }
                    };
                    parts.push(serde_json::json!({"type": "input_image", "image_url": url}));
                }
                crate::canonical::ContentBlock::ToolResult(tr) => {
                    let output = match &tr.content {
                        crate::canonical::ToolResultContent::Text(s) => s.clone(),
                        _ => String::new(),
                    };
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": tr.tool_use_id,
                        "output": output,
                    }));
                    continue;
                }
                _ => {}
            }
        }
        input.push(serde_json::json!({"role": role, "content": parts}));
    }

    let input_value = if input.len() == 1 && input[0].get("content").is_some() {
        input.remove(0)
    } else {
        serde_json::Value::Array(input)
    };

    let tools: Vec<ResponsesTool> = req
        .tools
        .iter()
        .map(|t| ResponsesTool {
            tool_type: "function".into(),
            name: Some(t.name.clone()),
            description: t.description.clone(),
            parameters: t.input_schema.clone(),
            strict: t.extra.get("strict").and_then(|v| v.as_bool()),
            extra: Default::default(),
        })
        .collect();

    let tool_choice = req.tool_choice.as_ref().map(|tc| match tc {
        crate::canonical::ToolChoice::Auto => ResponsesToolChoice::String("auto".into()),
        crate::canonical::ToolChoice::Required => ResponsesToolChoice::String("required".into()),
        crate::canonical::ToolChoice::None => ResponsesToolChoice::String("none".into()),
        crate::canonical::ToolChoice::Named { name } => {
            ResponsesToolChoice::Object(serde_json::json!({"type": "function", "name": name}))
        }
    });

    Ok(ResponsesRequest {
        model: req.model.clone(),
        instructions,
        input: Some(input_value),
        temperature: req.temperature,
        top_p: req.top_p,
        max_output_tokens: req.max_tokens,
        stream: req.stream,
        stop: if req.stop.is_empty() {
            None
        } else {
            Some(req.stop.clone())
        },
        tools: if tools.is_empty() { None } else { Some(tools) },
        tool_choice,
        response_format: req.response_format.as_ref().map(|rf| {
            let mut obj = serde_json::json!({"type": rf.format_type});
            if let Some(schema) = &rf.json_schema {
                obj["json_schema"] = schema.clone();
            }
            obj
        }),
        metadata: req.metadata.clone(),
        extra: Default::default(),
    })
}

fn encode_status(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "completed",
        FinishReason::StopSequence => "completed",
        FinishReason::Length => "incomplete",
        FinishReason::ToolCalls => "incomplete",
        FinishReason::ContentFilter => "incomplete",
        FinishReason::Error => "failed",
        FinishReason::Other(_) => "in_progress",
    }
}

/// Extract content parts from an input item.
fn decode_content_parts(
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
        serde_json::Value::Array(parts) => {
            let mut result = Vec::with_capacity(parts.len());
            for part in parts {
                let block = decode_content_part(part)?;
                if let Some(block) = block {
                    result.push(block);
                }
            }
            Ok(result)
        }
        other => {
            tracing::warn!(content = %other, "unexpected content type, treating as text");
            Ok(vec![ContentBlock::Text(TextContent {
                text: other.to_string(),
            })])
        }
    }
}

/// Decode a single Responses content part into a canonical content block.
fn decode_content_part(
    value: &serde_json::Value,
) -> Result<Option<ContentBlock>, ProtocolEngineError> {
    let part_type = value.get("type").and_then(|v| v.as_str());

    match part_type {
        Some("input_text") | Some("output_text") | Some("text") => {
            let text = value
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            Ok(Some(ContentBlock::Text(TextContent { text })))
        }
        Some("input_image") => {
            let url = value
                .get("image_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let detail = value
                .get("detail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            Ok(Some(ContentBlock::Image(ImageContent {
                source: ImageSource::Url { url, detail },
            })))
        }
        Some("input_audio") => {
            let audio_obj = value.get("input_audio");
            if let Some(audio_obj) = audio_obj {
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
                Ok(Some(ContentBlock::Audio(AudioContent {
                    source: AudioSource::Base64 {
                        media_type,
                        data,
                        format,
                    },
                })))
            } else {
                Ok(None)
            }
        }
        Some("refusal") => {
            let refusal = value
                .get("refusal")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            Ok(Some(ContentBlock::Text(TextContent { text: refusal })))
        }
        Some(other) => {
            tracing::warn!(block_type = other, "unknown Responses content part type");
            Ok(Some(ContentBlock::Text(TextContent {
                text: value.to_string(),
            })))
        }
        None => Ok(None),
    }
}

// ─── Adapter: Request decoding (Responses → canonical) ──────────────────────

/// Decode an OpenAI Responses request into a canonical request.
pub fn decode_request(req: ResponsesRequest) -> Result<CanonicalRequest, ProtocolEngineError> {
    // Decode instructions → system.
    let system = req.instructions.as_ref().map(|s| match s {
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

    // Decode input → messages.
    let mut messages: Vec<Message> = Vec::new();
    let input = req.input.clone();

    if let Some(serde_json::Value::Array(items)) = input {
        for item in items {
            if let Some(role) = item.get("role").and_then(|v| v.as_str()) {
                let blocks = decode_content_parts(&item.get("content").cloned())?;

                if blocks.is_empty() {
                    continue;
                }

                let canonical_role = match role {
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    "system" => Role::System,
                    "tool" => Role::Tool,
                    other => {
                        tracing::warn!(role = other, "unknown Responses input role");
                        Role::User
                    }
                };

                // Convert tool call items (function_call) in assistant input.
                messages.push(Message {
                    role: canonical_role,
                    content: MessageContent::Blocks(blocks),
                });
            }

            // Non-message items (function_call_output) that appear in input arrays
            // carry tool results. Handle them explicitly.
            if let Some(item_type) = item.get("type").and_then(|v| v.as_str())
                && item_type == "function_call_output"
            {
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let output = item
                    .get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                messages.push(Message {
                    role: Role::Tool,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult(
                        ToolResultBlock {
                            tool_use_id: call_id,
                            name: None,
                            content: ToolResultContent::Text(output),
                            is_error: None,
                        },
                    )]),
                });
            }

            // Non-message items (function_call) represent model tool calls.
            // Decode them as assistant messages with ToolUse blocks.
            if let Some(item_type) = item.get("type").and_then(|v| v.as_str())
                && item_type == "function_call"
            {
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let arguments = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}")
                    .to_owned();
                let input = serde_json::from_str(&arguments)
                    .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                messages.push(Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolUse(ToolUseBlock {
                        id,
                        name,
                        input,
                    })]),
                });
            }

            // Non-message items (reasoning) carry prior reasoning summaries.
            // Preserve them as canonical Reasoning blocks.
            if let Some(item_type) = item.get("type").and_then(|v| v.as_str())
                && item_type == "reasoning"
            {
                let combined: String = item
                    .get("summary")
                    .and_then(|s| s.as_array())
                    .map(|summaries| {
                        summaries
                            .iter()
                            .filter_map(|s| s.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if !combined.is_empty() {
                    messages.push(Message {
                        role: Role::Assistant,
                        content: MessageContent::Blocks(vec![ContentBlock::Reasoning(
                            ReasoningContent {
                                thinking: combined,
                                signature: None,
                            },
                        )]),
                    });
                }
            }
        }
    } else if let Some(serde_json::Value::String(s)) = req.input {
        // Input can be a plain string.
        messages.push(Message {
            role: Role::User,
            content: MessageContent::Text(s),
        });
    }

    // Decode tools.
    let tools = req
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name.unwrap_or_default(),
            description: t.description,
            input_schema: t.parameters,
            deferred: if t.tool_type == "function" {
                None
            } else {
                // Non-function tools (web_search, file_search, code_interpreter,
                // mcp) are preserved as extensions.
                Some(false)
            },
            extra: {
                let mut m = std::collections::HashMap::new();
                m.insert("type".into(), serde_json::Value::String(t.tool_type));
                if let Some(strict) = t.strict {
                    m.insert("strict".into(), serde_json::Value::Bool(strict));
                }
                for (k, v) in t.extra {
                    m.insert(k, v);
                }
                m
            },
        })
        .collect();

    // Decode response_format.
    let response_format = req.response_format.map(|rf| ResponseFormat {
        format_type: rf
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_owned(),
        json_schema: rf.get("json_schema").cloned(),
    });

    let stop = req.stop.unwrap_or_default();

    // Decode tool_choice.
    let tool_choice = req.tool_choice.as_ref().map(|tc| match tc {
        ResponsesToolChoice::String(s) => match s.as_str() {
            "auto" => ToolChoice::Auto,
            "required" => ToolChoice::Required,
            "none" => ToolChoice::None,
            other => ToolChoice::Named {
                name: other.to_owned(),
            },
        },
        ResponsesToolChoice::Object(obj) => {
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            ToolChoice::Named { name }
        }
    });

    // Build extensions from unknown fields.
    let extra_map: std::collections::HashMap<String, serde_json::Value> = req.extra.clone();
    let extensions = ProviderExtensions {
        openai: if extra_map.is_empty() && req.max_output_tokens.is_none() {
            None
        } else {
            let mut ext = extra_map.clone();
            if let Some(m) = req.max_output_tokens {
                ext.insert("max_output_tokens".into(), serde_json::Value::from(m));
            }
            Some(serde_json::to_value(ext).unwrap_or_default())
        },
        anthropic: None,
    };

    Ok(CanonicalRequest {
        model: req.model,
        messages,
        system,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_output_tokens,
        stop,
        tools,
        tool_choice,
        stream: req.stream,
        response_format,
        metadata: req.metadata,
        extensions,
    })
}

// ─── Adapter: Response encoding (canonical → Responses) ─────────────────────

/// Encode a canonical response into an OpenAI Responses format.
pub fn encode_response(
    resp: &CanonicalResponse,
    created: u64,
) -> Result<ResponsesResponse, ProtocolEngineError> {
    let mut output: Vec<ResponsesItem> = Vec::new();

    for block in &resp.content {
        match block {
            ContentBlock::Text(t) => {
                // Group consecutive text into a single message item.
                output.push(ResponsesItem::Message {
                    id: format!("msg_{}", created),
                    role: "assistant".into(),
                    content: Some(vec![ResponsesContentPart::OutputText {
                        text: t.text.clone(),
                    }]),
                });
            }
            ContentBlock::ToolUse(tu) => {
                let arguments = serde_json::to_string(&tu.input).map_err(|e| {
                    ProtocolEngineError::TranslationFailure {
                        message: format!("failed to serialize tool arguments: {e}"),
                    }
                })?;
                output.push(ResponsesItem::FunctionCall {
                    id: tu.id.clone(),
                    call_id: None,
                    name: tu.name.clone(),
                    arguments,
                });
            }
            ContentBlock::Image(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "image_output".into(),
                    reason: "OpenAI Responses API does not support image output".into(),
                });
            }
            ContentBlock::Audio(_) => {
                return Err(ProtocolEngineError::UnsupportedFeature {
                    feature: "audio_output".into(),
                    reason: "OpenAI Responses API does not support audio output blocks".into(),
                });
            }
            ContentBlock::Reasoning(_) => {
                // Reasoning is provider-internal.
            }
            ContentBlock::ToolResult(_) | ContentBlock::ToolReference(_) => {
                // These don't appear in model responses.
            }
        }
    }

    let status = resp.finish_reason.as_ref().map(encode_status);

    let usage = resp.usage.as_ref().map(|u| ResponsesUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
        input_tokens_details: None,
        output_tokens_details: None,
    });

    Ok(ResponsesResponse {
        id: resp.id.clone(),
        object: "response".into(),
        model: Some(resp.model.clone()),
        output,
        status: status.map(|s| s.to_owned()),
        usage,
    })
}

// ─── Adapter: Stream event encoding (canonical → Responses events) ──────────

/// Encode a canonical stream event into an OpenAI Responses streaming event.
///
/// Returns `None` for events with no Responses API equivalent.
pub fn encode_stream_event(
    event: &CanonicalStreamEvent,
    response_id: &str,
    item_id: &str,
    output_index: u32,
    content_index: u32,
) -> Result<Option<ResponsesStreamEvent>, ProtocolEngineError> {
    match event {
        CanonicalStreamEvent::MessageStart { .. } => {
            Ok(Some(ResponsesStreamEvent::ResponseCreated {
                response: Some(serde_json::json!({
                    "id": response_id,
                    "object": "response",
                    "status": "in_progress",
                })),
            }))
        }
        CanonicalStreamEvent::ContentBlockStart { content_block, .. } => match content_block {
            ContentBlock::Text(_) => Ok(Some(ResponsesStreamEvent::ResponseContentPartAdded {
                item_id: Some(item_id.into()),
                output_index: Some(output_index),
                content_index: Some(content_index),
                part: Some(ResponsesContentPart::OutputText {
                    text: String::new(),
                }),
            })),
            ContentBlock::ToolUse(tu) => Ok(Some(ResponsesStreamEvent::ResponseOutputItemAdded {
                output_index: Some(output_index),
                item: Some(ResponsesItem::FunctionCall {
                    id: tu.id.clone(),
                    call_id: None,
                    name: tu.name.clone(),
                    arguments: String::new(),
                }),
            })),
            _ => Ok(None),
        },
        CanonicalStreamEvent::TextDelta { text, .. } => {
            Ok(Some(ResponsesStreamEvent::ResponseOutputTextDelta {
                item_id: Some(item_id.into()),
                output_index: Some(output_index),
                content_index: Some(content_index),
                delta: Some(text.clone()),
            }))
        }
        CanonicalStreamEvent::ToolCallDelta {
            input_json_delta, ..
        } => {
            if let Some(delta) = input_json_delta {
                Ok(Some(
                    ResponsesStreamEvent::ResponseFunctionCallArgumentsDelta {
                        item_id: Some(item_id.into()),
                        output_index: Some(output_index),
                        delta: Some(delta.clone()),
                    },
                ))
            } else {
                Ok(None)
            }
        }
        CanonicalStreamEvent::AudioDelta { .. } => Ok(None),
        CanonicalStreamEvent::ReasoningDelta { .. } => Ok(None),
        CanonicalStreamEvent::ReasoningSignature { .. } => Ok(None),
        CanonicalStreamEvent::ContentBlockStop { .. } => Ok(None),
        CanonicalStreamEvent::MessageDelta { stop_reason, usage } => {
            let status = stop_reason.as_ref().map(encode_status);
            Ok(Some(ResponsesStreamEvent::ResponseCompleted {
                response: Some(ResponsesResponse {
                    id: response_id.into(),
                    object: "response".into(),
                    model: None,
                    output: vec![],
                    status: status.map(|s| s.to_owned()),
                    usage: usage.as_ref().map(|u| ResponsesUsage {
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                        total_tokens: u.total_tokens,
                        input_tokens_details: None,
                        output_tokens_details: None,
                    }),
                }),
            }))
        }
        CanonicalStreamEvent::Usage { usage } => {
            Ok(Some(ResponsesStreamEvent::ResponseCompleted {
                response: Some(ResponsesResponse {
                    id: response_id.into(),
                    object: "response".into(),
                    model: None,
                    output: vec![],
                    status: Some("completed".into()),
                    usage: Some(ResponsesUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.total_tokens,
                        input_tokens_details: None,
                        output_tokens_details: None,
                    }),
                }),
            }))
        }
        CanonicalStreamEvent::MessageStop => Ok(None),
        CanonicalStreamEvent::Error { message, .. } => Err(ProtocolEngineError::ProviderError {
            message: message.clone(),
            status: None,
        }),
        CanonicalStreamEvent::Ping => Ok(None),
    }
}

// ─── Adapter: Response decoding (Responses → canonical) ──────────────────────

/// Decode an OpenAI Responses API response into a canonical response.
pub fn decode_response(resp: ResponsesResponse) -> Result<CanonicalResponse, ProtocolEngineError> {
    let mut content: Vec<ContentBlock> = Vec::new();

    for item in &resp.output {
        match item {
            ResponsesItem::Message {
                content: Some(parts),
                ..
            } => {
                for part in parts {
                    match part {
                        ResponsesContentPart::OutputText { text } => {
                            content.push(ContentBlock::Text(TextContent { text: text.clone() }));
                        }
                        ResponsesContentPart::InputText { text } => {
                            content.push(ContentBlock::Text(TextContent { text: text.clone() }));
                        }
                        ResponsesContentPart::Other(other) => {
                            let blocks = decode_content_parts(&Some(other.clone()))?;
                            content.extend(blocks);
                        }
                        _ => {}
                    }
                }
            }
            ResponsesItem::Message { content: None, .. } => {}
            ResponsesItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                let input = serde_json::from_str(arguments).unwrap_or_else(|e| {
                    tracing::warn!(call_id = %id, error = %e, "malformed function arguments");
                    serde_json::Value::Object(Default::default())
                });
                content.push(ContentBlock::ToolUse(ToolUseBlock {
                    id: id.clone(),
                    name: name.clone(),
                    input,
                }));
            }
            ResponsesItem::Reasoning { summary, .. } => {
                // Map reasoning summaries into a single reasoning content block.
                if let Some(summaries) = summary {
                    let combined: String = summaries
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    content.push(ContentBlock::Reasoning(ReasoningContent {
                        thinking: combined,
                        signature: None,
                    }));
                }
            }
            ResponsesItem::FunctionCallOutput { .. } => {
                // Function call outputs don't appear in model response content.
            }
            ResponsesItem::Other(_) => {}
        }
    }

    let finish_reason = resp.status.as_deref().map(decode_status);

    let usage = resp.usage.as_ref().map(|u| Usage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    });

    Ok(CanonicalResponse {
        id: resp.id,
        model: resp.model.unwrap_or_default(),
        content,
        finish_reason,
        usage,
        extensions: ProviderExtensions {
            openai: None,
            anthropic: None,
        },
    })
}

// ─── Protocol info ───────────────────────────────────────────────────────────

/// Return the protocol capabilities for OpenAI Responses.
pub fn capabilities() -> ProtocolCapabilities {
    ProtocolCapabilities {
        streaming: true,
        tools: true,
        tool_streaming: true,
        multimodal_input: true,
        structured_output: true,
        // The Responses API exposes `reasoning_effort` and `reasoning.summary`
        // items, but not the raw reasoning tokens.
        reasoning: true,
        usage_streaming: true,
        deferred_tools: false,
    }
}

/// Return the protocol identifier.
pub fn protocol() -> Protocol {
    Protocol::OpenAiResponses
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_declared() {
        let caps = capabilities();
        assert!(caps.streaming);
        assert!(caps.tools);
        assert!(caps.reasoning);
        assert!(caps.multimodal_input);
    }

    #[test]
    fn test_decode_simple_request() {
        let json = r#"{
            "model": "gpt-4",
            "instructions": "You are helpful.",
            "input": [
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
    fn test_decode_string_input() {
        let json = r#"{
            "model": "gpt-4",
            "input": "Hello world"
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
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].as_text(), Some("Hello world"));
    }

    #[test]
    fn test_decode_tool_use_in_input() {
        let json = r#"{
            "model": "gpt-4",
            "input": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": [
                    {"type": "output_text", "text": "Let me check."}
                ]},
                {"type": "function_call_output", "call_id": "fc_123", "output": "NYC: 72F"}
            ],
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
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
        assert_eq!(canonical.tools.len(), 1);
        assert_eq!(canonical.tools[0].name, "get_weather");
        // function_call_output results in Role::Tool message
        assert!(canonical.messages.iter().any(|m| m.role == Role::Tool));
    }

    #[test]
    fn test_encode_simple_response() {
        let resp = CanonicalResponse {
            id: "resp_123".into(),
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

        let responses_resp = match encode_response(&resp, 1234567890) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(responses_resp.id, "resp_123");
        assert_eq!(responses_resp.object, "response");
        assert_eq!(responses_resp.output.len(), 1);
        match &responses_resp.output[0] {
            ResponsesItem::Message { role, content, .. } => {
                assert_eq!(role, "assistant");
                assert_eq!(content.as_ref().map(|c| c.len()), Some(1));
            }
            _ => panic!("expected message item"),
        }
        assert_eq!(responses_resp.status.as_deref(), Some("completed"));
    }

    #[test]
    fn test_encode_tool_call_response() {
        let resp = CanonicalResponse {
            id: "resp_456".into(),
            model: "gpt-4".into(),
            content: vec![ContentBlock::ToolUse(ToolUseBlock {
                id: "fc_xyz".into(),
                name: "search".into(),
                input: serde_json::json!({"query": "rust async"}),
            })],
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
            extensions: ProviderExtensions::default(),
        };

        let responses_resp = match encode_response(&resp, 1234567890) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(responses_resp.output.len(), 1);
        match &responses_resp.output[0] {
            ResponsesItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "fc_xyz");
                assert_eq!(name, "search");
                assert!(arguments.contains("rust async"));
            }
            _ => panic!("expected function_call item"),
        }
        assert_eq!(responses_resp.status.as_deref(), Some("incomplete"));
    }

    #[test]
    fn test_encode_stream_event_text_delta() {
        let event = CanonicalStreamEvent::TextDelta {
            index: 0,
            text: "Hello".into(),
        };
        let result = match encode_stream_event(&event, "resp_1", "msg_1", 0, 0) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        match result {
            Some(ResponsesStreamEvent::ResponseOutputTextDelta { delta, .. }) => {
                assert_eq!(delta.as_deref(), Some("Hello"));
            }
            _ => panic!("expected output_text.delta event"),
        }
    }

    #[test]
    fn test_encode_stream_event_tool_delta() {
        let event = CanonicalStreamEvent::ToolCallDelta {
            index: 0,
            tool_use_id: None,
            name: None,
            input_json_delta: Some(r#"{"city":"NYC""#.into()),
        };
        let result = match encode_stream_event(&event, "resp_1", "msg_1", 0, 0) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        match result {
            Some(ResponsesStreamEvent::ResponseFunctionCallArgumentsDelta { delta, .. }) => {
                assert_eq!(delta.as_deref(), Some(r#"{"city":"NYC""#));
            }
            _ => panic!("expected function_call_arguments.delta event"),
        }
    }

    #[test]
    fn test_round_trip() {
        let original = CanonicalResponse {
            id: "resp_789".into(),
            model: "gpt-4".into(),
            content: vec![ContentBlock::Text(TextContent {
                text: "Round trip!".into(),
            })],
            finish_reason: Some(FinishReason::Stop),
            usage: None,
            extensions: ProviderExtensions::default(),
        };

        let encoded = match encode_response(&original, 0) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let json = match serde_json::to_string(&encoded) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        let decoded: ResponsesResponse = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => panic!("expected success: {e:?}"),
        };
        assert_eq!(decoded.id, "resp_789");
        assert_eq!(decoded.output.len(), 1);
    }
}
