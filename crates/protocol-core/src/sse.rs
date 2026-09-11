//! SSE (Server-Sent Events) parser.
//!
//! Operates at the **protocol framing level**, not the transport chunk level.
//! A single SSE event may be split across multiple TCP chunks, and a single
//! TCP chunk may contain multiple events. This parser correctly handles both.
//!
//! Wire format per the SSE specification:
//!
//! ```text
//! event: <event type>\n
//! data: <payload>\n
//! id: <event id>\n
//! retry: <milliseconds>\n
//! \n
//! ```
//!
//! Multi-line data fields are supported (each `data:` line is joined with `\n`).
//! The `[DONE]` sentinel terminates OpenAI streams.

/// A parsed SSE event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Event type (from `event:` line).
    pub event_type: Option<String>,
    /// Event data (concatenated `data:` lines).
    pub data: String,
    /// Event ID (from `id:` line).
    pub id: Option<String>,
    /// Retry interval (from `retry:` line).
    pub retry: Option<u64>,
}

impl SseEvent {
    /// Check if this is the OpenAI `[DONE]` sentinel.
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Streaming SSE event parser that correctly accumulates partial events.
///
/// This is the primary parser interface for the protocol engine. It handles
/// all the edge cases of SSE wire format parsing across transport boundaries:
///
/// - Events split across TCP chunks
/// - Multiple events in a single TCP chunk
/// - Multi-line `data:` fields
/// - Comment lines (`:` prefix)
/// - `event:`, `id:`, `retry:` fields
/// - Blank-line event termination
/// - Connection close flushing incomplete events
#[derive(Debug, Default)]
pub struct StreamingSseParser {
    /// Line buffer for incomplete lines.
    line_buffer: String,
    /// Current event fields being accumulated.
    current_event_type: Option<String>,
    current_data_lines: Vec<String>,
    current_id: Option<String>,
    current_retry: Option<u64>,
}

impl StreamingSseParser {
    /// Create a new streaming parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes from the transport. Returns all complete events found.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.line_buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();

        while let Some(line_end) = self.line_buffer.find('\n') {
            // Clone the line to avoid borrowing self.buffer while draining it.
            let line = self.line_buffer[..line_end]
                .trim_end_matches('\r')
                .to_owned();
            self.line_buffer.drain(..=line_end);

            if line.is_empty() {
                // Blank line: emit the accumulated event if there is data.
                if !self.current_data_lines.is_empty() {
                    events.push(SseEvent {
                        event_type: self.current_event_type.take(),
                        data: self.current_data_lines.join("\n"),
                        id: self.current_id.take(),
                        retry: self.current_retry.take(),
                    });
                }
                // Reset for next event (blank line always starts a new event).
                self.current_data_lines.clear();
                self.current_event_type = None;
                self.current_id = None;
                self.current_retry = None;
                continue;
            }

            self.parse_line(&line);
        }

        events
    }

    /// Signal that the connection has ended. Flushes any incomplete event
    /// that has accumulated data (some servers send the last event without
    /// a trailing blank line).
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();

        // Parse any remaining line in the buffer.
        if !self.line_buffer.is_empty() {
            let remaining = self.line_buffer.trim_end_matches('\r').to_owned();
            self.line_buffer.clear();
            self.parse_line(&remaining);
        }

        // Emit any accumulated event.
        if !self.current_data_lines.is_empty() {
            events.push(SseEvent {
                event_type: self.current_event_type.take(),
                data: self.current_data_lines.join("\n"),
                id: self.current_id.take(),
                retry: self.current_retry.take(),
            });
            self.current_data_lines.clear();
        }

        events
    }

    /// Parse a single SSE field line and update the current event state.
    fn parse_line(&mut self, line: &str) {
        if line.is_empty() || line.starts_with(':') {
            return;
        }

        if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            self.current_data_lines.push(value.to_owned());
        } else if let Some(value) = line.strip_prefix("event:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            self.current_event_type = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("id:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            self.current_id = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("retry:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if let Ok(ms) = value.trim().parse::<u64>() {
                self.current_retry = Some(ms);
            }
        }
    }
}

/// Apply SSE wire format to data: produce the raw bytes for a single event.
pub fn format_sse_event(data: &str, event_type: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();

    if let Some(et) = event_type {
        out.extend_from_slice(b"event: ");
        out.extend_from_slice(et.as_bytes());
        out.push(b'\n');
    }

    // Each line of data gets its own `data: ` prefix.
    for line in data.split('\n') {
        out.extend_from_slice(b"data: ");
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }

    // Blank line terminates the event.
    out.push(b'\n');
    out
}

