//! Router node — selects a downstream path.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::RouterConfig;

/// Execute a router node.
pub async fn execute(
    _config: &RouterConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    // Stub: passthrough. Full implementation would evaluate
    // routing strategy and select downstream path.
    Ok(NodeOutput::Message(input.to_json()))
}
