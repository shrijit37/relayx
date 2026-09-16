//! Node implementations for the workflow runtime.

pub mod condition;
pub mod fallback;
pub mod llm;
pub mod mcp;
pub mod retry;
pub mod router;
pub mod skill;
pub mod transform;

use serde::{Deserialize, Serialize};

// ─── Runtime values ─────────────────────────────────────────────────────────

/// A typed runtime value flowing between nodes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuntimeValue {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Json(serde_json::Value),
    Message {
        role: String,
        content: String,
    },
}

impl RuntimeValue {
    /// Create from a JSON value, mapping JSON types to RuntimeValue.
    pub fn from_json(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => RuntimeValue::Null,
            serde_json::Value::Bool(b) => RuntimeValue::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    RuntimeValue::Integer(i)
                } else {
                    RuntimeValue::Number(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => RuntimeValue::String(s),
            other => RuntimeValue::Json(other),
        }
    }

    /// Convert to JSON.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            RuntimeValue::Null => serde_json::Value::Null,
            RuntimeValue::Bool(b) => serde_json::json!(b),
            RuntimeValue::Integer(n) => serde_json::json!(n),
            RuntimeValue::Number(n) => serde_json::json!(n),
            RuntimeValue::String(s) => serde_json::json!(s),
            RuntimeValue::Json(v) => v.clone(),
            RuntimeValue::Message { role, content } => {
                serde_json::json!({"role": role, "content": content})
            }
        }
    }

    /// Extract as a string if it's a string or text JSON.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            RuntimeValue::String(s) => Some(s),
            RuntimeValue::Json(serde_json::Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Extract as a bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            RuntimeValue::Bool(b) => Some(*b),
            RuntimeValue::Integer(n) => Some(*n != 0),
            RuntimeValue::Number(n) => Some(*n != 0.0),
            RuntimeValue::String(s) => match s.as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" | "" => Some(false),
                _ => None,
            },
            RuntimeValue::Null => Some(false),
            RuntimeValue::Json(v) => v.as_bool(),
            RuntimeValue::Message { .. } => None,
        }
    }

    /// Read a field from a JSON value (for Extract operations and conditions).
    pub fn get_field(&self, field: &str) -> Option<RuntimeValue> {
        let v = self.to_json();
        let parts: Vec<&str> = field.split('.').collect();
        let mut current = &v;
        for part in &parts {
            current = current.get(part)?;
        }
        Some(RuntimeValue::from_json(current.clone()))
    }
}

impl From<serde_json::Value> for RuntimeValue {
    fn from(v: serde_json::Value) -> Self {
        Self::from_json(v)
    }
}

impl From<RuntimeValue> for serde_json::Value {
    fn from(v: RuntimeValue) -> Self {
        v.to_json()
    }
}

// ─── Node input/output ──────────────────────────────────────────────────────

/// Input to a node — a runtime value with an optional port name.
#[derive(Debug, Clone)]
pub struct NodeInput {
    /// The port this input arrived on (if specified by an edge).
    pub port: Option<String>,
    /// The runtime value.
    pub value: RuntimeValue,
}

impl NodeInput {
    /// Create a simple message input.
    pub fn message(value: RuntimeValue) -> Self {
        Self { port: None, value }
    }

    /// Create a message input on a specific port.
    pub fn on_port(port: impl Into<String>, value: RuntimeValue) -> Self {
        Self {
            port: Some(port.into()),
            value,
        }
    }
}

/// Output from a node — a runtime value with an optional port name.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    /// The port this output is on.
    pub port: Option<String>,
    /// The runtime value.
    pub value: RuntimeValue,
}

impl NodeOutput {
    /// Create a simple message output.
    pub fn message(value: RuntimeValue) -> Self {
        Self { port: None, value }
    }

    /// Create a message output on a specific port.
    pub fn on_port(port: impl Into<String>, value: RuntimeValue) -> Self {
        Self {
            port: Some(port.into()),
            value,
        }
    }
}

/// Re-export node kinds from the schema crate.
pub use workflow_schema::NodeKind;
