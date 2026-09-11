//! MCP node — invokes an MCP tool via the executor trait.
//!
//! If an `McpToolExecutor` is provided in the execution context, the node
//! calls it. Otherwise, it returns a stub result (graceful degradation).

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
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
        None => {
            // Graceful degradation — no MCP server connected.
            Ok(NodeOutput::message(RuntimeValue::Json(serde_json::json!({
                "mcp_tool": config.tool_name,
                "server": config.server_ref,
                "status": "mcp_not_connected",
            }))))
        }
    }
}
