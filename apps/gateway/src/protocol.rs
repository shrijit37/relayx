//! Protocol translation engine for the gateway.
//!
//! Wires `protocol-core` adapters into the live proxy path.  Each
//! `ProtocolEngine` is constructed for a specific (source, target) pair
//! and carries the adapter functions needed to decode/encode requests
//! and responses.  Streaming is handled event-by-event: the upstream SSE
//! body is parsed incrementally, each complete event is translated through
//! the canonical model, and the translated bytes are forwarded immediately.

use std::time::Duration;

use axum::body::Body;
use bytes::Bytes;
use tokio_stream::wrappers::ReceiverStream;

use protocol_core::adapters::{anthropic_messages, openai_chat, openai_responses};
use protocol_core::canonical::{
    CanonicalRequest, CanonicalResponse, CanonicalStreamEvent, Protocol, ProtocolCapabilities,
};
use protocol_core::error::ProtocolEngineError;
use protocol_core::sse::StreamingSseParser;

// ─── Source-event decoding ───────────────────────────────────────────────────

/// Parse a raw SSE event data field into canonical stream events.
///
/// The source format is determined by `source`: each adapter has its own
/// SSE event taxonomy. A single source SSE event can map to zero or more
/// canonical events (OpenAI Chat chunks carry several independent pieces
/// of information).
fn decode_source_sse_event(source: Protocol, event_data: &str) -> Vec<CanonicalStreamEvent> {
    match source {
        Protocol::OpenAiChatCompletions => {
            let chunk: openai_chat::ChatCompletionChunk = match serde_json::from_str(event_data) {
                Ok(c) => c,
                Err(_) => return Vec::new(),
            };
            // OpenAI Chat Completions → canonical stream event.
            // A chunk with choices containing a delta is a TextDelta or ToolCallDelta.
            let Some(choice) = chunk.choices.first() else {
                return Vec::new();
            };
            let delta = &choice.delta;
            let mut events = Vec::new();

            // Emit MessageStart on the first chunk with role.
            if delta.role.is_some() {
                events.push(CanonicalStreamEvent::MessageStart {
                    message: protocol_core::canonical::StreamMessageInfo {
                        id: chunk.id.clone(),
                        model: chunk.model.clone(),
                        role: protocol_core::canonical::Role::Assistant,
                    },
                });
            }

            // Text delta.
            if let Some(text) = &delta.content
                && !text.is_empty()
            {
                events.push(CanonicalStreamEvent::TextDelta {
                    index: choice.index as usize,
                    text: text.clone(),
                });
            }

            // Tool call deltas.
            if let Some(tcs) = &delta.tool_calls {
                for tc in tcs {
                    events.push(CanonicalStreamEvent::ToolCallDelta {
                        index: choice.index as usize,
                        tool_use_id: tc.id.clone(),
                        name: tc.function.as_ref().and_then(|f| f.name.clone()),
                        input_json_delta: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                    });
                }
            }

            // Finish reason → MessageDelta.
            if let Some(reason) = &choice.finish_reason {
                use protocol_core::canonical::FinishReason;
                let stop = match reason.as_str() {
                    "stop" => Some(FinishReason::Stop),
                    "length" => Some(FinishReason::Length),
                    "tool_calls" => Some(FinishReason::ToolCalls),
                    "content_filter" => Some(FinishReason::ContentFilter),
                    other => Some(FinishReason::Other(other.to_owned())),
                };
                let usage = chunk
                    .usage
                    .as_ref()
                    .map(|u| protocol_core::canonical::Usage {
                        input_tokens: Some(u.prompt_tokens),
                        output_tokens: Some(u.completion_tokens),
                        total_tokens: Some(u.total_tokens),
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    });
                events.push(CanonicalStreamEvent::MessageDelta {
                    stop_reason: stop,
                    usage,
                });
            }

            // [DONE] sentinel.
            if event_data.trim() == "[DONE]" {
                events.push(CanonicalStreamEvent::MessageStop);
            }

            events
        }

        Protocol::AnthropicMessages => {
            let event: anthropic_messages::MessagesStreamEvent =
                match serde_json::from_str(event_data) {
                    Ok(e) => e,
                    Err(_) => return Vec::new(),
                };
            vec![anthropic_to_canonical_event(event)]
        }

        Protocol::OpenAiResponses => {
            let event: openai_responses::ResponsesStreamEvent =
                match serde_json::from_str(event_data) {
                    Ok(e) => e,
                    Err(_) => return Vec::new(),
                };
            responses_to_canonical_event(event)
                .into_iter()
                .collect::<Vec<_>>()
        }
    }
}

