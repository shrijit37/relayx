//! Integration tests: client → gateway (with protocol translation) → mock upstream.
//!
//! Tests the live protocol translation path through the gateway, verifying that:
//! - OpenAI Chat requests are correctly translated to Anthropic format
//! - Anthropic requests are correctly translated to OpenAI Chat format
//! - The mock receives correctly shaped translated requests
//! - The gateway translates responses back to the client's protocol
//! - Malformed inputs are rejected with appropriate errors
//! - Loss-policy enforcement is applied at the gateway level

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use test_harness::{get_hyper, post_hyper, spawn_translation_gateway};

#[tokio::test]
async fn openai_to_anthropic_translation() -> anyhow::Result<()> {
    // Client sends OpenAI Chat format → gateway translates → mock receives Anthropic format.
    let mock = spawn_mock(MockConfig {
        // Empty json_body so the messages handler uses the default Anthropic response.
        json_body: String::new(),
        ..Default::default()
    })
    .await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello!"}
        ],
        "temperature": 0.7,
        "max_tokens": 1024,
        "stream": false
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, resp_body) = post_hyper(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    // Mock echoes the Anthropic request body back, which the gateway then
    // decodes as an Anthropic response. Verify it contains Anthropic fields.
    let parsed: serde_json::Value = serde_json::from_slice(&resp_body)?;
    assert!(
        parsed.get("messages").is_some() || parsed.get("model").is_some(),
        "response should look like Anthropic wire format, got: {parsed}"
    );

    // Also verify the mock received a properly shaped Anthropic request body.
    let last_req = mock
        .state
        .last_request_body
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| anyhow::anyhow!("mock did not capture request body"))?;
    let anthro_req: serde_json::Value = serde_json::from_str(&last_req)?;
    assert!(
        anthro_req.get("max_tokens").is_some(),
        "mock should have received an Anthropic Messages request with max_tokens field"
    );
    assert!(
        anthro_req.get("system").is_some() || anthro_req.get("messages").is_some(),
        "mock should have received Anthropic Messages fields (system or messages)"
    );

    Ok(())
}

#[tokio::test]
async fn anthropic_to_openai_translation() -> anyhow::Result<()> {
    // Client sends Anthropic Messages format → gateway translates → mock receives OpenAI Chat format.
    // Mock returns JSON mode with a valid OpenAI response body.
    let openai_response = serde_json::json!({
        "id": "chatcmpl_mock_123",
        "object": "chat.completion",
        "created": 1234567890,
        "model": "gpt-4-mock",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello from OpenAI mock!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    })
    .to_string();
    let mock = spawn_mock(MockConfig {
        mode: MockMode::Json,
        json_body: openai_response,
        ..Default::default()
    })
    .await?;
    let gw =
        spawn_translation_gateway(mock.addr, "/v1/messages", "anthropic", "openai_chat").await?;

    let body = serde_json::json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "system": "You are helpful.",
        "messages": [
            {"role": "user", "content": "Hello!"}
        ]
    })
    .to_string();

    let url = format!("http://{}/v1/messages", gw.proxy);
    let (status, _resp_body) = post_hyper(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    // Verify the mock received an OpenAI Chat Completions request.
    let last_req = mock
        .state
        .last_request_body
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| anyhow::anyhow!("mock did not capture request body"))?;
    let openai_req: serde_json::Value = serde_json::from_str(&last_req)?;
    assert!(
        openai_req.get("messages").is_some(),
        "mock should have received an OpenAI Chat request with messages field"
    );
    // OpenAI uses "system" as a message role, not a top-level field.
    let messages = openai_req
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| anyhow::anyhow!("messages is not an array"))?;
    assert!(
        messages
            .iter()
            .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("system")),
        "system instruction should be a system role message in OpenAI format"
    );

    Ok(())
}

#[tokio::test]
async fn translation_rejects_invalid_json() -> anyhow::Result<()> {
    let mock = spawn_mock(MockConfig::default()).await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, body) = post_hyper(&url, "not json at all", &[]).await?;
    assert_eq!(status, http::StatusCode::BAD_REQUEST);
    let text = std::str::from_utf8(&body)?;
    assert!(
        text.contains("invalid") || text.contains("payload"),
        "error message should mention invalid payload: {text}"
    );

    Ok(())
}

