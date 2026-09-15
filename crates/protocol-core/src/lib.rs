//! protocol-core: canonical protocol model and adapters for the relay-x gateway.
//!
//! Provides a strongly typed internal representation for LLM API protocols.
//! The canonical model preserves source semantics and represents provider-specific
//! features via explicit extension fields.
//!
//! # Architecture
//!
//! ```text
//! Client Protocol → Adapter (decode) → Canonical → Adapter (encode) → Upstream Protocol
//! ```
//!
//! The canonical model is the semantic translation boundary. Everything above it
//! (routing, workflows, MCP) operates on canonical types; everything below it
//! (adapters) handles wire-format translation.

pub mod adapters;
pub mod canonical;
pub mod catalog;
pub mod error;
pub mod sse;