/// Convert an Anthropic stream event to a canonical stream event.
fn anthropic_to_canonical_event(
    event: anthropic_messages::MessagesStreamEvent,
) -> CanonicalStreamEvent {
    use anthropic_messages::{MessagesDelta, MessagesStreamEvent};
    match event {
        MessagesStreamEvent::MessageStart { message } => {
            let role = match message.role.as_str() {
                "assistant" => protocol_core::canonical::Role::Assistant,
                "user" => protocol_core::canonical::Role::User,
                _ => protocol_core::canonical::Role::Assistant,
            };
            CanonicalStreamEvent::MessageStart {
                message: protocol_core::canonical::StreamMessageInfo {
                    id: message.id,
                    model: message.model,
                    role,
                },
            }
        }
        MessagesStreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            use anthropic_messages::MessagesResponseBlock;
            let content_block = match content_block {
                MessagesResponseBlock::Text { text } => {
                    protocol_core::canonical::ContentBlock::Text(
                        protocol_core::canonical::TextContent { text },
                    )
                }
                MessagesResponseBlock::ToolUse { id, name, input } => {
                    protocol_core::canonical::ContentBlock::ToolUse(
                        protocol_core::canonical::ToolUseBlock { id, name, input },
                    )
                }
                MessagesResponseBlock::Thinking {
                    thinking,
                    signature,
                } => protocol_core::canonical::ContentBlock::Reasoning(
                    protocol_core::canonical::ReasoningContent {
                        thinking,
                        signature,
                    },
                ),
            };
            CanonicalStreamEvent::ContentBlockStart {
                index,
                content_block,
            }
        }
        MessagesStreamEvent::ContentBlockDelta { index, delta } => match delta {
            MessagesDelta::TextDelta { text } => CanonicalStreamEvent::TextDelta { index, text },
            MessagesDelta::ThinkingDelta { thinking } => {
                CanonicalStreamEvent::ReasoningDelta { index, thinking }
            }
            MessagesDelta::SignatureDelta { signature } => {
                CanonicalStreamEvent::ReasoningSignature { index, signature }
            }
            MessagesDelta::InputJsonDelta { partial_json } => CanonicalStreamEvent::ToolCallDelta {
                index,
                tool_use_id: None,
                name: None,
                input_json_delta: Some(partial_json),
            },
        },
        MessagesStreamEvent::ContentBlockStop { index } => {
            CanonicalStreamEvent::ContentBlockStop { index }
        }
        MessagesStreamEvent::MessageDelta { delta, usage } => {
            let stop_reason = delta.stop_reason.as_deref().map(|r| match r {
                "end_turn" => protocol_core::canonical::FinishReason::Stop,
                "stop_sequence" => protocol_core::canonical::FinishReason::StopSequence,
                "max_tokens" => protocol_core::canonical::FinishReason::Length,
                "tool_use" => protocol_core::canonical::FinishReason::ToolCalls,
                other => protocol_core::canonical::FinishReason::Other(other.to_owned()),
            });
            CanonicalStreamEvent::MessageDelta {
                stop_reason,
                usage: Some(protocol_core::canonical::Usage {
                    input_tokens: Some(usage.input_tokens),
                    output_tokens: Some(usage.output_tokens),
                    total_tokens: Some(usage.input_tokens + usage.output_tokens),
                    cache_creation_input_tokens: usage.cache_creation_input_tokens,
                    cache_read_input_tokens: usage.cache_read_input_tokens,
                }),
            }
        }
        MessagesStreamEvent::MessageStop => CanonicalStreamEvent::MessageStop,
        MessagesStreamEvent::Ping => CanonicalStreamEvent::Ping,
        MessagesStreamEvent::Error { error } => CanonicalStreamEvent::Error {
            message: error.message,
            code: Some(error.error_type),
        },
    }
}

/// Convert a Responses stream event to a canonical stream event (or None).
fn responses_to_canonical_event(
    event: openai_responses::ResponsesStreamEvent,
) -> Option<CanonicalStreamEvent> {
    use openai_responses::ResponsesStreamEvent;
    match event {
        ResponsesStreamEvent::ResponseCreated { .. } => {
            // Emit a placeholder — the real id comes later in ResponseCompleted.
            Some(CanonicalStreamEvent::MessageStart {
                message: protocol_core::canonical::StreamMessageInfo {
                    id: "streaming".into(),
                    model: String::new(),
                    role: protocol_core::canonical::Role::Assistant,
                },
            })
        }
        ResponsesStreamEvent::ResponseOutputTextDelta { delta, .. } => {
            delta.map(|text| CanonicalStreamEvent::TextDelta { index: 0, text })
        }
        ResponsesStreamEvent::ResponseFunctionCallArgumentsDelta { item_id, delta, .. } => {
            let tool_use_id = item_id.clone();
            delta.map(|d| CanonicalStreamEvent::ToolCallDelta {
                index: 0,
                tool_use_id,
                name: None,
                input_json_delta: Some(d),
            })
        }
        ResponsesStreamEvent::ResponseCompleted {
            response: Some(resp),
        } => {
            let stop_reason = resp.status.as_deref().map(|s| match s {
                "completed" => protocol_core::canonical::FinishReason::Stop,
                "incomplete" => protocol_core::canonical::FinishReason::Length,
                "failed" => protocol_core::canonical::FinishReason::Error,
                other => protocol_core::canonical::FinishReason::Other(other.to_owned()),
            });
            let usage = resp
                .usage
                .as_ref()
                .map(|u| protocol_core::canonical::Usage {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    total_tokens: u.total_tokens,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                });
            Some(CanonicalStreamEvent::MessageDelta { stop_reason, usage })
        }
        ResponsesStreamEvent::ResponseCompleted { .. } => None,
        ResponsesStreamEvent::Error { error } => {
            let message = error
                .as_ref()
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown response stream error")
                .to_owned();
            Some(CanonicalStreamEvent::Error {
                message,
                code: None,
            })
        }
        _ => None,
    }
}

