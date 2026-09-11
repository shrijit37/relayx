//! MCP node — invokes an MCP tool.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::McpConfig;

/// Execute an MCP node. Resolves and invokes an MCP tool.
pub async fn execute(
    config: &McpConfig,
    _ctx: &ExecutionContext,
    _input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    tracing::debug!(
        server = %config.server_ref,
        tool = %config.tool_name,
        deferred = config.deferred,
        "MCP node stub — no server connection yet"
    );

    // Stub: return a placeholder. Full implementation would:
    // 1. Check if tool schema is cached
    // 2. If deferred, fetch schema from MCP server
    // 3. Invoke the tool with input arguments
    // 4. Return the result
    Ok(NodeOutput::Message(serde_json::json!({
        "mcp_tool": config.tool_name,
        "server": config.server_ref,
        "status": "mcp_node_stub",
    })))
}
