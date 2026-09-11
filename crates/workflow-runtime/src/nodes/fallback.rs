//! Fallback node — tries providers in order until one succeeds.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::{FallbackConfig, FallbackProvider, LlmConfig};

/// Execute a fallback node.
pub async fn execute(
    config: &FallbackConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    if config.providers.is_empty() {
        return Err(NodeError::Internal("fallback: no providers".into()));
    }

    let mut last_error: Option<NodeError> = None;
    let mut attempts: u32 = 0;

    for FallbackProvider { lane_id, model } in &config.providers {
        attempts += 1;
        if attempts > config.max_retries + 1 {
            break;
        }

        if ctx.lane_registry.get(lane_id).is_none() {
            last_error = Some(NodeError::Internal(format!("lane not found: {lane_id}")));
            continue;
        }
        if ctx.upstream_client.is_none() {
            return Err(NodeError::Internal("no upstream client".into()));
        }

        tracing::debug!(
            node_id = %ctx.node_id,
            lane = %lane_id,
            model = %model,
            attempt = attempts,
            "fallback: trying provider"
        );

        let llm_config = LlmConfig {
            protocol: None,
            model: Some(model.clone()),
            temperature: None,
            max_tokens: None,
            stream: false,
            lane_id: Some(lane_id.clone()),
        };

        match super::llm::execute(&llm_config, ctx, input.clone()).await {
            Ok(output) => return Ok(output),
            Err(e) => {
                tracing::warn!(
                    node_id = %ctx.node_id,
                    lane = %lane_id,
                    error = %e,
                    "fallback: provider failed"
                );
                last_error = Some(e);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| NodeError::Internal("fallback: all providers exhausted".into())))
}
