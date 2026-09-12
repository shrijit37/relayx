//! LLM node — calls an LLM provider via the protocol engine.
//!
//! Resolution path:
//!
//! ```text
//! LLM Node
//!    ↓
//! resolve configuration
//!    ↓
//! resolve lane
//!    ↓
//! ProtocolEngine
//!    ↓
//! HTTP POST to provider
//! ```
//!
//! The node builds a canonical request from its configuration and input
//! messages, encodes it to the target protocol, sends it over the lane's
//! connection pool, decodes the response through the protocol engine, and
//! returns the canonical response as JSON.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
use protocol_core::adapters::openai_chat;
use protocol_core::canonical::{CanonicalRequest, Protocol};
use workflow_schema::LlmConfig;

/// Execute an LLM node — sends a real request to the provider via HTTP.
pub async fn execute(
    config: &LlmConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    let protocol =
        resolve_protocol(config).ok_or_else(|| NodeError::Internal("unknown protocol".into()))?;
    let model = config.model.clone().unwrap_or_else(|| "default".into());

    // A lane-less LLM node is valid only when the registry has exactly one
    // lane (that lane is the unambiguous default). `None` with 0 or many
    // lanes is a configuration error, not a surprise "default" lookup.
    let lane_id = match config.lane_id.clone() {
        Some(id) => id,
        None => {
            let ids: Vec<&String> = ctx.lane_registry.iter().map(|(id, _)| id).collect();
            match ids.as_slice() {
                [single] => (*single).clone(),
                [] => {
                    return Err(NodeError::Internal(
                        "LLM node has no lane_id and no lane is registered".into(),
                    ));
                }
                _ => {
                    return Err(NodeError::Internal(format!(
                        "LLM node has no lane_id but {} lanes exist; a lane_id is required",
                        ids.len()
                    )));
                }
            }
        }
    };

    // Resolve the lane.
    let lane = ctx
        .lane_registry
        .get(&lane_id)
        .ok_or_else(|| NodeError::Internal(format!("lane not found: {lane_id}")))?;

    // Prefer the lane-bound connection pool (per-lane isolation); fall back to
    // the shared client (Phase-1 single-pool deployments).
    let client = match ctx
        .lane_clients
        .as_ref()
        .and_then(|lc| lc.client_for_lane(&lane_id))
    {
        Some(c) => c,
        None => ctx
            .upstream_client
            .clone()
            .ok_or_else(|| NodeError::Internal("no upstream client configured".into()))?,
    };

    tracing::debug!(
        node_id = %ctx.node_id,
        protocol = ?protocol,
        model = %model,
        lane = %lane_id,
        "LLM node executing"
    );

    // Build a canonical request from the input.
    let mut canonical = build_canonical_request(config, protocol, model, &input.value)?;
    canonical.stream = config.stream;

    // Encode to the target protocol wire format using the protocol engine.
    let target_protocol = protocol_target(&protocol);
    let wire = encode_request(target_protocol, &canonical)?;

    // Build the URL.
    let url = lane
        .base_url
        .join("/v1/chat/completions")
        .map_err(|e| NodeError::Internal(format!("invalid lane URL: {e}")))?;

    // Build the HTTP request.
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url.as_str())
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(wire))
        .map_err(|e| NodeError::Internal(format!("failed to build request: {e}")))?;

    // Send the request with timeout.
    let response = tokio::select! {
        result = client.request(req) => result,
        _ = ctx.cancel_token.cancelled() => {
            return Err(NodeError::Internal("cancelled".into()));
        }
    }
    .map_err(|e| {
        NodeError::Provider(protocol_core::error::ProtocolEngineError::ProviderError {
            message: format!("upstream request failed: {e}"),
        })
    })?;

    let status = response.status();

    if !status.is_success() {
        // Buffer the error body for a readable message, then surface it.
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .map_err(|e| NodeError::Internal(format!("failed to read error body: {e}")))?
            .to_bytes();
        let body_text = String::from_utf8_lossy(&body).into_owned();
        return Err(NodeError::Provider(
            protocol_core::error::ProtocolEngineError::ProviderError {
                message: format!("provider returned {status}: {body_text}"),
            },
        ));
    }

    let canonical_response = if config.stream {
        decode_streamed_response_incremental(
            axum::body::Body::new(response.into_body()),
            &ctx.cancel_token,
        )
        .await?
    } else {
        // Non-streaming: buffer the full body, decode the JSON response.
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .map_err(|e| NodeError::Internal(format!("failed to read response body: {e}")))?
            .to_bytes();
        decode_response(target_protocol, &body)?
    };

    tracing::debug!(
        node_id = %ctx.node_id,
        response_id = %canonical_response.id,
        model = %canonical_response.model,
        content_blocks = canonical_response.content.len(),
        "LLM node received response"
    );

    // Serialize the canonical response as JSON.
    let json = serde_json::to_value(&canonical_response)
        .map_err(|e| NodeError::Internal(format!("failed to serialize response: {e}")))?;

    Ok(NodeOutput::message(RuntimeValue::Json(json)))
}

