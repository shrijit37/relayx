//! Skill node — loads and applies a skill via the loader trait.
//!
//! If a `SkillLoader` is provided in the execution context, the node calls it.
//! Otherwise it fails — a fabricated "success" must never reach downstream nodes.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::SkillConfig;

/// Execute a skill node. Loads skill content progressively.
pub async fn execute(
    config: &SkillConfig,
    ctx: &ExecutionContext,
    _input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    tracing::debug!(
        skill = %config.skill_ref,
        progressive = config.progressive,
        has_loader = ctx.skill_loader.is_some(),
        "Skill node executing"
    );

    match &ctx.skill_loader {
        Some(loader) => {
            let result = loader.load_skill(&config.skill_ref).await?;
            Ok(NodeOutput::message(result))
        }
        None => Err(NodeError::Internal(format!(
            "Skill loader not available: skill '{}' cannot be loaded",
            config.skill_ref
        ))),
    }
}
