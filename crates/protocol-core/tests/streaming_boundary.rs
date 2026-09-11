//! SSE streaming boundary tests.
//!
//! Tests that the SSE parser correctly handles:
//! - Events split at arbitrary byte boundaries
//! - Multiple events in a single TCP chunk
//! - Large events
//! - Connection termination
//! - CRLF line endings
//! - Empty lines and comments

use protocol_core::sse::{StreamingSseParser, format_done_event, format_sse_event};

#[test]
fn test_byte_level_splitting() {
    let mut parser = StreamingSseParser::new();
    let full_event = "data: hello world\n\n";

    // Feed one byte at a time.
    let mut all_events = Vec::new();
    for byte in full_event.bytes() {
        all_events.extend(parser.feed(&[byte]));
    }
    assert_eq!(all_events.len(), 1);
    assert_eq!(all_events[0].data, "hello world");
}

#[test]
fn test_split_in_middle_of_data() {
    let mut parser = StreamingSseParser::new();

    // Split right in the middle of a JSON payload.
    let chunk1 = b"data: {\"key\": \"val";
    let chunk2 = b"ue\"}\n\n";

    let events1 = parser.feed(chunk1);
    assert!(events1.is_empty());

    let events2 = parser.feed(chunk2);
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].data, r#"{"key": "value"}"#);
}

#[test]
fn test_split_in_middle_of_event_type() {
    let mut parser = StreamingSseParser::new();

    let chunk1 = b"event: mess";
    let chunk2 = b"age\ndata: test\n\n";

    let events1 = parser.feed(chunk1);
    assert!(events1.is_empty());

    let events2 = parser.feed(chunk2);
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].event_type.as_deref(), Some("message"));
    assert_eq!(events2[0].data, "test");
}

#[test]
fn test_large_event() {
    let mut parser = StreamingSseParser::new();

    // Create a 100KB data payload.
    let payload = "x".repeat(100_000);
    let wire = format!("data: {payload}\n\n");

    let events = parser.feed(wire.as_bytes());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data.len(), 100_000);
}

#[test]
fn test_many_events_in_single_chunk() {
    let mut parser = StreamingSseParser::new();

    let mut wire = String::new();
    for i in 0..100 {
        wire.push_str(&format!("data: event-{i}\n\n"));
    }

    let events = parser.feed(wire.as_bytes());
    assert_eq!(events.len(), 100);
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event.data, format!("event-{i}"));
    }
}

#[test]
fn test_connection_termination_flushes_incomplete_event() {
    let mut parser = StreamingSseParser::new();

    // Send data without a trailing blank line.
    parser.feed(b"data: incomplete\n");
    let events = parser.finish();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "incomplete");
}

#[test]
fn test_connection_termination_with_partial_line() {
    let mut parser = StreamingSseParser::new();

    // Send a partial line (no newline yet).
    parser.feed(b"data: part");
    let events = parser.finish();
    // The partial line doesn't end with \n, so it's not a complete line.
    // But finish() should try to process it.
    // The current implementation only processes lines ending with \n,
    // so the partial "data: part" stays in the buffer. finish() checks
    // if the buffer starts with "data:" and extracts it.
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "part");
}

#[test]
fn test_crlf_consistently() {
    let mut parser = StreamingSseParser::new();

    let wire = "data: hello\r\n\r\n";
    let events = parser.feed(wire.as_bytes());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn test_mixed_lf_and_crlf() {
    let mut parser = StreamingSseParser::new();

    // First event with LF.
    let events1 = parser.feed("data: first\n\n".as_bytes());
    assert_eq!(events1.len(), 1);

    // Second event with CRLF.
    let events2 = parser.feed("data: second\r\n\r\n".as_bytes());
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].data, "second");
}

#[test]
fn test_comment_lines_ignored() {
    let mut parser = StreamingSseParser::new();

    let wire = ": this is a comment\ndata: actual\n\n";
    let events = parser.feed(wire.as_bytes());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "actual");
}

#[test]
fn test_unknown_field_ignored() {
    let mut parser = StreamingSseParser::new();

    let wire = "unknown_field: value\ndata: test\n\n";
    let events = parser.feed(wire.as_bytes());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "test");
}

#[test]
fn test_format_then_parse_roundtrip() {
    let original_data = "Hello, world!";
    let wire = format_sse_event(original_data, Some("message"));

    let mut parser = StreamingSseParser::new();
    let events = parser.feed(&wire);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_deref(), Some("message"));
    assert_eq!(events[0].data, original_data);
}

#[test]
fn test_format_done_then_parse() {
    let wire = format_done_event();

    let mut parser = StreamingSseParser::new();
    let events = parser.feed(&wire);

    assert_eq!(events.len(), 1);
    assert!(events[0].is_done());
}

#[test]
fn test_event_with_only_event_type_no_data() {
    let mut parser = StreamingSseParser::new();

    // An event with only an event type and no data should not emit.
    let events = parser.feed("event: ping\n\n".as_bytes());
    assert!(events.is_empty());
}

