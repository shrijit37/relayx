//! Transform node — modifies runtime values via extraction, merging, or filtering.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
use workflow_schema::TransformConfig;

/// Execute a transform node.
pub async fn execute(
    config: &TransformConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    let result = match config.operation {
        workflow_schema::TransformOperation::Passthrough => input.value,
        workflow_schema::TransformOperation::Extract => {
            // When input is a JSON object, return the first non-null field.
            // When input has a "field" key, extract it.
            match &input.value {
                RuntimeValue::Json(obj) => {
                    if let Some(field) = obj.get("field").and_then(|f| f.as_str()) {
                        obj.get(field)
                            .cloned()
                            .map(RuntimeValue::from_json)
                            .unwrap_or(RuntimeValue::Null)
                    } else {
                        input.value
                    }
                }
                _ => input.value,
            }
        }
        workflow_schema::TransformOperation::Merge => {
            // Merge multiple upstream inputs into one JSON object.
            match &input.value {
                RuntimeValue::Json(serde_json::Value::Array(items)) => {
                    let mut merged = serde_json::Map::new();
                    for item in items {
                        if let serde_json::Value::Object(map) = item {
                            for (k, v) in map {
                                merged.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    RuntimeValue::Json(serde_json::Value::Object(merged))
                }
                _ => input.value,
            }
        }
        workflow_schema::TransformOperation::Filter => {
            // When input is a JSON array, keep items matching a condition.
            match &input.value {
                RuntimeValue::Json(serde_json::Value::Array(items)) => {
                    let filtered: Vec<serde_json::Value> = items
                        .iter()
                        .filter(|item| {
                            // Keep non-null, non-empty items.
                            !matches!(item, serde_json::Value::Null)
                                && !matches!(
                                    item,
                                    serde_json::Value::String(s) if s.is_empty()
                                )
                        })
                        .cloned()
                        .collect();
                    RuntimeValue::Json(serde_json::Value::Array(filtered))
                }
                _ => input.value,
            }
        }
    };

    Ok(NodeOutput::message(result))
}
