//! Stable node extension contract.
//!
//! The core execution engine dispatches to nodes through this trait when a
//! node kind is not one of the built-in variants. New node kinds can be added
//! without modifying `ExecutionPlan`, `NodeRuntime`, or the topological
//! scheduler: register a `NodeExecutor` in the `NodeRegistry` and the engine
//! finds it by kind string.

use async_trait::async_trait;

use crate::capability::Capabilities;
use crate::context::ExecutionContext;
use crate::error::NodeError;
use crate::nodes::{NodeInput, NodeOutput};

/// The kind identifier for a node (e.g. `"debug_echo"`).
pub const REGISTERED_KIND_FIELD: &str = "kind";

/// A node implementation registered with the engine.
///
/// Implementations are executed for nodes whose `kind` string matches
/// [`NodeExecutor::kind`]. The engine checks the registry before falling back
/// to the built-in match dispatch, so extensions never touch the scheduler.
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    /// Stable kind identifier. Must match the workflow node's `kind` field.
    fn kind(&self) -> &str;

    /// Statically declared capabilities. Used by the compiler to validate
    /// provider compatibility before execution.
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    /// Execute the node within an execution context.
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        input: NodeInput,
    ) -> Result<NodeOutput, NodeError>;
}

/// Thread-safe registered node implementations.
pub type RegisteredNode = Box<dyn NodeExecutor>;

/// Registry of externally added node kinds.
///
/// The registry is intentionally append-only and shared immutably after
/// construction. The engine reads it without locking per request.
#[derive(Default)]
pub struct NodeRegistry {
    nodes: Vec<RegisteredNode>,
}

impl std::fmt::Debug for NodeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRegistry")
            .field("registered_kinds", &self.kind_names())
            .finish()
    }
}

impl NodeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a node implementation.
    pub fn register(&mut self, node: impl NodeExecutor + 'static) {
        self.nodes.push(Box::new(node));
    }

    /// Find a registered executor for a node kind.
    pub fn get(&self, kind: &str) -> Option<&dyn NodeExecutor> {
        self.nodes.iter().find(|n| n.kind() == kind).map(|b| &**b)
    }

    /// Number of registered nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the registry has no registered nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Return the kind names of all registered nodes.
    pub fn kind_names(&self) -> Vec<&str> {
        self.nodes.iter().map(|n| n.kind()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::NodeOutput;
    use crate::nodes::{NodeInput, RuntimeValue};
    use std::sync::Arc;

    #[derive(Default)]
    struct SucceedingNode;

    #[async_trait]
    impl NodeExecutor for SucceedingNode {
        fn kind(&self) -> &str {
            "unit_test_ok"
        }

        async fn execute(
            &self,
            _ctx: &ExecutionContext,
            input: NodeInput,
        ) -> Result<NodeOutput, NodeError> {
            Ok(NodeOutput::message(input.value))
        }
    }

    #[derive(Default)]
    struct FailingNode;

    #[async_trait]
    impl NodeExecutor for FailingNode {
        fn kind(&self) -> &str {
            "unit_test_err"
        }

        async fn execute(
            &self,
            _ctx: &ExecutionContext,
            _input: NodeInput,
        ) -> Result<NodeOutput, NodeError> {
            Err(NodeError::Internal("unit test failure".into()))
        }
    }

    #[test]
    fn registry_finds_by_kind() {
        let mut registry = NodeRegistry::new();
        registry.register(SucceedingNode);
        registry.register(FailingNode);

        assert_eq!(registry.len(), 2);
        assert!(registry.get("unit_test_ok").is_some());
        assert!(registry.get("unit_test_err").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn empty_registry_is_empty() {
        let registry = NodeRegistry::new();
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn registry_executes_registered_node() {
        let mut registry = NodeRegistry::new();
        registry.register(SucceedingNode);

        let lanes = Arc::new(crate::context::LaneRegistry::new());
        let ctx = crate::context::ExecutionContext::new("wf".into(), "run".into(), lanes);

        let executor = match registry.get("unit_test_ok") {
            Some(e) => e,
            None => panic!("node should be registered"),
        };
        let out = match executor
            .execute(&ctx, NodeInput::message(RuntimeValue::String("hi".into())))
            .await
        {
            Ok(o) => o,
            Err(e) => panic!("node should succeed: {e}"),
        };
        assert_eq!(out.value, RuntimeValue::String("hi".into()));
    }
}
