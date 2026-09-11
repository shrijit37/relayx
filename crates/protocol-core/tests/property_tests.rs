//! Property-based tests for the SSE parser and canonical model.
//!
//! Uses proptest to verify:
//! - SSE parser never panics on arbitrary input
//! - format → parse roundtrip preserves event data
//! - Canonical model serde roundtrip preserves structure

use proptest::prelude::*;

use protocol_core::canonical::*;
use protocol_core::sse::{StreamingSseParser, format_sse_event};

// ─── SSE parser: arbitrary byte safety ────────────────────────────────────────

proptest! {
    #[test]
    fn feed_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let mut parser = StreamingSseParser::new();
        let _events = parser.feed(&data);
        let _remaining = parser.finish();
    }
}

proptest! {
    #[test]
    fn feed_small_chunks_never_panics(data in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut parser = StreamingSseParser::new();
        // Feed one byte at a time — maximum fragmentation.
        for byte in &data {
            let _events = parser.feed(&[*byte]);
        }
        let _remaining = parser.finish();
    }
}

// ─── SSE format → parse roundtrip ────────────────────────────────────────────

proptest! {
    #[test]
    fn format_then_parse_preserves_data(
        data in "[a-zA-Z0-9 \\n]{0,512}",
        event_type in proptest::option::of("[a-z_]{1,32}")
    ) {
        let wire = format_sse_event(&data, event_type.as_deref());
        let mut parser = StreamingSseParser::new();
        let mut events = parser.feed(&wire);
        events.extend(parser.finish());

        // We should get exactly one event with the same data.
        prop_assert!(!events.is_empty(), "no event parsed from formatted data");
        let parsed = &events[0];
        prop_assert_eq!(&parsed.data, &data);
        prop_assert_eq!(&parsed.event_type, &event_type);
    }
}

// ─── Canonical model serde roundtrip ─────────────────────────────────────────

proptest! {
    #[test]
    fn stream_event_serde_roundtrip(
        text in "[a-zA-Z0-9 ]{0,256}",
        index in 0usize..16,
    ) {
        let event = CanonicalStreamEvent::TextDelta { index, text };
        let json = serde_json::to_string(&event).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
        let decoded: CanonicalStreamEvent = serde_json::from_str(&json).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
        // Roundtrip: re-serialize and compare JSON strings (CanonicalStreamEvent
        // does not derive PartialEq due to embedded serde_json::Value).
        let re_encoded = serde_json::to_string(&decoded).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&json, &re_encoded);
    }
}

proptest! {
    #[test]
    fn usage_serde_roundtrip(
        input in prop::option::of(0u32..100_000),
        output in prop::option::of(0u32..100_000),
        total in prop::option::of(0u32..200_000),
    ) {
        let usage = Usage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: total,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let json = serde_json::to_string(&usage).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
        let decoded: Usage = serde_json::from_str(&json).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded.input_tokens, &input);
        prop_assert_eq!(&decoded.output_tokens, &output);
        prop_assert_eq!(&decoded.total_tokens, &total);
    }
}
