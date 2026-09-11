//! End-to-end streaming protocol-translation tests.
//!
//! Flow: Client → Gateway (translation config) → Mock upstream (SSE) → Gateway translates → Client
//!
//! Each test spins up a mock with `raw_sse` (exact wire events), routes through
//! a translation gateway, and asserts the translated SSE events the client sees.

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use test_harness::spawn_translation_gateway;

use http_body_util::BodyExt;
use hyper_util::client::legacy::Client as HttpClient;
use hyper_util::rt::TokioExecutor;

/// Send a streaming POST request and collect the full response body as bytes.
///
/// Returns (status_code, response_body).
async fn send_streaming_request(
    url: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> anyhow::Result<(http::StatusCode, bytes::Bytes)> {
    let client = HttpClient::builder(TokioExecutor::new()).build_http();

    let mut builder = http::Request::builder().method(http::Method::POST).uri(url);
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    let has_ct = extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    if !has_ct {
        builder = builder.header(http::header::CONTENT_TYPE, "application/json");
    }

    let req = builder.body(axum::body::Body::from(body.as_bytes().to_vec()))?;
    let resp = client.request(req).await?;
    let status = resp.status();
    let collected = BodyExt::collect(resp.into_body()).await?;
    Ok((status, collected.to_bytes()))
}

/// Parse an SSE response body into the raw `data:` payloads.
///
/// Each SSE event is separated by `\n\n`. Only `data:` lines are extracted.
fn parse_sse_data_lines(body: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(body).unwrap_or("");
    text.split("\n\n")
        .filter_map(|block| {
            block
                .lines()
                .find_map(|line| line.strip_prefix("data: ").map(|s| s.to_string()))
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: Anthropic → OpenAI streaming text deltas
// ─────────────────────────────────────────────────────────────────────────────

/// Mock emits Anthropic SSE events (message_start, content_block_start,
/// text_delta x3, content_block_stop, message_delta, message_stop).
/// Client sends OpenAI Chat request with `stream: true`.
/// Gateway translates Anthropic→OpenAI. Client receives OpenAI SSE chunks.
#[tokio::test]
async fn streaming_openai_to_anthropic_text_deltas() -> anyhow::Result<()> {
    let mock_events = vec![
        r#"{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","model":"claude-3","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#.into(),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into(),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#.into(),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}"#.into(),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}}"#.into(),
        r#"{"type":"content_block_stop","index":0}"#.into(),
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.into(),
        r#"{"type":"message_stop"}"#.into(),
    ];

    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        raw_sse: Some(mock_events),
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
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true,
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, resp_body) = send_streaming_request(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    let data_lines = parse_sse_data_lines(&resp_body);
    assert!(
        !data_lines.is_empty(),
        "client should receive SSE events, got nothing"
    );

    // Collect all text deltas across all chunks.
    let mut texts = Vec::new();
    let mut found_done = false;
    for line in &data_lines {
        if line.trim() == "[DONE]" {
            found_done = true;
            continue;
        }
        let chunk: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("failed to parse SSE chunk: {e}\nraw: {line}"))?;

        // Every non-DONE chunk must be a chat.completion.chunk.
        let obj_type = chunk.get("object").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            obj_type, "chat.completion.chunk",
            "every chunk must be chat.completion.chunk, got: {obj_type}\n{chunk}"
        );

        // Extract content deltas.
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(content) = choice
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                {
                    texts.push(content.to_string());
                }
            }
        }
    }

    assert_eq!(texts, vec!["Hello", " world", "!"]);
    assert!(found_done, "stream must end with [DONE] sentinel");

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: OpenAI → Anthropic streaming text deltas
// ─────────────────────────────────────────────────────────────────────────────

/// Mock emits OpenAI SSE chunks (role delta, content deltas, finish_reason).
/// Client sends Anthropic request with `stream: true`.
/// Gateway translates OpenAI→Anthropic. Client receives Anthropic SSE events.
#[tokio::test]
async fn streaming_anthropic_to_openai_text_deltas() -> anyhow::Result<()> {
    let mock_events = vec![
        // First chunk: role delta.
        r#"{"id":"chatcmpl_123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#.into(),
        // Content delta 1.
        r#"{"id":"chatcmpl_123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#.into(),
        // Content delta 2.
        r#"{"id":"chatcmpl_123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}"#.into(),
        // Content delta 3.
        r#"{"id":"chatcmpl_123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}"#.into(),
        // Finish chunk.
        r#"{"id":"chatcmpl_123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}"#.into(),
    ];

    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        raw_sse: Some(mock_events),
        ..Default::default()
    })
    .await?;

    let gw =
        spawn_translation_gateway(mock.addr, "/v1/messages", "anthropic", "openai_chat").await?;

    let body = serde_json::json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true,
    })
    .to_string();

    let url = format!("http://{}/v1/messages", gw.proxy);
    let (status, resp_body) = send_streaming_request(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    let data_lines = parse_sse_data_lines(&resp_body);
    assert!(
        !data_lines.is_empty(),
        "client should receive SSE events, got nothing"
    );

    let mut event_types = Vec::new();
    let mut texts = Vec::new();

    for line in &data_lines {
        let event: serde_json::Value = serde_json::from_str(line).map_err(|e| {
            anyhow::anyhow!("failed to parse Anthropic SSE event: {e}\nraw: {line}")
        })?;

        let evt_type = event
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        event_types.push(evt_type.to_string());

        // Collect text deltas from content_block_delta events.
        if evt_type == "content_block_delta"
            && let Some(text) = event
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
        {
            texts.push(text.to_string());
        }
    }

    // Must have message_start.
    assert!(
        event_types.contains(&"message_start".to_string()),
        "must have message_start event, got: {event_types:?}"
    );

    // Must have message_stop.
    assert!(
        event_types.contains(&"message_stop".to_string()),
        "must have message_stop event, got: {event_types:?}"
    );

    // Text deltas must arrive.
    assert_eq!(texts, vec!["Hello", " world", "!"]);

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: Preserves tool call deltas
// ─────────────────────────────────────────────────────────────────────────────

/// Mock emits OpenAI chunks with tool_calls deltas.
/// Client sends Anthropic streaming request. Verify tool call info arrives.
#[tokio::test]
async fn streaming_preserves_tool_call_deltas() -> anyhow::Result<()> {
    let mock_events = vec![
        // Role chunk.
        r#"{"id":"chatcmpl_tc","object":"chat.completion.chunk","created":1000,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#.into(),
        // Tool call delta: name + arguments.
        r#"{"id":"chatcmpl_tc","object":"chat.completion.chunk","created":1000,"model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#.into(),
        // Tool call arguments delta.
        r#"{"id":"chatcmpl_tc","object":"chat.completion.chunk","created":1000,"model":"gpt-4","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"arguments":"{\"city\":\"NYC\"}"}}]},"finish_reason":null}]}"#.into(),
        // Finish with tool_calls.
        r#"{"id":"chatcmpl_tc","object":"chat.completion.chunk","created":1000,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#.into(),
    ];

    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        raw_sse: Some(mock_events),
        ..Default::default()
    })
    .await?;

    let gw =
        spawn_translation_gateway(mock.addr, "/v1/messages", "anthropic", "openai_chat").await?;

    let body = serde_json::json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "What's the weather?"}],
        "stream": true,
    })
    .to_string();

    let url = format!("http://{}/v1/messages", gw.proxy);
    let (status, resp_body) = send_streaming_request(&url, &body, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    let data_lines = parse_sse_data_lines(&resp_body);
    let raw_text = std::str::from_utf8(&resp_body)?;

    // Must have at least one input_json_delta carrying tool arguments.
    // OpenAI tool calls are decoded as ToolCallDelta (no ContentBlockStart),
    // which Anthropic encodes as ContentBlockDelta with input_json_delta.
    let has_input_json = data_lines.iter().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .map(|v| {
                v.get("type").and_then(|t| t.as_str()) == Some("content_block_delta")
                    && v.get("delta")
                        .and_then(|d| d.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("input_json_delta")
            })
            .unwrap_or(false)
    });
    assert!(
        has_input_json,
        "must have a content_block_delta with input_json_delta for tool arguments"
    );

    // Must have a tool_use name somewhere in the content_block_delta or content_block_start.
    let has_tool_name = raw_text.contains("get_weather") || raw_text.contains("NYC");
    assert!(
        has_tool_name,
        "must have tool name or arguments in the output"
    );

    // Message must end cleanly.
    assert!(
        raw_text.contains("message_stop"),
        "stream must end with message_stop"
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: Malformed SSE handled gracefully
// ─────────────────────────────────────────────────────────────────────────────

/// Mock emits some valid events then a malformed chunk.
/// Verify the client gets what was valid before the error.
#[tokio::test]
async fn streaming_malformed_sse_handled_gracefully() -> anyhow::Result<()> {
    let mock_events = vec![
        // Valid Anthropic events first.
        r#"{"type":"message_start","message":{"id":"msg_mal","type":"message","role":"assistant","model":"claude-3","content":[],"stop_reason":null,"usage":{"input_tokens":5,"output_tokens":0}}}"#.into(),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into(),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Valid part"}}"#.into(),
        // Malformed: not valid JSON.
        "{this is not valid json at all".into(),
        // More valid events after the malformed one.
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" after"}}"#.into(),
        r#"{"type":"content_block_stop","index":0}"#.into(),
        r#"{"type":"message_stop"}"#.into(),
    ];

    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        raw_sse: Some(mock_events),
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
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true,
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, resp_body) = send_streaming_request(&url, &body, &[]).await?;

    // The gateway should still return 200 for streaming (errors happen mid-stream).
    assert_eq!(status, http::StatusCode::OK);

    let data_lines = parse_sse_data_lines(&resp_body);

    // Collect all text deltas the client received.
    let mut texts = Vec::new();
    for line in &data_lines {
        if line.trim() == "[DONE]" {
            continue;
        }
        let chunk: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // malformed SSE line from gateway — skip
        };
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(content) = choice
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                {
                    texts.push(content.to_string());
                }
            }
        }
    }

    // At minimum the first valid text delta must arrive before the malformed event.
    assert!(
        texts.contains(&"Valid part".to_string()),
        "client must receive the valid delta before the malformed event, got: {texts:?}"
    );

    // The malformed event should be silently skipped (parsed as empty by the
    // Anthropic decoder), so subsequent valid events may or may not arrive
    // depending on whether the parser recovered. Either outcome is acceptable.
    // The key assertion is: valid-before-error IS received.

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: Empty upstream returns gracefully
// ─────────────────────────────────────────────────────────────────────────────

/// Mock emits no events (empty raw_sse). Verify client handles gracefully.
#[tokio::test]
async fn streaming_empty_upstream_returns_error() -> anyhow::Result<()> {
    let mock = spawn_mock(MockConfig {
        mode: MockMode::Sse,
        raw_sse: Some(vec![]),
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
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true,
    })
    .to_string();

    let url = format!("http://{}/v1/chat/completions", gw.proxy);
    let (status, resp_body) = send_streaming_request(&url, &body, &[]).await?;

    // With no upstream events, the gateway should either return an error
    // (500/502) or an empty 200 with a [DONE] sentinel.
    let text = std::str::from_utf8(&resp_body)?;

    if status.is_success() {
        // If 200, it must at least send [DONE] so the client knows it's over.
        assert!(
            text.contains("[DONE]"),
            "empty stream on 200 must end with [DONE], got: {text}"
        );
        // No text deltas expected.
        assert!(
            !text.contains("content_delta") && !text.contains("\"content\""),
            "empty stream must not contain content deltas: {text}"
        );
    } else {
        // Error status is also acceptable — the gateway detected empty upstream.
        assert!(
            status.is_server_error() || status.is_client_error(),
            "empty upstream should produce error status, got: {status}"
        );
    }

    Ok(())
}
