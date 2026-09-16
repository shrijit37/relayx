//! Stable capability representation.
//!
//! Capabilities describe what a provider or node supports. The compiler
//! checks compatibility between node requirements and provider capabilities
//! before execution begins.

use serde::{Deserialize, Serialize};

/// What a provider or node supports.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    /// Supports streaming responses.
    pub streaming: bool,
    /// Supports tool/function calling.
    pub tools: bool,
    /// Supports structured output (JSON schema).
    pub structured_output: bool,
    /// Supports reasoning / thinking blocks.
    pub reasoning: bool,
    /// Supports image inputs.
    pub vision: bool,
    /// Supports audio inputs.
    pub audio: bool,
    /// Supports citations in responses.
    pub citations: bool,
    /// Supports deferred (lazy) tool references.
    pub deferred_tools: bool,
    /// Supports cache hints.
    pub cache_hints: bool,
}

/// Number of capability fields — must stay in sync with `FIELD_NAMES` and
/// [`Capabilities::bools`]. A mismatch is a compile error via the array
/// size, but this constant documents the contract.
const CAPABILITY_COUNT: usize = 9;

/// Field name for each capability, in declaration order. Used to report
/// missing capabilities without hand-rolling near-identical branches.
const FIELD_NAMES: [&str; CAPABILITY_COUNT] = [
    "streaming",
    "tools",
    "structured_output",
    "reasoning",
    "vision",
    "audio",
    "citations",
    "deferred_tools",
    "cache_hints",
];

impl Capabilities {
    /// Return the set of capability fields that are in `self` but not `other`.
    pub fn excess(&self, other: &Capabilities) -> Vec<&'static str> {
        let self_bools = self.bools();
        let other_bools = other.bools();
        FIELD_NAMES
            .iter()
            .zip(self_bools.iter().zip(other_bools.iter()))
            .filter(|(_, (s, o))| **s && !**o)
            .map(|(name, _)| *name)
            .collect()
    }

    /// The capability flags as a fixed-order array (indexes align with
    /// [`FIELD_NAMES`]).
    fn bools(&self) -> [bool; CAPABILITY_COUNT] {
        [
            self.streaming,
            self.tools,
            self.structured_output,
            self.reasoning,
            self.vision,
            self.audio,
            self.citations,
            self.deferred_tools,
            self.cache_hints,
        ]
    }

    /// Check if `other` satisfies all capability requirements in `self`.
    pub fn is_satisfied_by(&self, other: &Capabilities) -> bool {
        self.excess(other).is_empty()
    }

    /// Convert from protocol-core `ProtocolCapabilities`.
    pub fn from_protocol(caps: &protocol_core::canonical::ProtocolCapabilities) -> Self {
        Self {
            streaming: caps.streaming,
            tools: caps.tools,
            structured_output: caps.structured_output,
            reasoning: caps.reasoning,
            vision: caps.multimodal_input,
            audio: false,
            citations: false,
            deferred_tools: caps.deferred_tools,
            cache_hints: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_are_all_false() {
        let caps = Capabilities::default();
        assert!(!caps.streaming);
        assert!(!caps.tools);
        assert!(!caps.structured_output);
        assert!(!caps.reasoning);
    }

    #[test]
    fn excess_detects_missing_capabilities() {
        let required = Capabilities {
            streaming: true,
            tools: true,
            ..Default::default()
        };
        let available = Capabilities {
            streaming: true,
            tools: false,
            ..Default::default()
        };
        let missing = required.excess(&available);
        assert_eq!(missing, vec!["tools"]);
    }

    #[test]
    fn satisfied_when_all_required_present() {
        let required = Capabilities {
            streaming: true,
            reasoning: true,
            ..Default::default()
        };
        let available = Capabilities {
            streaming: true,
            reasoning: true,
            vision: true,
            ..Default::default()
        };
        assert!(required.is_satisfied_by(&available));
    }

    #[test]
    fn not_satisfied_when_missing_required() {
        let required = Capabilities {
            streaming: true,
            reasoning: true,
            ..Default::default()
        };
        let available = Capabilities {
            streaming: true,
            ..Default::default()
        };
        assert!(!required.is_satisfied_by(&available));
    }
}
