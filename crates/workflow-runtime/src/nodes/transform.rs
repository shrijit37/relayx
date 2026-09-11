//! Transform node — passes data through or transforms it.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::TransformConfig;

/// Execute a transform node.
pub async fn execute(
    config: &TransformConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    match config.operation {
        workflow_schema::TransformOperation::Passthrough => {
            Ok(NodeOutput::Message(input.to_json()))
        }
        _ => {
            // Other transform operations are stubs for now.
            Ok(NodeOutput::Message(input.to_json()))
        }
    }
}
