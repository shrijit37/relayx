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

impl Capabilities {
    /// Return the set of capability fields that are in `self` but not `other`.
    pub fn excess(&self, other: &Capabilities) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.streaming && !other.streaming {
            missing.push("streaming");
        }
        if self.tools && !other.tools {
            missing.push("tools");
        }
        if self.structured_output && !other.structured_output {
            missing.push("structured_output");
        }
        if self.reasoning && !other.reasoning {
            missing.push("reasoning");
        }
        if self.vision && !other.vision {
            missing.push("vision");
        }
        if self.audio && !other.audio {
            missing.push("audio");
        }
        if self.citations && !other.citations {
            missing.push("citations");
        }
        if self.deferred_tools && !other.deferred_tools {
            missing.push("deferred_tools");
        }
        if self.cache_hints && !other.cache_hints {
            missing.push("cache_hints");
        }
        missing
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
