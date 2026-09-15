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

use std::sync::Arc;

use crate::context::{ExecutionContext, GatewayHttpClient};
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
    let lane_id = resolve_lane_id(config, ctx)?;

    // Resolve the lane.
    let lane = ctx
        .lane_registry
        .get(&lane_id)
        .ok_or_else(|| NodeError::Internal(format!("lane not found: {lane_id}")))?;

    // Resolve the HTTP client (per-lane pool or shared fallback).
    let client = resolve_client(ctx, &lane_id)?;

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

    // Build the HTTP request.
    let req = build_http_request(wire, lane, target_protocol)?;

    // Send the request with timeout and cancellation support.
    let response = send_request_with_timeout(&client, req, &ctx.cancel_token).await?;

    if response.status().is_success() {
        let canonical_response = decode_response_body(
            response,
            &ctx.cancel_token,
            ctx.deadline,
            target_protocol,
            config.stream,
            ctx.token_sender.clone(),
        )
        .await?;

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
    } else {
        // The admin /run handler reliably sends a terminal `error` SSE event
        // for any Err, so the node layer must NOT emit its own duplicate.
        Err(handle_error_response(response).await)
    }
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

/// The protocol the provider (target) speaks.
fn protocol_target(source: &Protocol) -> Protocol {
    *source
}

/// Resolve the lane ID from configuration or registry.
fn resolve_lane_id(config: &LlmConfig, ctx: &ExecutionContext) -> Result<String, NodeError> {
    match config.lane_id.clone() {
        Some(id) => Ok(id),
        None => {
            let ids: Vec<&String> = ctx.lane_registry.iter().map(|(id, _)| id).collect();
            match ids.as_slice() {
                [single] => Ok((*single).clone()),
                [] => Err(NodeError::Internal(
                    "LLM node has no lane_id and no lane is registered".into(),
                )),
                _ => Err(NodeError::Internal(format!(
                    "LLM node has no lane_id but {} lanes exist; a lane_id is required",
                    ids.len()
                ))),
            }
        }
    }
}

/// Resolve the HTTP client for a given lane.
fn resolve_client(
    ctx: &ExecutionContext,
    lane_id: &str,
) -> Result<Arc<GatewayHttpClient>, NodeError> {
    // Prefer the lane-bound connection pool (per-lane isolation); fall back to
    // the shared client (Phase-1 single-pool deployments).
    match ctx
        .lane_clients
        .as_ref()
        .and_then(|lc| lc.client_for_lane(lane_id))
    {
        Some(c) => Ok(c),
        None => ctx
            .upstream_client
            .clone()
            .ok_or_else(|| NodeError::Internal("no upstream client configured".into())),
    }
}

/// Build an HTTP request for the given wire bytes, lane, and protocol.
fn build_http_request(
    wire: Vec<u8>,
    lane: &crate::context::LaneEntry,
    target_protocol: Protocol,
) -> Result<hyper::Request<axum::body::Body>, NodeError> {
    let path = match target_protocol {
        Protocol::OpenAiChatCompletions => "/v1/chat/completions",
        Protocol::AnthropicMessages => "/v1/messages",
        Protocol::OpenAiResponses => "/v1/responses",
    };
    let url = lane
        .base_url
        .join(path)
        .map_err(|e| NodeError::Internal(format!("invalid lane URL: {e}")))?;

    let mut builder = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url.as_str())
        .header(http::header::CONTENT_TYPE, "application/json");
    // Anthropic requires an explicit API version header.
    if target_protocol == Protocol::AnthropicMessages {
        builder = builder.header("anthropic-version", "2023-06-01");
    }
    // Attach the lane's resolved authorization header when the lane carries
    // one (control-plane credential resolution, never in workflow JSON).
    if let Some(auth) = &lane.authorization {
        builder = builder.header(http::header::AUTHORIZATION, auth);
    }
    builder
        .body(axum::body::Body::from(wire))
        .map_err(|e| NodeError::Internal(format!("failed to build request: {e}")))
}