/// Resolve the protocol from configuration.
fn resolve_protocol(config: &LlmConfig) -> Option<Protocol> {
    config
        .protocol
        .as_deref()
        .and_then(|p| match p {
            "openai_chat" | "openai_chat_completions" => Some(Protocol::OpenAiChatCompletions),
            "anthropic" | "anthropic_messages" => Some(Protocol::AnthropicMessages),
            "openai_responses" => Some(Protocol::OpenAiResponses),
            _ => None,
        })
        .or(Some(Protocol::OpenAiChatCompletions))
}

/// The protocol the provider (target) speaks. For MVP, OpenAI Chat Completions.
fn protocol_target(_source: &Protocol) -> Protocol {
    Protocol::OpenAiChatCompletions
}

/// Build a canonical request from configuration and input.
fn build_canonical_request(
    config: &LlmConfig,
    protocol: Protocol,
    model: String,
    input: &RuntimeValue,
) -> Result<CanonicalRequest, NodeError> {
    // Extract messages from the input.
    let messages = extract_messages(input);

    let canonical = CanonicalRequest {
        model,
        messages,
        system: None,
        temperature: config.temperature,
        top_p: None,
        max_tokens: config.max_tokens,
        stop: vec![],
        tools: vec![],
        tool_choice: None,
        stream: false, // MVP: non-streaming; streaming is a refinement.
        response_format: None,
        metadata: None,
        extensions: protocol_core::canonical::ProviderExtensions::default(),
    };

    tracing::debug!(
        protocol = ?protocol,
        messages = canonical.messages.len(),
        model = %canonical.model,
        "LLM node built canonical request"
    );

    Ok(canonical)
}

/// Extract canonical messages from a runtime value.
fn extract_messages(input: &RuntimeValue) -> Vec<protocol_core::canonical::Message> {
    use protocol_core::canonical::{Message, MessageContent, Role};

    // If the input contains a "messages" array, use it.
    if let RuntimeValue::Json(json) = input
        && let Some(messages) = json.get("messages").and_then(|m| m.as_array())
    {
        let mut parsed = Vec::new();
        for m in messages {
            let role = match m.get("role").and_then(|r| r.as_str()) {
                Some("system") => Role::System,
                Some("assistant") => Role::Assistant,
                Some("tool") => Role::Tool,
                _ => Role::User,
            };
            let content = m
                .get("content")
                .map(|c| match c {
                    serde_json::Value::String(s) => MessageContent::Text(s.clone()),
                    other => MessageContent::Text(serde_json::to_string(other).unwrap_or_default()),
                })
                .unwrap_or_else(|| MessageContent::Text(String::new()));
            parsed.push(Message { role, content });
        }
        return parsed;
    }

    // Otherwise, treat the entire input as a user message.
    let text = input.as_text().map(|s| s.to_owned()).unwrap_or_default();
    vec![Message {
        role: Role::User,
        content: MessageContent::Text(text),
    }]
}

