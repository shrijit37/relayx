//! Skill node — loads and applies a skill.

use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::SkillConfig;

/// Execute a skill node. Loads skill content progressively.
pub async fn execute(
    config: &SkillConfig,
    _ctx: &ExecutionContext,
    _input: NodeInput,
) -> Result<NodeOutput, NodeError> {
    tracing::debug!(
        skill = %config.skill_ref,
        progressive = config.progressive,
        "Skill node stub — no skill registry yet"
    );

    // Stub: return a placeholder. Full implementation would:
    // 1. Load skill metadata
    // 2. If progressive, load SKILL.md content
    // 3. If needed, load references/resources
    // 4. Apply skill instructions to the input
    Ok(NodeOutput::Message(serde_json::json!({
        "skill": config.skill_ref,
        "status": "skill_node_stub",
    })))
}