/// Send an HTTP request with timeout and cancellation support.
async fn send_request_with_timeout(
    client: &GatewayHttpClient,
    req: hyper::Request<axum::body::Body>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<hyper::Response<hyper::body::Incoming>, NodeError> {
    tokio::select! {
        result = client.request(req) => result,
        _ = cancel.cancelled() => {
            return Err(NodeError::Internal("cancelled".into()));
        }
    }
    .map_err(|e| {
        NodeError::Provider(protocol_core::error::ProtocolEngineError::ProviderError {
            message: format!("upstream request failed: {e}"),
        })
    })
}

/// Buffer the error body of a failed response and surface it as a ProviderError.
async fn handle_error_response(response: hyper::Response<hyper::body::Incoming>) -> NodeError {
    let status = response.status();
    let body = match http_body_util::BodyExt::collect(response.into_body()).await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            return NodeError::Internal(format!("failed to read error body: {e}"));
        }
    };
    let body_text = String::from_utf8_lossy(&body).into_owned();
    NodeError::Provider(protocol_core::error::ProtocolEngineError::ProviderError {
        message: format!("provider returned {status}: {body_text}"),
    })
}

/// Decode a successful response body into a canonical response.
async fn decode_response_body(
    response: hyper::Response<hyper::body::Incoming>,
    cancel: &tokio_util::sync::CancellationToken,
    deadline: Option<tokio::time::Instant>,
    target_protocol: Protocol,
    stream: bool,
    wire_tx: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>,
) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
    if stream {
        return decode_streamed_response_incremental(
            axum::body::Body::new(response.into_body()),
            cancel,
            deadline,
            target_protocol,
            wire_tx,
        )
        .await;
    }
    // Non-streaming: buffer the full body, decompress gzip, decode the JSON.
    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .map_err(|e| NodeError::Internal(format!("failed to read response body: {e}")))?
        .to_bytes();

    // Fail closed on non-JSON output (mirrors the buffered /run path): a
    // silent null would mask corrupted upstream data.
    let raw_bytes: Vec<u8> = if body.starts_with(b"\x1f\x8b") {
        inflate(&body)?
    } else {
        body.to_vec()
    };
    let json: serde_json::Value = serde_json::from_slice(&raw_bytes).map_err(|e| {
        NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
            message: format!("invalid provider JSON response: {e}"),
        })
    })?;

    if !json.is_object() && !json.is_array() {
        return Err(NodeError::Provider(
            protocol_core::error::ProtocolEngineError::InvalidPayload {
                message: "provider returned non-object JSON response".into(),
            },
        ));
    }

    match &json {
        // A 2xx with an `error` object is a streamable protocol-level error
        // (OpenAI-style); surface it instead of treating it as success.
        serde_json::Value::Object(o) if o.contains_key("error") && o.get("error").is_some() => {
            return Err(NodeError::Provider(
                protocol_core::error::ProtocolEngineError::ProviderError {
                    message: format!("provider returned error at 2xx: {json}"),
                },
            ));
        }
        _ => {}
    }

    decode_response(target_protocol, json)
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
    use protocol_core::canonical::{ContentBlock, Message, MessageContent, Role};

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
                    // Multimodal input: an array of content blocks. Preserve each
                    // typed block so downstream adapters keep the source semantics.
                    serde_json::Value::Array(blocks) => {
                        let blocks: Vec<ContentBlock> =
                            blocks.iter().filter_map(parse_content_block).collect();
                        MessageContent::Blocks(blocks)
                    }
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