/// Format the `[DONE]` sentinel for OpenAI-style streams.
pub fn format_done_event() -> Vec<u8> {
    format_sse_event("[DONE]", None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_event() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"event: message\ndata: {\"hello\":\"world\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_deref(), Some("message"));
        assert_eq!(events[0].data, r#"{"hello":"world"}"#);
    }

    #[test]
    fn test_parse_event_split_across_chunks() {
        let mut parser = StreamingSseParser::new();

        // Chunk 1: partial data line.
        let events1 = parser.feed(b"event: message\ndata: {\"del");
        assert!(events1.is_empty(), "no complete event yet");

        // Chunk 2: rest of data + blank line terminator.
        let events2 = parser.feed(b"ta\":\"hello\"}\n\n");
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].event_type.as_deref(), Some("message"));
        assert_eq!(events2[0].data, r#"{"delta":"hello"}"#);
    }

    #[test]
    fn test_parse_multiple_events_in_one_chunk() {
        let mut parser = StreamingSseParser::new();
        let chunk = "data: first\n\ndata: second\n\ndata: third\n\n";
        let events = parser.feed(chunk.as_bytes());
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
        assert_eq!(events[2].data, "third");
    }

    #[test]
    fn test_parse_multi_line_data() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"data: line1\ndata: line2\ndata: line3\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2\nline3");
    }

    #[test]
    fn test_parse_done_sentinel() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"data: [DONE]\n\n");
        assert_eq!(events.len(), 1);
        assert!(events[0].is_done());
    }

    #[test]
    fn test_parse_event_with_id() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"id: 123\ndata: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("123"));
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_parse_event_with_retry() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"retry: 5000\ndata: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].retry, Some(5000));
    }

    #[test]
    fn test_parse_comment_lines_ignored() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b": this is a comment\ndata: actual data\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "actual data");
    }

    #[test]
    fn test_parse_no_trailing_blank_line() {
        let mut parser = StreamingSseParser::new();
        // Some servers don't send a trailing blank line on the last event.
        let events = parser.feed(b"data: last\n");
        assert!(
            events.is_empty(),
            "no event yet without blank line terminator"
        );

        // finish() should flush it.
        let final_events = parser.finish();
        assert_eq!(final_events.len(), 1);
        assert_eq!(final_events[0].data, "last");
    }

    #[test]
    fn test_format_sse_event_basic() {
        let wire = format_sse_event("hello world", None);
        assert_eq!(wire, b"data: hello world\n\n");
    }

    #[test]
    fn test_format_sse_event_with_type() {
        let wire = format_sse_event(r#"{"delta":"hi"}"#, Some("message.delta"));
        let expected = b"event: message.delta\ndata: {\"delta\":\"hi\"}\n\n";
        assert_eq!(wire, expected);
    }

    #[test]
    fn test_format_sse_event_multiline() {
        let wire = format_sse_event("line1\nline2", None);
        assert_eq!(wire, b"data: line1\ndata: line2\n\n");
    }

    #[test]
    fn test_format_done_event() {
        let wire = format_done_event();
        assert_eq!(wire, b"data: [DONE]\n\n");
    }

    #[test]
    fn test_crlf_handling() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(b"data: hello\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_partial_line_across_many_chunks() {
        let mut parser = StreamingSseParser::new();
        // Split a single "data: hello\n\n" across many tiny chunks.
        let chunks: Vec<&[u8]> = vec![
            b"d", b"a", b"t", b"a", b": ", b"h", b"e", b"l", b"l", b"o", b"\n", b"\n",
        ];
        let mut all_events = Vec::new();
        for chunk in chunks {
            all_events.extend(parser.feed(chunk));
        }
        assert_eq!(all_events.len(), 1);
        assert_eq!(all_events[0].data, "hello");
    }

    #[test]
    fn test_event_type_only_no_data() {
        let mut parser = StreamingSseParser::new();
        // An event with event type but no data lines should not emit.
        let events = parser.feed(b"event: ping\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn test_interleaved_event_types() {
        let mut parser = StreamingSseParser::new();
        let events = parser.feed(
            b"event: message_start\ndata: {\"type\":\"start\"}\n\nevent: message_delta\ndata: {\"type\":\"delta\"}\n\n"
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type.as_deref(), Some("message_start"));
        assert_eq!(events[1].event_type.as_deref(), Some("message_delta"));
    }
}
