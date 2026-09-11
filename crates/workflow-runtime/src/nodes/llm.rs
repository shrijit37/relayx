//! LLM node — calls an LLM provider via the protocol engine.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use protocol_core::canonical::Protocol;
use workflow_schema::LlmConfig;

/// Execute an LLM node.
pub async fn execute(
    config: &LlmConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    let _protocol = config
        .protocol
        .as_deref()
        .and_then(|p| match p {
            "openai_chat" | "openai_chat_completions" => Some(Protocol::OpenAiChatCompletions),
            "anthropic" | "anthropic_messages" => Some(Protocol::AnthropicMessages),
            "openai_responses" => Some(Protocol::OpenAiResponses),
            _ => None,
        })
        .unwrap_or(Protocol::OpenAiChatCompletions);

    let _model = config.model.as_deref().unwrap_or("default");
    let _lane_id = config.lane_id.as_deref().unwrap_or("default");

    tracing::debug!(
        node_id = %ctx.node_id,
        protocol = ?_protocol,
        model = %_model,
        lane = %_lane_id,
        stream = config.stream,
        "LLM node executing"
    );

    // Build a canonical request from the input.
    let input_json_str = serde_json::to_string(&input.to_json()).unwrap_or_default();
    let input_text = input.as_text().unwrap_or(&input_json_str);

    let _canonical = protocol_core::canonical::CanonicalRequest {
        model: _model.to_owned(),
        messages: vec![protocol_core::canonical::Message {
            role: protocol_core::canonical::Role::User,
            content: protocol_core::canonical::MessageContent::text(input_text),
        }],
        system: None,
        temperature: config.temperature,
        top_p: None,
        max_tokens: config.max_tokens,
        stop: vec![],
        tools: vec![],
        tool_choice: None,
        stream: config.stream,
        response_format: None,
        metadata: None,
        extensions: protocol_core::canonical::ProviderExtensions::default(),
    };

    // In a full implementation, this would:
    // 1. Resolve the lane from ctx.lane_registry
    // 2. Create a ProtocolEngine for source→target translation
    // 3. Encode the request for the target protocol
    // 4. Forward to the provider via the connection pool
    // 5. Decode the response back to canonical

    // For now, return the input as output (passthrough).
    Ok(NodeOutput::Message(serde_json::json!({
        "node": ctx.node_id,
        "input_received": input.to_json(),
        "status": "llm_node_stub",
    })))
}
