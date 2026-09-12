//! Retry node — re-invokes a configured LLM call with retry policy.

use std::time::Duration;

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::RetryConfig;

/// Execute a retry node.
///
/// Invokes the configured target LLM lane up to `max_attempts` times. A short
/// delay is applied between attempts. Only errors classified by
/// `on_timeout` / `on_provider_error` trigger a retry; client-side errors
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
    let mut attempt: u32 = 0;
    let mut last_error: Option<NodeError> = None;

    while attempt < config.max_attempts {
        attempt += 1;

        match super::llm::execute(&config.target, ctx, input.clone()).await {
            Ok(output) => return Ok(output),
            Err(e) => {
                // Only retry on errors the policy opts into.
                if !should_retry(config, &e) {
                    return Err(e);
                }
                tracing::warn!(
                    node_id = %ctx.node_id,
                    error = %e,
                    attempt = attempt,
                    max_attempts = config.max_attempts,
                    "retry: attempt failed"
                );
                last_error = Some(e);
                if attempt < config.max_attempts {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    match last_error {
        Some(e) => Err(e),
        None => Err(NodeError::Internal("retry: exhausted attempts".into())),
    }
}

/// Whether an error should trigger another attempt under the config policy.
///
/// `ProtocolEngineError` has no dedicated Timeout variant, so provider-side
/// failures (upstream HTTP/transport — the set containing timeouts) are
/// gated entirely by `on_provider_error`. Client-side/internal errors are
/// never retried.
fn should_retry(config: &RetryConfig, e: &NodeError) -> bool {
    match e {
        NodeError::Provider(_) => config.on_provider_error,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol_core::error::ProtocolEngineError;
    use workflow_schema::LlmConfig;

    fn cfg(on_provider: bool) -> RetryConfig {
        RetryConfig {
            max_attempts: 3,
            delay_ms: 0,
            on_timeout: true,
            on_provider_error: on_provider,
            target: LlmConfig {
                protocol: None,
                model: None,
                temperature: None,
                max_tokens: None,
                stream: false,
                lane_id: Some("real-lane".into()),
            },
        }
    }

    #[test]
    fn provider_error_retried_when_policy_allows() {
        let config = cfg(true);
        let err = NodeError::Provider(ProtocolEngineError::ProviderError {
            message: "upstream 500".into(),
        });
        assert!(should_retry(&config, &err));
    }

    #[test]
    fn provider_error_not_retried_when_policy_denies() {
        let config = cfg(false);
        let err = NodeError::Provider(ProtocolEngineError::ProviderError {
            message: "upstream 500".into(),
        });
        assert!(!should_retry(&config, &err));
    }

    #[test]
    fn internal_error_never_retried() {
        let config = cfg(true);
        let err = NodeError::Internal("boom".into());
        assert!(!should_retry(&config, &err));
    }

    #[test]
    fn input_mismatch_never_retried() {
        let config = cfg(true);
        let err = NodeError::InputMismatch {
            expected: "x".into(),
            got: "y".into(),
        };
        assert!(!should_retry(&config, &err));
    }

    #[test]
    fn target_lane_is_preserved_in_config() {
        // The corrected retry must NOT hard-code "default"; the target carries
        // the real lane the workflow author configured.
        let config = cfg(true);
        assert_eq!(config.target.lane_id.as_deref(), Some("real-lane"));
    }
}
