//! Retry node — re-invokes a configured LLM call with retry policy.

use std::time::Duration;

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::{LlmConfig, RetryConfig};

/// Execute a retry node.
///
/// Invokes the configured LLM lane up to `max_attempts` times. A short
/// delay is applied between attempts. Only provider-side errors (the
/// upstream returned an error status) trigger a retry; client-side errors
/// surface immediately.
pub async fn execute(
    config: &RetryConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    if config.max_attempts == 0 {
        return Err(NodeError::Internal(
            "retry: max_attempts must be ≥ 1".into(),
        ));
    }

    let delay = Duration::from_millis(config.delay_ms);
    let llm_config = LlmConfig {
        protocol: None,
        model: None,
        temperature: None,
        max_tokens: None,
        stream: false,
        lane_id: Some("default".into()),
    };

    let mut attempt: u32 = 0;
    let mut last_provider_error: Option<NodeError> = None;

    while attempt < config.max_attempts {
        attempt += 1;

        match super::llm::execute(&llm_config, ctx, input.clone()).await {
            Ok(output) => return Ok(output),
            Err(e) => {
                // Only retry on provider-side failures.
                if !is_provider_error(&e) {
                    return Err(e);
                }
                tracing::warn!(
                    node_id = %ctx.node_id,
                    error = %e,
                    attempt = attempt,
                    max_attempts = config.max_attempts,
                    "retry: attempt failed"
                );
                last_provider_error = Some(e);
                if attempt < config.max_attempts {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    match last_provider_error {
        Some(e) => Err(e),
        None => Err(NodeError::Internal("retry: exhausted attempts".into())),
    }
}

fn is_provider_error(e: &NodeError) -> bool {
    matches!(e, NodeError::Provider(_))
}
