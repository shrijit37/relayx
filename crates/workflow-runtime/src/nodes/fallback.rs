//! Fallback node — tries providers in order until one succeeds.
//!
//! Each provider is tried once per "round"; `config.rounds` controls how
//! many times the whole provider list is cycled before giving up. A provider
//! whose lane is missing is skipped (counted as a failure), and a missing
//! upstream client is a hard abort only if *no* provider can run.
//!
//! With `FallbackStrategy::RoundRobin`, each request starts at the next
//! provider (`counter.fetch_add(1) % n`) so traffic spreads across lanes /
//! egress IPs. A failure whose upstream HTTP status is in `config.retry_on`
//! (e.g. 429) immediately advances to the next provider in the same round —
//! the rate-limit failover path.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::context::{ExecutionContext, LaneClient};
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::{FallbackConfig, FallbackProvider, FallbackStrategy, LlmConfig};

/// Resolve the HTTP client for a lane, preferring the per-lane connection
/// pool only. There is no shared fallback: wrapping the plain upstream
/// client as a "lane" client would leak a masked lane's egress IP (the
/// fail-closed egress contract). Mirrors `llm::resolve_client`.
fn resolve_lane_client(
    ctx: &ExecutionContext,
    lane_id: &str,
) -> Result<Arc<LaneClient>, NodeError> {
    match ctx
        .lane_clients
        .as_ref()
        .and_then(|lc| lc.client_for_lane(lane_id))
    {
        Some(c) => Ok(c),
        None => Err(NodeError::Internal(format!(
            "no connection pool for lane '{lane_id}' (masked egress requires a lane pool)"
        ))),
    }
}

/// Execute a fallback node.
pub async fn execute(
    config: &FallbackConfig,
    ctx: &ExecutionContext,
    input: NodeInput,
    counter: &AtomicUsize,
) -> Result<NodeOutput, NodeError> {
    if config.providers.is_empty() {
        return Err(NodeError::Internal("fallback: no providers".into()));
    }

    let rounds = config.rounds.max(1);
    let n = config.providers.len();
    let mut last_error: Option<NodeError> = None;
    let mut any_ran = false;

    // RoundRobin: each request starts at the next provider (fetch_add % n).
    // Sequential: always start at provider[0].
    let start = match config.strategy {
        FallbackStrategy::Sequential => 0,
        FallbackStrategy::RoundRobin => counter.fetch_add(1, Ordering::Relaxed) % n,
    };

    for round in 0..rounds {
        for offset in 0..n {
            let provider = &config.providers[(start + offset) % n];
            let FallbackProvider {
                lane_id,
                model,
                protocol,
            } = provider;

            if ctx.lane_registry.get(lane_id).is_none() {
                last_error = Some(NodeError::Internal(format!("lane not found: {lane_id}")));
                continue;
            }

            // A provider is only "attempted" when its lane actually has a
            // usable client. The per-lane pool is the only source: proxy-only
            // deployments have no shared upstream client, and a shared direct
            // wrapper must never stand in for a masked lane.
            let client = resolve_lane_client(ctx, lane_id)?;
            if client.available() {
                any_ran = true;
            } else {
                last_error = Some(NodeError::Internal("no upstream client".into()));
                continue;
            }

            tracing::debug!(
                node_id = %ctx.node_id,
                lane = %lane_id,
                model = %model,
                round = round + 1,
                strategy = ?config.strategy,
                "fallback: trying provider"
            );

            let llm_config = LlmConfig {
                protocol: protocol.clone(),
                model: Some(model.clone()),
                temperature: None,
                max_tokens: None,
                // Buffered JSON: the workflow route returns a JSON envelope
                // (token-level streaming is fast-path-only today). Streaming
                // through fallback chains is tracked as a follow-up; forcing
                // SSE here would break JSON-mode upstreams under rotation.
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
                    let status = match &e {
                        NodeError::Provider(pe) => pe.status(),
                        _ => None,
                    };

                    // 429 (or any configured retry_on code) fails over to the
                    // next provider — a transient rate limit is a lane-level
                    // signal, not a permanent provider failure. Advance
                    // `offset` (next iteration) and keep going this round.
                    let is_retryable_status = match status {
                        Some(status) => config.retry_on.contains(&status),
                        None => false,
                    };
                    if is_retryable_status {
                        let status = status.unwrap_or_default();
                        tracing::info!(
                            node_id = %ctx.node_id,
                            lane = %lane_id,
                            status = status,
                            "fallback: retryable status, advancing to next provider"
                        );
                        // Continue the loop — the next offset (mod n) is tried
                        // in this same round instead of waiting for the next
                        // round to wrap around.
                        last_error = Some(e);
                        continue;
                    }

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
