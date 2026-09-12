//! Fallback node — tries providers in order until one succeeds.
//!
//! Each provider is tried once per "round"; `config.rounds` controls how
//! many times the whole provider list is cycled before giving up. A provider
//! whose lane is missing is skipped (counted as a failure), and a missing
//! upstream client is a hard abort only if *no* provider can run.

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

    let rounds = config.rounds.max(1);
    let mut last_error: Option<NodeError> = None;
    let mut any_ran = false;

    for round in 0..rounds {
        for FallbackProvider {
            lane_id,
            model,
            protocol,
        } in &config.providers
        {
            if ctx.lane_registry.get(lane_id).is_none() {
                last_error = Some(NodeError::Internal(format!("lane not found: {lane_id}")));
                continue;
            }

            if ctx.upstream_client.is_none() {
                last_error = Some(NodeError::Internal("no upstream client".into()));
                continue;
            }

            any_ran = true;

            tracing::debug!(
                node_id = %ctx.node_id,
                lane = %lane_id,
                model = %model,
                round = round + 1,
                "fallback: trying provider"
            );

            let llm_config = LlmConfig {
                protocol: protocol.clone(),
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
    }

    if !any_ran {
        // None of the providers could be attempted (e.g. all lanes missing or
        // the client absent) — that's a hard error, not a "tried and failed".
        return Err(NodeError::Internal(
            "fallback: no provider could be attempted (missing lanes or client)".into(),
        ));
    }

    Err(last_error
        .unwrap_or_else(|| NodeError::Internal("fallback: all providers exhausted".into())))
}