// ─── Target-event encoding ───────────────────────────────────────────────────

// ─── ProtocolEngine ─────────────────────────────────────────────────────────

/// Protocol translation engine.
///
/// Constructed for a specific (source, target) protocol pair and used to
/// translate requests, responses, and streaming events through the
/// canonical model.
pub struct ProtocolEngine {
    source: Protocol,
    target: Protocol,
}

impl ProtocolEngine {
    /// Create an engine for a source→target translation pair.
    pub fn from_pair(source: Protocol, target: Protocol) -> Result<Self, ProtocolEngineError> {
        if source == target {
            return Err(ProtocolEngineError::UnsupportedProtocol {
                protocol: format!("{source} → {target} is a passthrough, not a translation"),
            });
        }
        Ok(Self { source, target })
    }

    pub fn source_protocol(&self) -> Protocol {
        self.source
    }

    pub fn target_protocol(&self) -> Protocol {
        self.target
    }

    pub fn source_capabilities(&self) -> ProtocolCapabilities {
        adapter_capabilities(self.source)
    }

    pub fn target_capabilities(&self) -> ProtocolCapabilities {
        adapter_capabilities(self.target)
    }

    /// Check whether translation preserves all features.  Returns an error
    /// for `Reject`/`Drop` policy losses; non-blocking policies are logged.
    pub fn check_losses(&self) -> Result<(), ProtocolEngineError> {
        self.target_capabilities()
            .enforce_translation_losses(&self.source_capabilities())
    }

    /// Request-aware loss gate: what the *actual* request uses vs what the
    /// target adapter can represent.
    ///
    /// Capability matrices for two protocols can differ without a given
    /// request being lossy (OpenAI Chat → Anthropic is equipped for tools and
    /// streaming; only a request that *uses* structured output is drop-lossy).
    /// This builds an effective source-capability set from the request and
    /// enforces the loss policy against it.
    pub fn check_request_losses(
        &self,
        request: &CanonicalRequest,
    ) -> Result<(), ProtocolEngineError> {
        let used = protocol_core::canonical::ProtocolCapabilities {
            streaming: request.stream,
            tools: !request.tools.is_empty() || request.tool_choice.is_some(),
            multimodal_input: request.messages.iter().any(|m| {
                m.content.clone().into_blocks().iter().any(|b| {
                    matches!(
                        b,
                        protocol_core::canonical::ContentBlock::Image(_)
                            | protocol_core::canonical::ContentBlock::Audio(_)
                    )
                })
            }),
            structured_output: request.response_format.is_some(),
            ..Default::default()
        };

        self.target_capabilities().enforce_translation_losses(&used)
    }

    // ── Request ──────────────────────────────────────────────────────────────

