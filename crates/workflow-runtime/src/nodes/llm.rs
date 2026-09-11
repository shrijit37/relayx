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
    let lane_id = config.lane_id.clone().unwrap_or_else(|| "default".into());

    // Resolve the lane.
    let lane = ctx
        .lane_registry
        .get(&lane_id)
        .ok_or_else(|| NodeError::Internal(format!("lane not found: {lane_id}")))?;

    // Get the upstream HTTP client.
    let client = ctx
        .upstream_client
        .clone()
        .ok_or_else(|| NodeError::Internal("no upstream client configured".into()))?;

    tracing::debug!(
        node_id = %ctx.node_id,
        protocol = ?protocol,
        model = %model,
        lane = %lane_id,
        "LLM node executing"
    );

    // Build a canonical request from the input.
    let canonical = build_canonical_request(config, protocol, model, &input.value)?;

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
    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .map_err(|e| NodeError::Internal(format!("failed to read response body: {e}")))?;
    let body = body.to_bytes();

    if !status.is_success() {
        let body_text = String::from_utf8_lossy(&body).into_owned();
        return Err(NodeError::Provider(
            protocol_core::error::ProtocolEngineError::ProviderError {
                message: format!("provider returned {status}: {body_text}"),
            },
        ));
    }

    // Decode the response through the protocol engine.
    let canonical_response = decode_response(target_protocol, &body)?;

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