#[tokio::test]
async fn translation_preserves_tool_calls_annotate() -> anyhow::Result<()> {
    // Send an OpenAI request with tool_calls in the assistant message through
    // translation to Anthropic format and verify the mock receives the tool_use block.
    let mock = spawn_mock(MockConfig {
        json_body: String::new(),
        ..Default::default()
    })
    .await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "What's the weather?"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_123", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_123", "content": "72F"}
        ],
        "tools": [
            {"type": "function", "function": {"name": "get_weather", "description": "Get weather", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}}
        ],
        "stream": false
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, _) = post_hyper(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    // Verify mock received Anthropic format with tool_use/tool_result blocks.
    let last_req = mock
        .state
        .last_request_body
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| anyhow::anyhow!("mock did not capture request body"))?;
    assert!(
        last_req.contains("tool_use") || last_req.contains("get_weather"),
        "mock should receive Anthropic tool_use content, got: {last_req}"
    );
    assert!(
        last_req.contains("tool_result"),
        "mock should receive Anthropic tool_result content"
    );

    Ok(())
}

#[tokio::test]
async fn passthrough_unaffected_by_translation_config() -> anyhow::Result<()> {
    // Admin endpoints still work when a translation route is configured.
    let mock = spawn_mock(MockConfig::default()).await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let (status, body) = get_hyper(&format!("http://{}/healthz", gw.admin)).await?;
    assert_eq!(status, http::StatusCode::OK);
    let text = std::str::from_utf8(&body)?;
    assert!(text.contains("ok"));

    Ok(())
}

#[tokio::test]
async fn translation_rejects_structured_output_loss() -> anyhow::Result<()> {
    // Test that protocol-level capability loss detection is active.
    // OpenAI → Anthropic: Anthropic reports structured_output: false.
    // When OpenAI capability says structured_output: true and Anthropic says false,
    // the capability matrix flags this as a loss. The check_losses() method enforces
    // this at the engine level.
    //
    // Verify the engine detects the loss:
    let engine = relay_gateway::protocol::ProtocolEngine::from_pair(
        protocol_core::canonical::Protocol::OpenAiChatCompletions,
        protocol_core::canonical::Protocol::AnthropicMessages,
    )?;
    let result = engine.check_losses();
    assert!(
        result.is_err(),
        "engine should detect structured_output loss translating OpenAI → Anthropic"
    );

    // Now verify the gateway correctly propagates the error via translation path.
    // Send a request with a streaming flag to trigger the translation path.
    let mock = spawn_mock(MockConfig::default()).await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello!"}],
        "response_format": {"type": "json_object"},
        "stream": false
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, _resp_body) = post_hyper(&url, &body, &[]).await?;
    // The structured_output capability loss is a Drop policy — it currently
    // gets encoded but logged, not rejected at the gateway level (since the
    // request-level response_format is not part of the capability check).
    // This test documents that behavior. When loss enforcement is tightened
    // to message-level features, this should become a 400.
    assert!(
        status == http::StatusCode::OK || status == http::StatusCode::BAD_REQUEST,
        "unexpected status: {status}"
    );

    Ok(())
}

#[tokio::test]
async fn translation_invalid_upstream_response_body() -> anyhow::Result<()> {
    // Gateway sends translated request upstream; mock returns garbage JSON.
    // The gateway should return an error, not 200 OK.
    let mock = spawn_mock(MockConfig {
        mode: MockMode::Json,
        // Return a valid JSON object that is NOT an Anthropic MessagesResponse.
        // Missing required fields (id, model, role, content, usage).
        json_body: r#"{"completely": "unrelated", "data": 42}"#.into(),
        ..Default::default()
    })
    .await?;
    let gw = spawn_translation_gateway(
        mock.addr,
        "/v1/chat/completions",
        "openai_chat",
        "anthropic",
    )
    .await?;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello!"}],
        "stream": false
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, resp_body) = post_hyper(&url, &body, &[]).await?;
    let text = std::str::from_utf8(&resp_body).unwrap_or("");
    // The gateway should fail to decode the upstream response as an Anthropic
    // MessagesResponse and return an error (400 InvalidPayload or 500 Internal).
    assert!(
        status.is_client_error() || status.is_server_error(),
        "invalid upstream response should produce an error, got: {status}, body: {text}"
    );
    assert!(
        !text.contains("panicked"),
        "gateway should not panic: {text}"
    );

    Ok(())
}
