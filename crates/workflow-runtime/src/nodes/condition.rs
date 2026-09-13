//! Condition node — evaluates a condition and outputs on the "true" or "false" port.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
use workflow_schema::ConditionConfig;

/// Execute a condition node. Evaluates the configured condition against the input
/// and returns the result on the "true" or "false" output port.
pub async fn execute(
    config: &ConditionConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    let result = evaluate_condition(config, &input.value);

    let port = if result { "true" } else { "false" };

    Ok(NodeOutput::on_port(port, RuntimeValue::Bool(result)))
}

/// Evaluate a condition config against a runtime value.
fn evaluate_condition(config: &ConditionConfig, value: &RuntimeValue) -> bool {
    let field_value = value.get_field(&config.field);

    match (&field_value, &config.operator) {
        (None, workflow_schema::ConditionOp::IsEmpty) => true,
        (None, _) => false,
        (Some(rv), op) => match op {
            workflow_schema::ConditionOp::Equal => match (rv, &config.value) {
                (RuntimeValue::Integer(a), serde_json::Value::Number(n)) => n.as_i64() == Some(*a),
                (RuntimeValue::Number(a), serde_json::Value::Number(n)) => n.as_f64() == Some(*a),
                _ => rv.to_json() == config.value,
            },
            workflow_schema::ConditionOp::NotEqual => match (rv, &config.value) {
                (RuntimeValue::Integer(a), serde_json::Value::Number(n)) => n.as_i64() != Some(*a),
                (RuntimeValue::Number(a), serde_json::Value::Number(n)) => n.as_f64() != Some(*a),
                _ => rv.to_json() != config.value,
            },
            workflow_schema::ConditionOp::GreaterThan => {
                let a = match rv {
                    RuntimeValue::Integer(i) => *i as f64,
                    _ => rv.to_json().as_f64().unwrap_or(0.0),
                };
                let b = config.value.as_f64().unwrap_or(0.0);
                a > b
            }
            workflow_schema::ConditionOp::LessThan => {
                let a = match rv {
                    RuntimeValue::Integer(i) => *i as f64,
                    _ => rv.to_json().as_f64().unwrap_or(0.0),
                };
                let b = config.value.as_f64().unwrap_or(0.0);
                a < b
            }
            workflow_schema::ConditionOp::Contains => {
                let text = rv.as_text().unwrap_or("");
                let needle = config.value.as_str().unwrap_or("");
                text.contains(needle)
            }
            workflow_schema::ConditionOp::NotContains => {
                let text = rv.as_text().unwrap_or("");
                let needle = config.value.as_str().unwrap_or("");
                !text.contains(needle)
            }
            workflow_schema::ConditionOp::IsEmpty => rv.to_json() == serde_json::Value::Null,
            workflow_schema::ConditionOp::IsNotEmpty => rv.to_json() != serde_json::Value::Null,
        },
    }
}
