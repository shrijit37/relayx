//! Skill node — loads and applies a skill via the loader trait.
//!
//! If a `SkillLoader` is provided in the execution context, the node calls it.
//! Otherwise, it returns a stub result (graceful degradation).

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
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
        None => {
            // Graceful degradation — no skill registry connected.
            Ok(NodeOutput::message(RuntimeValue::Json(serde_json::json!({
                "skill": config.skill_ref,
                "status": "skill_not_loaded",
            }))))
        }
    }
}
