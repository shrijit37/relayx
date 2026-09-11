//! Node implementations for the workflow runtime.

pub mod condition;
pub mod llm;
pub mod mcp;
pub mod router;
pub mod skill;
pub mod transform;

/// Input to a node — a JSON value.
#[derive(Debug, Clone)]
pub enum NodeInput {
    /// A single JSON value.
    Message(serde_json::Value),
    /// A stream of JSON values (for streaming nodes).
    Stream(Vec<serde_json::Value>),
}

impl NodeInput {
    /// Convert to a JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            NodeInput::Message(v) => v.clone(),
            NodeInput::Stream(items) => serde_json::Value::Array(items.clone()),
        }
    }

    /// Extract as a string if it's a simple text value.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            NodeInput::Message(serde_json::Value::String(s)) => Some(s),
            _ => None,
        }
    }
}

/// Output from a node — a JSON value.
#[derive(Debug, Clone)]
pub enum NodeOutput {
    /// A single JSON value.
    Message(serde_json::Value),
    /// A streaming response.
    Stream(Vec<serde_json::Value>),
}

impl NodeOutput {
    /// Convert to a JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            NodeOutput::Message(v) => v.clone(),
            NodeOutput::Stream(items) => serde_json::Value::Array(items.clone()),
        }
    }
}

/// Re-export node kinds from the schema crate.
pub use workflow_schema::NodeKind;