/// Encode a canonical request to the target protocol.
fn encode_request(target: Protocol, canonical: &CanonicalRequest) -> Result<Vec<u8>, NodeError> {
    match target {
        Protocol::OpenAiChatCompletions => {
            let req = openai_chat::encode_request(canonical)?;
            serde_json::to_vec(&req)
                .map_err(|e| NodeError::Internal(format!("failed to encode request: {e}")))
        }
        Protocol::AnthropicMessages => {
            let req = protocol_core::adapters::anthropic_messages::encode_request(canonical)?;
            serde_json::to_vec(&req)
                .map_err(|e| NodeError::Internal(format!("failed to encode request: {e}")))
        }
        Protocol::OpenAiResponses => {
            // OpenAI Responses encoding not yet in adapter; use OpenAI Chat as wire format.
            let req = openai_chat::encode_request(canonical)?;
            serde_json::to_vec(&req)
                .map_err(|e| NodeError::Internal(format!("failed to encode request: {e}")))
        }
    }
}

/// Incrementally decode a streamed (SSE) LLM response body into a canonical
/// response.
///
/// Reads the body frame-by-frame (bounded memory — never a full-body buffer),
/// feeding each frame through the SSE parser and folding the deltas into a
/// single response. Cancellation is checked per frame, and a frame-timeout
/// guard bounds an endless/keep-alive stream.
async fn decode_streamed_response_incremental(
    body: axum::body::Body,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
    use protocol_core::sse::StreamingSseParser;

    let mut parser = StreamingSseParser::new();
    let mut fold = StreamFold::default();

    let mut stream = std::pin::pin!(body.into_data_stream());
    loop {
        // Bound each frame wait so a dead-but-open stream can't hang forever.
        let frame = tokio::select! {
            _ = cancel.cancelled() => return Err(NodeError::Internal("cancelled".into())),
            frame = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio_stream::StreamExt::next(&mut stream),
            ) => frame,
        };

        match frame {
            Ok(Some(Ok(bytes))) => {
                for event in parser.feed(&bytes) {
                    fold.fold(&event);
                }
            }
            Ok(Some(Err(e))) => {
                return Err(NodeError::Internal(format!(
                    "failed to read streamed response: {e}"
                )));
            }
            Ok(None) => break, // stream ended cleanly
            Err(_elapsed) => {
                return Err(NodeError::Internal(
                    "frame timeout while reading streamed response".into(),
                ));
            }
        }
    }
    for event in parser.finish() {
        fold.fold(&event);
    }

    fold.into_response()
}

/// Accumulator for folding SSE events into a canonical response.
#[derive(Default)]
struct StreamFold {
    response_id: String,
    model: String,
    text: String,
    stop_reason: Option<protocol_core::canonical::FinishReason>,
    usage: Option<protocol_core::canonical::Usage>,
    tool_slots: std::collections::HashMap<u32, ToolAccum>,
}

