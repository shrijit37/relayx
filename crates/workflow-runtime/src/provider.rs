//! Stable provider extension boundary.
//!
//! A provider is the unit of upstream LLM execution: an identity, an
//! endpoint, a protocol, and a set of capabilities. Providers are registered
//! on the runtime snapshot and referenced by id from the LLM node. Adding a
//! provider never requires modifying core executor logic.

use protocol_core::canonical::Protocol;
use url::Url;

use crate::capability::Capabilities;

/// A registered provider entry.
///
/// Immutable after registration. Bundled into the runtime snapshot so the
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
