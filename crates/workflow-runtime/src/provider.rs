//! Stable provider extension boundary.
//!
//! A provider is the unit of upstream LLM execution: an identity, an
//! endpoint, a protocol, and a set of capabilities. Providers are registered
//! in a `ProviderRegistry` and referenced by id from the compiler and LLM
//! node. Adding a provider never requires modifying core executor logic.

use std::sync::Arc;

use protocol_core::canonical::Protocol;
use url::Url;

use crate::capability::Capabilities;

/// A registered provider entry.
///
/// Immutable after registration. The compiler uses this to validate
/// model/protocol/capability compatibility at plan-compile time so the
/// hot path never performs discovery.
#[derive(Debug, Clone)]
pub struct ProviderEntry {
    /// Stable provider identifier (e.g. `"anthropic-prod"`).
    pub id: String,
    /// Wire protocol the provider speaks.
    pub protocol: Protocol,
    /// Upstream base URL.
    pub base_url: Url,
    /// Default model served by this provider.
    pub model: String,
    /// What the provider supports.
    pub capabilities: Capabilities,
    /// Lane carrying network egress to this provider.
    pub lane_id: String,
}

/// Thread-safe registry of registered providers.
///
/// Read-only after construction; the executor reads without locking.
#[derive(Debug, Default)]
pub struct ProviderRegistry {
    providers: std::collections::HashMap<String, Arc<ProviderEntry>>,
}

impl ProviderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider entry.
    pub fn register(&mut self, entry: ProviderEntry) {
        let id = entry.id.clone();
        self.providers.insert(id, Arc::new(entry));
    }

    /// Look up a provider by id.
    pub fn get(&self, id: &str) -> Option<Arc<ProviderEntry>> {
        self.providers.get(id).cloned()
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Iterate over all registered providers.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<ProviderEntry>> {
        self.providers.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_provider(id: &str) -> ProviderEntry {
        let base_url = match Url::parse("http://127.0.0.1:9000") {
            Ok(u) => u,
            Err(e) => panic!("failed to parse test url: {e}"),
        };
        ProviderEntry {
            id: id.into(),
            protocol: Protocol::OpenAiChatCompletions,
            base_url,
            model: "test-model".into(),
            capabilities: Capabilities::default(),
            lane_id: "default".into(),
        }
    }

    #[test]
    fn registry_roundtrip() {
        let mut registry = ProviderRegistry::new();
        registry.register(sample_provider("anthropic"));

        let entry = match registry.get("anthropic") {
            Some(e) => e,
            None => panic!("provider should be found"),
        };
        assert_eq!(entry.id, "anthropic");
        assert_eq!(entry.model, "test-model");
        assert!(registry.get("missing").is_none());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_is_empty_initially() {
        let registry = ProviderRegistry::new();
        assert!(registry.is_empty());
    }
}
