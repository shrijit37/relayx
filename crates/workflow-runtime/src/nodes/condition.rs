//! Condition node — evaluates a condition and routes accordingly.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::ConditionConfig;

/// Execute a condition node. Evaluates the configured condition against the input.
pub async fn execute(
    config: &ConditionConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    let data = input.to_json();
    let field_value = data
        .get(&config.field)
        .and_then(|v| match config.operator {
            workflow_schema::ConditionOp::IsEmpty => Some(v.is_null() || v == ""),
            workflow_schema::ConditionOp::IsNotEmpty => Some(!v.is_null() && v != ""),
            workflow_schema::ConditionOp::Equal => Some(v == &config.value),
            workflow_schema::ConditionOp::NotEqual => Some(v != &config.value),
            workflow_schema::ConditionOp::Contains => v
                .as_str()
                .map(|s| s.contains(config.value.as_str().unwrap_or(""))),
            workflow_schema::ConditionOp::NotContains => v
                .as_str()
                .map(|s| !s.contains(config.value.as_str().unwrap_or(""))),
            _ => Some(false),
        })
        .unwrap_or(false);

    Ok(NodeOutput::Message(serde_json::json!({
        "condition": config.condition,
        "result": field_value,
    })))
}