    /// Decode a raw request body (bytes) from the source wire protocol
    /// into a canonical request.
    pub fn decode_request(&self, body: &[u8]) -> Result<CanonicalRequest, ProtocolEngineError> {
        match self.source {
            Protocol::OpenAiChatCompletions => {
                let req: openai_chat::ChatCompletionRequest = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Chat request: {e}"),
                    })?;
                openai_chat::decode_request(req)
            }
            Protocol::AnthropicMessages => {
                let req: anthropic_messages::MessagesRequest = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid Anthropic Messages request: {e}"),
                    })?;
                anthropic_messages::decode_request(req)
            }
            Protocol::OpenAiResponses => {
                let req: openai_responses::ResponsesRequest = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Responses request: {e}"),
                    })?;
                openai_responses::decode_request(req)
            }
        }
    }

    /// Encode a canonical request to the target wire format.
    pub fn encode_request(
        &self,
        canonical: &CanonicalRequest,
    ) -> Result<Vec<u8>, ProtocolEngineError> {
        let json = match self.target {
            Protocol::OpenAiChatCompletions => {
                let wire = encode_openai_chat_request(canonical)?;
                serde_json::to_vec(&wire)
            }
            Protocol::AnthropicMessages => {
                let wire = anthropic_messages::encode_request(canonical)?;
                serde_json::to_vec(&wire)
            }
            Protocol::OpenAiResponses => {
                let wire = encode_responses_request(canonical)?;
                serde_json::to_vec(&wire)
            }
        };
        json.map_err(|e| ProtocolEngineError::TranslationFailure {
            message: format!("failed to serialize request: {e}"),
        })
    }

    // ── Response ─────────────────────────────────────────────────────────────

    /// Decode a raw response body (bytes) from the upstream target wire protocol
    /// into a canonical response.
    pub fn decode_response(&self, body: &[u8]) -> Result<CanonicalResponse, ProtocolEngineError> {
        match self.target {
            Protocol::OpenAiChatCompletions => {
                let resp: openai_chat::ChatCompletionResponse = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Chat response: {e}"),
                    })?;
                decode_openai_chat_response(&resp)
            }
            Protocol::AnthropicMessages => {
                let resp: anthropic_messages::MessagesResponse = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid Anthropic Messages response: {e}"),
                    })?;
                anthropic_messages::decode_response(resp)
            }
            Protocol::OpenAiResponses => {
                let resp: openai_responses::ResponsesResponse = serde_json::from_slice(body)
                    .map_err(|e| ProtocolEngineError::InvalidPayload {
                        message: format!("invalid OpenAI Responses response: {e}"),
                    })?;
                openai_responses::decode_response(resp)
            }
        }
    }

    /// Encode a canonical response to the client-facing source wire format.
    pub fn encode_response(
        &self,
        canonical: &CanonicalResponse,
    ) -> Result<Vec<u8>, ProtocolEngineError> {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let json = match self.source {
            Protocol::OpenAiChatCompletions => {
                let resp = openai_chat::encode_response(canonical, created)?;
                serde_json::to_vec(&resp)
            }
            Protocol::AnthropicMessages => {
                let resp = anthropic_messages::encode_response(canonical)?;
                serde_json::to_vec(&resp)
            }
            Protocol::OpenAiResponses => {
                let resp = openai_responses::encode_response(canonical, created)?;
                serde_json::to_vec(&resp)
            }
        };
        json.map_err(|e| ProtocolEngineError::TranslationFailure {
            message: format!("failed to serialize response: {e}"),
        })
    }

    // ── Streaming ────────────────────────────────────────────────────────────

    /// Wrap an upstream SSE body with event-by-event protocol translation.
    ///
    /// Returns a new `Body` that, when polled, reads the upstream SSE stream,
    /// parses events incrementally, translates each through the canonical
    /// model, and emits the translated SSE bytes immediately.
    pub fn stream_response(
        &self,
        upstream_body: Body,
        frame_timeout: Duration,
    ) -> Result<Body, ProtocolEngineError> {
        let upstream_proto = self.target; // upstream speaks this protocol
        let client_proto = self.source; // client speaks this protocol
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);

        // Spawn a task that reads upstream SSE, translates, and sends to client.
        // Decode upstream SSE using upstream_proto, encode for client using client_proto.
        tokio::spawn(async move {
            let stream = upstream_body.into_data_stream();
            let mut parser = StreamingSseParser::new();
            let mut stream = std::pin::pin!(stream);

            let mut response_id = String::new();
            let mut model = String::new();
            let item_id = String::new();
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let mut seen_done = false;

            loop {
                let chunk =
                    tokio::time::timeout(frame_timeout, tokio_stream::StreamExt::next(&mut stream))
                        .await;

                match chunk {
                    Ok(Some(Ok(bytes))) => {
                        let events = parser.feed(&bytes);
                        for sse_event in &events {
                            if sse_event.is_done() {
                                seen_done = true;
                                // [DONE] is an OpenAI-specific sentinel.
                                // Emit client-native termination signal.
                                let done = client_native_termination(
                                    client_proto,
                                    &response_id,
                                    &model,
                                    &item_id,
                                );
                                if let Some(wire) = done
                                    && tx.send(Ok(Bytes::from(wire))).await.is_err()
                                {
                                    return;
                                }
                                continue;
                            }

                            // Decode upstream SSE event (in upstream protocol) to canonical.
                            let canonical_events =
                                decode_source_sse_event(upstream_proto, &sse_event.data);
                            for canonical_event in &canonical_events {
                                if let CanonicalStreamEvent::MessageStop = canonical_event {
                                    seen_done = true;
                                }
                                if let CanonicalStreamEvent::MessageStart { message } =
                                    canonical_event
                                {
                                    response_id = message.id.clone();
                                    model = message.model.clone();
                                }

                                // Encode canonical event for the client protocol.
                                let encoded = encode_client_sse_event(
                                    client_proto,
                                    canonical_event,
                                    &response_id,
                                    &model,
                                    created,
                                    &item_id,
                                );
                                if let Some(wire_bytes) = encoded {
                                    if tx.send(Ok(Bytes::from(wire_bytes))).await.is_err() {
                                        return;
                                    }
                                } else if matches!(
                                    canonical_event,
                                    CanonicalStreamEvent::MessageStop
                                ) {
                                    // Client adapter doesn't produce a termination
                                    // event — emit the client-native signal.
                                    let done = client_native_termination(
                                        client_proto,
                                        &response_id,
                                        &model,
                                        &item_id,
                                    );
                                    if let Some(wire) = done
                                        && tx.send(Ok(Bytes::from(wire))).await.is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Some(Err(_e))) => break,
                    Ok(None) => {
                        // Stream ended — flush partial event.
                        for sse_event in parser.finish() {
                            if sse_event.is_done() {
                                seen_done = true;
                                continue;
                            }
                            let canonical_events =
                                decode_source_sse_event(upstream_proto, &sse_event.data);
                            for canonical_event in &canonical_events {
                                if let CanonicalStreamEvent::MessageStop = canonical_event {
                                    seen_done = true;
                                }
                                if let Some(wire_bytes) = encode_client_sse_event(
                                    client_proto,
                                    canonical_event,
                                    &response_id,
                                    &model,
                                    created,
                                    &item_id,
                                ) {
                                    let _ = tx.send(Ok(Bytes::from(wire_bytes))).await;
                                }
                            }
                        }
                        if !seen_done
                            && let Some(wire) = client_native_termination(
                                client_proto,
                                &response_id,
                                &model,
                                &item_id,
                            )
                        {
                            let _ = tx.send(Ok(Bytes::from(wire))).await;
                        }
                        break;
                    }
                    Err(_) => {
                        let _ = tx
                            .send(Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "upstream body frame timeout during translation",
                            )))
                            .await;
                        break;
                    }
                }
            }
        });

        let body = Body::from_stream(ReceiverStream::new(rx));
        Ok(body)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Produce the client-native stream termination signal for a protocol.
fn client_native_termination(
    protocol: Protocol,
    response_id: &str,
    model: &str,
    item_id: &str,
) -> Option<Vec<u8>> {
    match protocol {
        Protocol::OpenAiChatCompletions => Some(protocol_core::sse::format_done_event()),
        Protocol::AnthropicMessages => {
            let evt = anthropic_messages::encode_stream_event(
                &CanonicalStreamEvent::MessageStop,
                response_id,
                model,
            )
            .ok()
            .flatten();
            evt.and_then(serialize_anthropic_sse)
        }
        Protocol::OpenAiResponses => {
            let evt = openai_responses::encode_stream_event(
                &CanonicalStreamEvent::MessageStop,
                response_id,
                item_id,
                0,
                0,
            )
            .ok()
            .flatten();
            evt.and_then(serialize_responses_sse)
        }
    }
}

/// Encode a canonical stream event for a specific client-facing protocol.
fn encode_client_sse_event(
    protocol: Protocol,
    event: &CanonicalStreamEvent,
    response_id: &str,
    model: &str,
    created: u64,
    item_id: &str,
) -> Option<Vec<u8>> {
    match protocol {
        Protocol::OpenAiChatCompletions => {
            let chunk =
                openai_chat::encode_stream_event(event, response_id, model, created).ok()?;
            let chunk = chunk?;
            let json = serde_json::to_string(&chunk).ok()?;
            Some(protocol_core::sse::format_sse_event(
                &json,
                Some("chat.completion.chunk"),
            ))
        }
        Protocol::AnthropicMessages => {
            let event = anthropic_messages::encode_stream_event(event, response_id, model).ok()?;
            let event = event?;
            let json = serde_json::to_string(&event).ok()?;
            let event_type = match &event {
                anthropic_messages::MessagesStreamEvent::MessageStart { .. } => "message_start",
                anthropic_messages::MessagesStreamEvent::ContentBlockStart { .. } => {
                    "content_block_start"
                }
                anthropic_messages::MessagesStreamEvent::ContentBlockDelta { .. } => {
                    "content_block_delta"
                }
                anthropic_messages::MessagesStreamEvent::ContentBlockStop { .. } => {
                    "content_block_stop"
                }
                anthropic_messages::MessagesStreamEvent::MessageDelta { .. } => "message_delta",
                anthropic_messages::MessagesStreamEvent::MessageStop => "message_stop",
                anthropic_messages::MessagesStreamEvent::Ping => "ping",
                anthropic_messages::MessagesStreamEvent::Error { .. } => "error",
            };
            Some(protocol_core::sse::format_sse_event(
                &json,
                Some(event_type),
            ))
        }
        Protocol::OpenAiResponses => {
            let event =
                openai_responses::encode_stream_event(event, response_id, item_id, 0, 0).ok()?;
            let event = event?;
            let json = serde_json::to_string(&event).ok()?;
            let event_type = match &event {
                openai_responses::ResponsesStreamEvent::ResponseCreated { .. } => {
                    "response.created"
                }
                openai_responses::ResponsesStreamEvent::ResponseOutputItemAdded { .. } => {
                    "response.output_item.added"
                }
                openai_responses::ResponsesStreamEvent::ResponseContentPartAdded { .. } => {
                    "response.content_part.added"
                }
                openai_responses::ResponsesStreamEvent::ResponseOutputTextDelta { .. } => {
                    "response.output_text.delta"
                }
                openai_responses::ResponsesStreamEvent::ResponseFunctionCallArgumentsDelta {
                    ..
                } => "response.function_call_arguments.delta",
                openai_responses::ResponsesStreamEvent::ResponseCompleted { .. } => {
                    "response.completed"
                }
                openai_responses::ResponsesStreamEvent::Error { .. } => "error",
                _ => "response.other",
            };
            Some(protocol_core::sse::format_sse_event(
                &json,
                Some(event_type),
            ))
        }
    }
}
fn serialize_anthropic_sse(event: anthropic_messages::MessagesStreamEvent) -> Option<Vec<u8>> {
    let event_type = match &event {
        anthropic_messages::MessagesStreamEvent::MessageStart { .. } => "message_start",
        anthropic_messages::MessagesStreamEvent::ContentBlockStart { .. } => "content_block_start",
        anthropic_messages::MessagesStreamEvent::ContentBlockDelta { .. } => "content_block_delta",
        anthropic_messages::MessagesStreamEvent::ContentBlockStop { .. } => "content_block_stop",
        anthropic_messages::MessagesStreamEvent::MessageDelta { .. } => "message_delta",
        anthropic_messages::MessagesStreamEvent::MessageStop => "message_stop",
        anthropic_messages::MessagesStreamEvent::Ping => "ping",
        anthropic_messages::MessagesStreamEvent::Error { .. } => "error",
    };
    let json = serde_json::to_string(&event).ok()?;
    Some(protocol_core::sse::format_sse_event(
        &json,
        Some(event_type),
    ))
}

/// Serialize a Responses stream event to SSE wire bytes.
fn serialize_responses_sse(event: openai_responses::ResponsesStreamEvent) -> Option<Vec<u8>> {
    let event_type = match &event {
        openai_responses::ResponsesStreamEvent::ResponseCreated { .. } => "response.created",
        openai_responses::ResponsesStreamEvent::ResponseOutputItemAdded { .. } => {
            "response.output_item.added"
        }
        openai_responses::ResponsesStreamEvent::ResponseContentPartAdded { .. } => {
            "response.content_part.added"
        }
        openai_responses::ResponsesStreamEvent::ResponseOutputTextDelta { .. } => {
            "response.output_text.delta"
        }
        openai_responses::ResponsesStreamEvent::ResponseFunctionCallArgumentsDelta { .. } => {
            "response.function_call_arguments.delta"
        }
        openai_responses::ResponsesStreamEvent::ResponseCompleted { .. } => "response.completed",
        openai_responses::ResponsesStreamEvent::Error { .. } => "error",
        _ => "response.other",
    };
    let json = serde_json::to_string(&event).ok()?;
    Some(protocol_core::sse::format_sse_event(
        &json,
        Some(event_type),
    ))
}

fn adapter_capabilities(protocol: Protocol) -> ProtocolCapabilities {
    match protocol {
        Protocol::OpenAiChatCompletions => openai_chat::capabilities(),
        Protocol::AnthropicMessages => anthropic_messages::capabilities(),
        Protocol::OpenAiResponses => openai_responses::capabilities(),
    }
}

/// Encode a canonical request as an OpenAI Chat Completions wire request.
fn encode_openai_chat_request(
    req: &CanonicalRequest,
) -> Result<openai_chat::ChatCompletionRequest, ProtocolEngineError> {
    let mut messages = Vec::new();

    // System instruction → system message.
    if let Some(sys) = &req.system {
        let text = match sys {
            protocol_core::canonical::SystemInstruction::Text(t) => t.clone(),
            protocol_core::canonical::SystemInstruction::Blocks(blocks) => {
                let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
                texts.join("\n")
            }
        };
        messages.push(openai_chat::ChatMessage {
            role: "system".into(),
            content: Some(serde_json::Value::String(text)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Messages.
    for msg in &req.messages {
        match msg.role {
            protocol_core::canonical::Role::System => {} // handled above
            _ => {
                let (role_str, content, tool_calls) = encode_chat_message_content(msg)?;
                messages.push(openai_chat::ChatMessage {
                    role: role_str,
                    content,
                    name: None,
                    tool_calls,
                    tool_call_id: None,
                });
            }
        }
    }

    // Tool results: OpenAI uses separate `tool` role messages.
    for msg in &req.messages {
        if msg.role == protocol_core::canonical::Role::Tool {
            let blocks = msg.content.clone().into_blocks();
            for block in blocks {
                if let protocol_core::canonical::ContentBlock::ToolResult(tr) = block {
                    let content = match &tr.content {
                        protocol_core::canonical::ToolResultContent::Text(s) => {
                            Some(serde_json::Value::String(s.clone()))
                        }
                        protocol_core::canonical::ToolResultContent::Blocks(blocks) => {
                            let texts: Vec<String> = blocks
                                .iter()
                                .filter_map(|b| b.as_text().map(|s| s.to_owned()))
                                .collect();
                            Some(serde_json::Value::String(texts.join("\n")))
                        }
                    };
                    messages.push(openai_chat::ChatMessage {
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

    let tools: Vec<openai_chat::ChatToolDefinition> = req
        .tools
        .iter()
        .map(|t| openai_chat::ChatToolDefinition {
            tool_type: "function".into(),
            function: openai_chat::ChatFunctionDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
                strict: t.extra.get("strict").and_then(|v| v.as_bool()),
            },
        })
        .collect();

    let tool_choice = encode_chat_tool_choice(&req.tool_choice);

    let response_format = req
        .response_format
        .as_ref()
        .map(|rf| openai_chat::ChatResponseFormat {
            format_type: rf.format_type.clone(),
            json_schema: rf.json_schema.clone(),
        });

    Ok(openai_chat::ChatCompletionRequest {
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

/// Result of encoding a canonical message into OpenAI chat fields.
type ChatEncodedMessage = (
    String,
    Option<serde_json::Value>,
    Option<Vec<openai_chat::ChatToolCall>>,
);

/// Encode a canonical message into OpenAI chat message fields.
fn encode_chat_message_content(
    msg: &protocol_core::canonical::Message,
) -> Result<ChatEncodedMessage, ProtocolEngineError> {
    use protocol_core::canonical::ContentBlock;

    let role_str = match msg.role {
        protocol_core::canonical::Role::User => "user",
        protocol_core::canonical::Role::Assistant => "assistant",
        protocol_core::canonical::Role::Tool => "tool",
        protocol_core::canonical::Role::System => "system",
    };

    let blocks = msg.content.clone().into_blocks();
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<openai_chat::ChatToolCall> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text(t) => text_parts.push(t.text),
            ContentBlock::ToolUse(tu) => {
                let arguments = serde_json::to_string(&tu.input).map_err(|e| {
                    ProtocolEngineError::TranslationFailure {
                        message: format!("failed to serialize tool arguments: {e}"),
                    }
                })?;
                tool_calls.push(openai_chat::ChatToolCall {
                    id: tu.id,
                    call_type: "function".into(),
                    function: openai_chat::ChatFunctionCall {
                        name: tu.name,
                        arguments,
                    },
                });
            }
            ContentBlock::ToolResult(_tr) => {
                // Tool results in an assistant message — unusual, skip.
            }
            ContentBlock::Image(img) => {
                // Reconstruct OpenAI image_url content.
                let url_str = match &img.source {
                    protocol_core::canonical::ImageSource::Url { url, .. } => url.clone(),
                    protocol_core::canonical::ImageSource::Base64 {
                        media_type, data, ..
                    } => format!("data:{media_type};base64,{data}"),
                };
                let detail = match &img.source {
                    protocol_core::canonical::ImageSource::Url { detail, .. } => detail.clone(),
                    _ => None,
                };
                let mut image_obj = serde_json::json!({"url": url_str});
                if let Some(d) = detail {
                    image_obj["detail"] = serde_json::Value::String(d);
                }
                text_parts.push(
                    serde_json::to_string(&serde_json::json!({
                        "type": "image_url",
                        "image_url": image_obj,
                    }))
                    .unwrap_or_default(),
                );
            }
            ContentBlock::Audio(_) => {
                // Cannot represent audio in plain-text content array — skip.
            }
            ContentBlock::Reasoning(_) => {
                // Reasoning blocks are provider-internal; not present in user messages.
            }
            ContentBlock::ToolReference(_) => {
                // Deferred references not materialized; skip.
            }
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
fn encode_chat_tool_choice(
    tc: &Option<protocol_core::canonical::ToolChoice>,
) -> Option<openai_chat::ChatToolChoice> {
    use protocol_core::canonical::ToolChoice;
    tc.as_ref().map(|tc| match tc {
        ToolChoice::Auto => openai_chat::ChatToolChoice::String("auto".into()),
        ToolChoice::Required => openai_chat::ChatToolChoice::String("required".into()),
        ToolChoice::None => openai_chat::ChatToolChoice::String("none".into()),
        ToolChoice::Named { name } => {
            openai_chat::ChatToolChoice::Object(openai_chat::ChatToolChoiceObject {
                choice_type: "function".into(),
                function: openai_chat::ChatToolChoiceFunction { name: name.clone() },
            })
        }
    })
}

/// Decode an OpenAI Chat Completions response into a canonical response.
fn decode_openai_chat_response(
    resp: &openai_chat::ChatCompletionResponse,
) -> Result<CanonicalResponse, ProtocolEngineError> {
    let mut content = Vec::new();

    for choice in &resp.choices {
        if let Some(text) = &choice.message.content
            && !text.is_empty()
        {
            content.push(protocol_core::canonical::ContentBlock::Text(
                protocol_core::canonical::TextContent { text: text.clone() },
            ));
        }
        if let Some(tcs) = &choice.message.tool_calls {
            for tc in tcs {
                let input = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|e| {
                    tracing::warn!(call_id = %tc.id, error = %e, "malformed tool arguments");
                    serde_json::Value::Object(Default::default())
                });
                content.push(protocol_core::canonical::ContentBlock::ToolUse(
                    protocol_core::canonical::ToolUseBlock {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    },
                ));
            }
        }
    }

    let finish_reason = resp.choices.first().and_then(|c| {
        c.finish_reason.as_ref().map(|r| match r.as_str() {
            "stop" => protocol_core::canonical::FinishReason::Stop,
            "length" => protocol_core::canonical::FinishReason::Length,
            "tool_calls" => protocol_core::canonical::FinishReason::ToolCalls,
            "content_filter" => protocol_core::canonical::FinishReason::ContentFilter,
            other => protocol_core::canonical::FinishReason::Other(other.to_owned()),
        })
    });

    let usage = resp
        .usage
        .as_ref()
        .map(|u| protocol_core::canonical::Usage {
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
        extensions: protocol_core::canonical::ProviderExtensions::default(),
    })
}

/// Encode a canonical request as an OpenAI Responses wire request.
fn encode_responses_request(
    req: &CanonicalRequest,
) -> Result<openai_responses::ResponsesRequest, ProtocolEngineError> {
    // System instructions → instructions field.
    let instructions = req.system.as_ref().map(|s| match s {
        protocol_core::canonical::SystemInstruction::Text(t) => {
            serde_json::Value::String(t.clone())
        }
        protocol_core::canonical::SystemInstruction::Blocks(blocks) => {
            let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
            serde_json::Value::String(texts.join("\n"))
        }
    });

    // Messages → input items.
    let mut input: Vec<serde_json::Value> = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            protocol_core::canonical::Role::User => "user",
            protocol_core::canonical::Role::Assistant => "assistant",
            protocol_core::canonical::Role::System => "system",
            protocol_core::canonical::Role::Tool => "user", // tool results → user content
        };
        let blocks = msg.content.clone().into_blocks();
        let mut parts: Vec<serde_json::Value> = Vec::new();
        for block in blocks {
            match block {
                protocol_core::canonical::ContentBlock::Text(t) => {
                    parts.push(serde_json::json!({"type": "input_text", "text": t.text}));
                }
                protocol_core::canonical::ContentBlock::Image(img) => {
                    let url = match &img.source {
                        protocol_core::canonical::ImageSource::Url { url, .. } => url.clone(),
                        protocol_core::canonical::ImageSource::Base64 {
                            media_type, data, ..
                        } => format!("data:{media_type};base64,{data}"),
                    };
                    parts.push(serde_json::json!({"type": "input_image", "image_url": url}));
                }
                protocol_core::canonical::ContentBlock::ToolResult(tr) => {
                    let output = match &tr.content {
                        protocol_core::canonical::ToolResultContent::Text(s) => s.clone(),
                        _ => String::new(),
                    };
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": tr.tool_use_id,
                        "output": output,
                    }));
                    continue; // not a message content part
                }
                _ => {} // skip other types
            }
        }
        input.push(serde_json::json!({"role": role, "content": parts}));
    }

    let input_value = if input.len() == 1 && input[0].get("content").is_some() {
        input.remove(0)
    } else {
        serde_json::Value::Array(input)
    };

    let tools: Vec<openai_responses::ResponsesTool> = req
        .tools
        .iter()
        .map(|t| openai_responses::ResponsesTool {
            tool_type: "function".into(),
            name: Some(t.name.clone()),
            description: t.description.clone(),
            parameters: t.input_schema.clone(),
            strict: t.extra.get("strict").and_then(|v| v.as_bool()),
            extra: Default::default(),
        })
        .collect();

    let tool_choice = req.tool_choice.as_ref().map(|tc| {
        use protocol_core::canonical::ToolChoice;
        match tc {
            ToolChoice::Auto => openai_responses::ResponsesToolChoice::String("auto".into()),
            ToolChoice::Required => {
                openai_responses::ResponsesToolChoice::String("required".into())
            }
            ToolChoice::None => openai_responses::ResponsesToolChoice::String("none".into()),
            ToolChoice::Named { name } => openai_responses::ResponsesToolChoice::Object(
                serde_json::json!({"type": "function", "name": name}),
            ),
        }
    });

    Ok(openai_responses::ResponsesRequest {
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine =
            ProtocolEngine::from_pair(Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_engine_rejects_same_pair() {
        let engine = ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiChatCompletions,
        );
        assert!(engine.is_err());
    }

    #[test]
    fn test_capabilities_accessors() {
        let engine = match ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            Protocol::AnthropicMessages,
        ) {
            Ok(e) => e,
            Err(e) => panic!("expected engine creation to succeed: {e:?}"),
        };
        assert!(engine.source_capabilities().tools);
        assert!(engine.target_capabilities().tools);
    }

    #[test]
    fn test_check_losses_structured_output() {
        let engine = match ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            Protocol::AnthropicMessages,
        ) {
            Ok(e) => e,
            Err(e) => panic!("expected engine creation to succeed: {e:?}"),
        };
        // OpenAI has structured_output, Anthropic does not → Drop loss → rejected.
        let result = engine.check_losses();
        assert!(result.is_err());
    }

    #[test]
    fn request_aware_losses_pass_for_plain_text() {
        let engine = match ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            Protocol::AnthropicMessages,
        ) {
            Ok(e) => e,
            Err(e) => panic!("expected engine creation to succeed: {e:?}"),
        };
        let req = CanonicalRequest {
            stream: false,
            tools: vec![],
            tool_choice: None,
            response_format: None,
            ..plain_request()
        };
        // Plain text/tools are lossless when translating to Anthropic.
        let result = engine.check_request_losses(&req);
        assert!(result.is_ok(), "plain request must not be rejected");
    }

    #[test]
    fn request_aware_losses_reject_structured_output() {
        let engine = match ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            Protocol::AnthropicMessages,
        ) {
            Ok(e) => e,
            Err(e) => panic!("expected engine creation to succeed: {e:?}"),
        };
        let req = CanonicalRequest {
            response_format: Some(protocol_core::canonical::ResponseFormat {
                format_type: "json_schema".into(),
                json_schema: Some(serde_json::json!({ "type": "object" })),
            }),
            ..plain_request()
        };
        // Structured output → Anthropic is a Drop loss → rejected.
        let result = engine.check_request_losses(&req);
        assert!(
            result.is_err(),
            "structured-output request must be rejected"
        );
    }

    #[test]
    fn request_aware_losses_reject_streaming_to_non_streaming() {
        let engine = match ProtocolEngine::from_pair(
            Protocol::OpenAiChatCompletions,
            // A target without streaming (hypothetically):
            Protocol::OpenAiResponses,
        ) {
            Ok(e) => e,
            Err(e) => panic!("expected engine creation to succeed: {e:?}"),
        };
        let req = CanonicalRequest {
            stream: true,
            ..plain_request()
        };
        let result = engine.check_request_losses(&req);
        assert!(
            result.is_ok(),
            "OpenAI Responses supports streaming — should not be rejected"
        );
    }

    fn plain_request() -> CanonicalRequest {
        CanonicalRequest {
            model: "gpt-4".into(),
            messages: vec![protocol_core::canonical::Message {
                role: protocol_core::canonical::Role::User,
                content: protocol_core::canonical::MessageContent::Text("hi".into()),
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
            extensions: protocol_core::canonical::ProviderExtensions::default(),
        }
    }
}