impl StreamFold {
    /// Fold one parsed SSE event.
    fn fold(&mut self, event: &protocol_core::sse::SseEvent) {
        if event.is_done() {
            return;
        }
        use protocol_core::adapters::openai_chat::ChatCompletionChunk;
        let chunk: ChatCompletionChunk = match serde_json::from_str(&event.data) {
            Ok(c) => c,
            // Non-ChatCompletions events (keep-alives, other protocols) are
            // skipped — the response stream they belong to is still valid.
            Err(_) => return,
        };
        if !chunk.id.is_empty() {
            self.response_id = chunk.id.clone();
        }
        if !chunk.model.is_empty() {
            self.model = chunk.model.clone();
        }
        if let Some(chunk_usage) = chunk.usage.as_ref() {
            self.usage = Some(protocol_core::canonical::Usage {
                input_tokens: Some(chunk_usage.prompt_tokens),
                output_tokens: Some(chunk_usage.completion_tokens),
                total_tokens: Some(chunk_usage.total_tokens),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            });
        }
        for choice in &chunk.choices {
            if let Some(reason) = &choice.finish_reason {
                self.stop_reason = Some(match reason.as_str() {
                    "stop" => protocol_core::canonical::FinishReason::Stop,
                    "length" => protocol_core::canonical::FinishReason::Length,
                    "tool_calls" => protocol_core::canonical::FinishReason::ToolCalls,
                    _ => protocol_core::canonical::FinishReason::Other(reason.clone()),
                });
            }
            if let Some(delta) = &choice.delta.content {
                self.text.push_str(delta);
            }
            if let Some(tcs) = &choice.delta.tool_calls {
                for tc in tcs {
                    self.tool_slots
                        .entry(tc.index)
                        .or_insert_with(|| ToolAccum {
                            id: tc.id.clone().unwrap_or_else(|| "tool-use".into()),
                            name: String::new(),
                            args: String::new(),
                        });
                    let slot = match self.tool_slots.get_mut(&tc.index) {
                        Some(s) => s,
                        None => continue,
                    };
                    if let Some(name) = tc.function.as_ref().and_then(|f| f.name.clone()) {
                        slot.name = name;
                    }
                    if let Some(args) = tc.function.as_ref().and_then(|f| f.arguments.clone()) {
                        slot.args.push_str(&args);
                    }
                }
            }
        }
    }

    /// Build the canonical response from accumulated state.
    fn into_response(mut self) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
        let mut tool_uses: Vec<protocol_core::canonical::ToolUseBlock> =
            Vec::with_capacity(self.tool_slots.len());
        let mut slots: Vec<(u32, ToolAccum)> = self.tool_slots.drain().collect();
        slots.sort_by_key(|(idx, _)| *idx);
        for (_, acc) in slots {
            let input = serde_json::from_str::<serde_json::Value>(&acc.args)
                .unwrap_or(serde_json::Value::String(acc.args));
            tool_uses.push(protocol_core::canonical::ToolUseBlock {
                id: acc.id,
                name: acc.name,
                input,
            });
        }

        let mut content: Vec<protocol_core::canonical::ContentBlock> = Vec::new();
        if !self.text.is_empty() {
            content.push(protocol_core::canonical::ContentBlock::Text(
                protocol_core::canonical::TextContent { text: self.text },
            ));
        }
        content.extend(
            tool_uses
                .into_iter()
                .map(protocol_core::canonical::ContentBlock::ToolUse),
        );

        Ok(protocol_core::canonical::CanonicalResponse {
            id: self.response_id,
            model: self.model,
            content,
            finish_reason: self.stop_reason,
            usage: self.usage,
            extensions: protocol_core::canonical::ProviderExtensions::default(),
        })
    }
}

/// Accumulates a single tool-call in a streaming response by delta index.
struct ToolAccum {
    id: String,
    name: String,
    args: String,
}

/// Decode a canonical response from the target protocol response body.
fn decode_response(
    target: Protocol,
    body: &[u8],
) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
    match target {
        Protocol::OpenAiChatCompletions => {
            let resp: openai_chat::ChatCompletionResponse =
                serde_json::from_slice(body).map_err(|e| {
                    NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Chat response: {e}"),
                    })
                })?;
            Ok(openai_chat::decode_response(&resp)?)
        }
        Protocol::AnthropicMessages => {
            let resp: protocol_core::adapters::anthropic_messages::MessagesResponse =
                serde_json::from_slice(body).map_err(|e| {
                    NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
                        message: format!("invalid Anthropic Messages response: {e}"),
                    })
                })?;
            Ok(protocol_core::adapters::anthropic_messages::decode_response(resp)?)
        }
        Protocol::OpenAiResponses => {
            let resp: protocol_core::adapters::openai_responses::ResponsesResponse =
                serde_json::from_slice(body).map_err(|e| {
                    NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Responses response: {e}"),
                    })
                })?;
            Ok(protocol_core::adapters::openai_responses::decode_response(
                resp,
            )?)
        }
    }
}