/// Map a JSON content block into its matching canonical variant. Unrecognized
/// block types are skipped so a partial multimodal payload degrades to the
/// blocks that can be represented, rather than failing the whole node.
fn parse_content_block(
    block: &serde_json::Value,
) -> Option<protocol_core::canonical::ContentBlock> {
    use protocol_core::canonical::{
        AudioContent, AudioSource, ContentBlock, ImageContent, ImageSource, TextContent,
    };

    let block_type = block.get("type").and_then(|t| t.as_str())?;
    match block_type {
        "text" => Some(ContentBlock::Text(TextContent {
            text: block
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_owned(),
        })),
        "image_url" => Some(ContentBlock::Image(ImageContent {
            source: ImageSource::Url {
                url: block
                    .get("image_url")
                    .and_then(|u| u.get("url"))
                    .and_then(|u| u.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                detail: block
                    .get("image_url")
                    .and_then(|u| u.get("detail"))
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_owned()),
            },
        })),
        "input_image" | "image" => {
            let source = block.get("image_url").or_else(|| block.get("source"));
            let url = source.and_then(|s| s.get("url")).and_then(|u| u.as_str());
            let media_type = source
                .and_then(|s| s.get("media_type"))
                .and_then(|m| m.as_str());
            let data = source.and_then(|s| s.get("data")).and_then(|d| d.as_str());
            match (url, data) {
                (Some(url), _) => Some(ContentBlock::Image(ImageContent {
                    source: ImageSource::Url {
                        url: url.to_owned(),
                        detail: None,
                    },
                })),
                (_, Some(_)) => Some(ContentBlock::Image(ImageContent {
                    source: ImageSource::Base64 {
                        media_type: media_type.unwrap_or_default().to_owned(),
                        data: data.unwrap_or_default().to_owned(),
                    },
                })),
                _ => None,
            }
        }
        "input_audio" | "audio" => {
            let source = block.get("input_audio").or_else(|| block.get("source"));
            let data = source.and_then(|s| s.get("data")).and_then(|d| d.as_str());
            let media_type = source
                .and_then(|s| s.get("format"))
                .and_then(|f| f.as_str());
            let format = source
                .and_then(|s| s.get("format"))
                .and_then(|f| f.as_str());
            data.map(|data| {
                ContentBlock::Audio(AudioContent {
                    source: AudioSource::Base64 {
                        media_type: media_type.unwrap_or("audio/wav").to_owned(),
                        data: data.to_owned(),
                        format: format.map(|s| s.to_owned()),
                    },
                })
            })
        }
        _ => None,
    }
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
            let req = protocol_core::adapters::openai_responses::encode_request(canonical)?;
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
/// guard bounds an endless/keep-alive stream. The target protocol drives how
/// each SSE event is folded (OpenAI Chat chunks vs Anthropic stream events vs
/// Responses events).
async fn decode_streamed_response_incremental(
    body: axum::body::Body,
    cancel: &tokio_util::sync::CancellationToken,
    deadline: Option<tokio::time::Instant>,
    protocol: Protocol,
    wire_tx: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>,
) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
    use protocol_core::sse::StreamingSseParser;

    let mut parser = StreamingSseParser::new();
    let mut fold = StreamFold::new(protocol, wire_tx);

    let mut stream = std::pin::pin!(body.into_data_stream());
    loop {
        // Bound each frame wait so a dead-but-open stream can't hang forever.
        // Also enforce the run's ABSOLUTE deadline each frame: an upstream
        // that keeps a stream alive one frame at a time (each under the 30s
        // frame window) must not run past the workflow's overall deadline.
        let frame = tokio::select! {
            _ = cancel.cancelled() => return Err(NodeError::Internal("cancelled".into())),
            _ = timeout_deadline(deadline) => {
                return Err(NodeError::Internal("workflow deadline exceeded".into()))
            }
            frame = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio_stream::StreamExt::next(&mut stream),
            ) => frame,
        };

        match frame {
            Ok(Some(Ok(bytes))) => {
                for event in parser.feed(&bytes) {
                    fold.fold(&event).await;
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
        fold.fold(&event).await;
    }

    fold.into_response()
}

/// A future that wakes when the run deadline is reached (or never, if none).
async fn timeout_deadline(deadline: Option<tokio::time::Instant>) {
    if let Some(d) = deadline {
        tokio::time::sleep_until(d).await;
    } else {
        std::future::pending::<()>().await;
    }
}

/// Accumulator for folding SSE events into a canonical response.
struct StreamFold {
    protocol: Protocol,
    response_id: String,
    model: String,
    text: String,
    stop_reason: Option<protocol_core::canonical::FinishReason>,
    usage: Option<protocol_core::canonical::Usage>,
    tool_slots: std::collections::HashMap<u32, ToolAccum>,
    /// Optional channel to forward raw SSE wire bytes for token-level streaming.
    wire_tx: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>,
}

impl StreamFold {
    fn new(protocol: Protocol, wire_tx: Option<tokio::sync::mpsc::Sender<bytes::Bytes>>) -> Self {
        Self {
            protocol,
            response_id: String::new(),
            model: String::new(),
            text: String::new(),
            stop_reason: None,
            usage: None,
            tool_slots: std::collections::HashMap::new(),
            wire_tx,
        }
    }

    /// Emit a token SSE event through the wire channel, blocking on
    /// backpressure instead of silently dropping tokens.
    ///
    /// Inlines the SSE wire format directly: pre-formatting avoids a
    /// `serde_json::json!` + `to_string()` + `Bytes` allocation per token.
    async fn emit_token(&self, delta: &str) {
        if let Some(ref tx) = self.wire_tx {
            // Worst case: every byte is escaped (\u00XX) → 6 bytes, plus
            // the fixed SSE framing overhead of ~30 bytes.
            let mut wire = Vec::with_capacity(delta.len() * 6 + 32);
            wire.extend_from_slice(b"event: token\ndata: {\"delta\":\"");
            for byte in delta.bytes() {
                match byte {
                    b'"' => wire.extend_from_slice(b"\\\""),
                    b'\\' => wire.extend_from_slice(b"\\\\"),
                    b'\n' => wire.extend_from_slice(b"\\n"),
                    b'\r' => wire.extend_from_slice(b"\\r"),
                    b'\t' => wire.extend_from_slice(b"\\t"),
                    b if b < 0x20 => {
                        // Control characters: \u00XX (4 hex digits, no ambiguity).
                        wire.extend_from_slice(
                            format!("\\u{:04x}", b).as_bytes(),
                        );
                    }
                    b => wire.push(b),
                }
            }
            wire.extend_from_slice(b"\"}\n\n");
            let _ = tx.send(bytes::Bytes::from(wire)).await;
        }
    }

    /// Fold one parsed SSE event.
    async fn fold(&mut self, event: &protocol_core::sse::SseEvent) {
        if event.is_done() {
            return;
        }
        match self.protocol {
            Protocol::OpenAiResponses => self.fold_responses(event).await,
            Protocol::AnthropicMessages => self.fold_anthropic(event).await,
            Protocol::OpenAiChatCompletions => self.fold_chat(event).await,
        }
    }

    async fn fold_chat(&mut self, event: &protocol_core::sse::SseEvent) {
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
                self.emit_token(delta).await;
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

    /// Fold an Anthropic stream event. Anthropic SSE events carry a `type`
    /// discriminator: `message_start`, `content_block_delta`, `message_delta`,
    /// `message_stop`. Only text deltas / usage / stop are folded here; tool
    /// delta accumulation is delegated to the canonical fold path.
    async fn fold_anthropic(&mut self, event: &protocol_core::sse::SseEvent) {
        use protocol_core::adapters::anthropic_messages::MessagesStreamEvent;
        let parsed: MessagesStreamEvent = match serde_json::from_str(&event.data) {
            Ok(e) => e,
            Err(_) => return,
        };
        match parsed {
            MessagesStreamEvent::MessageStart { message } => {
                self.response_id = message.id;
                self.model = message.model;
            }
            MessagesStreamEvent::ContentBlockDelta { delta, .. } => {
                use protocol_core::adapters::anthropic_messages::MessagesDelta;
                match delta {
                    MessagesDelta::TextDelta { text } => {
                        self.text.push_str(&text);
                        self.emit_token(&text).await;
                    }
                    MessagesDelta::InputJsonDelta { partial_json } => {
                        // Tool-call JSON accumulation — store for later parsing.
                        if let Some(last) = self.tool_slots.values_mut().last() {
                            last.args.push_str(&partial_json);
                        }
                    }
                    _ => {}
                }
            }
            MessagesStreamEvent::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason.as_deref() {
                    self.stop_reason = Some(match reason {
                        "end_turn" => protocol_core::canonical::FinishReason::Stop,
                        "max_tokens" => protocol_core::canonical::FinishReason::Length,
                        "tool_use" => protocol_core::canonical::FinishReason::ToolCalls,
                        other => protocol_core::canonical::FinishReason::Other(other.to_owned()),
                    });
                }
                self.usage = Some(protocol_core::canonical::Usage {
                    input_tokens: Some(usage.input_tokens),
                    output_tokens: Some(usage.output_tokens),
                    total_tokens: None,
                    cache_creation_input_tokens: usage.cache_creation_input_tokens,
                    cache_read_input_tokens: usage.cache_read_input_tokens,
                });
            }
            _ => {}
        }
    }

    /// Fold an OpenAI Responses stream event. Responses events use
    /// `response.output_text.delta` for text deltas and
    /// `response.completed` for usage / final status.
    async fn fold_responses(&mut self, event: &protocol_core::sse::SseEvent) {
        use protocol_core::adapters::openai_responses::ResponsesStreamEvent;
        let parsed: ResponsesStreamEvent = match serde_json::from_str(&event.data) {
            Ok(e) => e,
            Err(_) => return,
        };
        match parsed {
            ResponsesStreamEvent::ResponseCreated { response } => {
                if let Some(r) = response.as_ref()
                    && let Some(id) = r.get("id").and_then(|v| v.as_str())
                {
                    self.response_id = id.to_owned();
                }
                if let Some(r) = response.as_ref()
                    && let Some(model) = r.get("model").and_then(|v| v.as_str())
                {
                    self.model = model.to_owned();
                }
            }
            ResponsesStreamEvent::ResponseOutputTextDelta { delta, .. } => {
                if let Some(text) = delta
                    && !text.is_empty()
                {
                    self.text.push_str(&text);
                    self.emit_token(&text).await;
                }
            }
            _ => {}
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

/// Decompress a gzip payload (some providers deliver gzipped response bodies
/// even without an explicit Content-Encoding, e.g. OpenAI Responses).
fn inflate(compressed: &[u8]) -> Result<Vec<u8>, NodeError> {
    let mut decoder = flate2::read::GzDecoder::new(compressed);
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut out)
        .map_err(|e| NodeError::Internal(format!("failed to decompress gzip response: {e}")))?;
    Ok(out)
}

/// Decode a canonical response from the target protocol response JSON.
fn decode_response(
    target: Protocol,
    payload: serde_json::Value,
) -> Result<protocol_core::canonical::CanonicalResponse, NodeError> {
    match target {
        Protocol::OpenAiChatCompletions => {
            let resp: openai_chat::ChatCompletionResponse =
                serde_json::from_value(payload).map_err(|e| {
                    NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Chat response: {e}"),
                    })
                })?;
            Ok(openai_chat::decode_response(&resp)?)
        }
        Protocol::AnthropicMessages => {
            let resp: protocol_core::adapters::anthropic_messages::MessagesResponse =
                serde_json::from_value(payload).map_err(|e| {
                    NodeError::Provider(protocol_core::error::ProtocolEngineError::InvalidPayload {
                        message: format!("invalid Anthropic Messages response: {e}"),
                    })
                })?;
            Ok(protocol_core::adapters::anthropic_messages::decode_response(resp)?)
        }
        Protocol::OpenAiResponses => {
            let resp: protocol_core::adapters::openai_responses::ResponsesResponse =
                serde_json::from_value(payload).map_err(|e| {
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
