//! Router node — selects a downstream path using a routing strategy.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::RouterConfig;

/// Execute a router node. Selects a downstream path based on the routing strategy.
/// Returns the input on the selected port: "route_0", "route_1", etc.
pub async fn execute(
    config: &RouterConfig,
    _ctx: &ExecutionContext,
    input: NodeInput,
    counter: &AtomicUsize,
) -> Result<NodeOutput, NodeError> {
    let selected = match config.strategy {
        workflow_schema::RouterStrategy::FirstMatch => 0,
        workflow_schema::RouterStrategy::RoundRobin => counter.fetch_add(1, Ordering::Relaxed) % 2,
        workflow_schema::RouterStrategy::LoadBased => 0,
    };

    Ok(NodeOutput::on_port(
        format!("route_{selected}"),
        input.value,
    ))
}