#[test]
fn test_event_with_all_fields() {
    let mut parser = StreamingSseParser::new();

    let wire = "id: 42\nevent: message\ndata: {\"text\":\"hi\"}\nretry: 3000\n\n";
    let events = parser.feed(wire.as_bytes());

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id.as_deref(), Some("42"));
    assert_eq!(events[0].event_type.as_deref(), Some("message"));
    assert_eq!(events[0].data, r#"{"text":"hi"}"#);
    assert_eq!(events[0].retry, Some(3000));
}

#[test]
fn test_realistic_openai_stream_simulation() {
    let mut parser = StreamingSseParser::new();

    // Simulate a realistic OpenAI streaming response with tool calls.
    let chunks = vec![
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"content\":[]}}\n\n",
        "event: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"text\":\"Hello world\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\"}}\n\n",
    ];

    let mut all_events = Vec::new();
    for chunk in &chunks {
        all_events.extend(parser.feed(chunk.as_bytes()));
    }

    assert_eq!(all_events.len(), 7);

    // Verify event types.
    assert_eq!(
        all_events[0].event_type.as_deref(),
        Some("response.created")
    );
    assert_eq!(
        all_events[3].event_type.as_deref(),
        Some("response.output_text.delta")
    );
    assert_eq!(
        all_events[3].data,
        r#"{"type":"response.output_text.delta","delta":"Hello"}"#
    );
}

#[test]
fn test_realistic_anthropic_stream_simulation() {
    let mut parser = StreamingSseParser::new();

    let chunks = vec![
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\"}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" there!\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ];

    let mut all_events = Vec::new();
    for chunk in &chunks {
        all_events.extend(parser.feed(chunk.as_bytes()));
    }

    assert_eq!(all_events.len(), 7);
    assert_eq!(all_events[0].event_type.as_deref(), Some("message_start"));
    assert_eq!(all_events[6].event_type.as_deref(), Some("message_stop"));
}

#[test]
fn test_interleaved_split_events() {
    let mut parser = StreamingSseParser::new();

    // Send two events where the split happens in the middle of event 2.
    let combined = "data: first\n\ndata: sec";
    let events1 = parser.feed(combined.as_bytes());
    assert_eq!(events1.len(), 1);
    assert_eq!(events1[0].data, "first");

    let events2 = parser.feed("ond\n\n".as_bytes());
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].data, "second");
}

// ─── Missing SSE edge cases ─────────────────────────────────────────────────

#[test]
fn test_empty_data_value() {
    let mut parser = StreamingSseParser::new();
    // data: with an empty value — should be emitted as an empty string.
    let events = parser.feed(b"data: \n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
}

#[test]
fn test_malformed_json_data() {
    let mut parser = StreamingSseParser::new();
    let events = parser.feed(b"data: {unclosed json\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "{unclosed json");
}

#[test]
fn test_consecutive_blank_lines() {
    let mut parser = StreamingSseParser::new();
    // Multiple blank lines between events — only the first starts a new event.
    let events = parser.feed(b"data: first\n\n\ndata: second\n\n");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].data, "first");
    assert_eq!(events[1].data, "second");
}

#[test]
fn test_bare_carriage_return() {
    let mut parser = StreamingSseParser::new();
    // \r\r\n — the trim_end_matches('\r') strips the trailing \r before \n.
    let events = parser.feed(b"data: hello\r\r\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn test_crlf_split_across_chunks() {
    let mut parser = StreamingSseParser::new();
    // The \r\n is split across two chunks — \r in chunk 1, \n in chunk 2.
    let events1 = parser.feed(b"data: hello\r");
    assert!(events1.is_empty());
    let events2 = parser.feed(b"\n\n");
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].data, "hello");
}

#[test]
fn test_non_utf8_replacement() {
    let mut parser = StreamingSseParser::new();
    // Invalid UTF-8 bytes — should be replaced with the Unicode replacement character.
    let mut input = b"data: hello".to_vec();
    input.push(0xFF);
    input.extend_from_slice(b" world\n\n");
    let events = parser.feed(&input);
    assert_eq!(events.len(), 1);
    assert!(events[0].data.contains("hello"));
    assert!(events[0].data.contains("world"));
    assert!(events[0].data.contains('\u{FFFD}'));
}

#[test]
fn test_finish_with_non_data_lines_in_buffer() {
    let mut parser = StreamingSseParser::new();
    // Feed a data line, then an event type line, then call finish.
    parser.feed(b"data: hello\n");
    parser.feed(b"event: ping\n");
    let events = parser.finish();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
    assert_eq!(events[0].event_type.as_deref(), Some("ping"));
}

#[test]
fn test_retry_non_numeric_ignored() {
    let mut parser = StreamingSseParser::new();
    let events = parser.feed(b"retry: abc\ndata: hello\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].retry, None);
}
