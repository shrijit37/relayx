//! MCP node — invokes an MCP tool via the executor trait.
//!
//! If an `McpToolExecutor` is provided in the execution context, the node
//! calls it. Otherwise it fails — a fabricated "success" must never reach
//! downstream nodes.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::McpConfig;

/// Execute an MCP node. Resolves and invokes an MCP tool.
pub async fn execute(
    config: &McpConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    tracing::debug!(
        server = %config.server_ref,
        tool = %config.tool_name,
        deferred = config.deferred,
        has_executor = ctx.mcp_executor.is_some(),
        "MCP node executing"
    );

    match &ctx.mcp_executor {
        Some(executor) => {
            // Delegate to the real MCP tool executor.
            let result = executor
                .execute_tool(&config.server_ref, &config.tool_name, &input.value)
                .await?;
            Ok(NodeOutput::message(result))
        }
        None => Err(NodeError::Internal(format!(
            "MCP executor not available: tool '{}' cannot be executed",
            config.tool_name
        ))),
    }
}
