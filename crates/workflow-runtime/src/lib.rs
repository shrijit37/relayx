//! Node-based workflow runtime for relay-x.
//!
//! Provides the execution engine for compiled workflow IR. Nodes execute
//! in topological order, passing data through typed ports. The runtime
//! supports branching, concurrency, cancellation, and typed error propagation.
//!
//! ```text
//! Workflow Definition → Validation → Compilation → Execution IR → Runtime
//! ```

pub mod capability;
pub mod compiler;
pub mod context;
pub mod error;
pub mod execution;
pub mod fast_path;
pub mod nodes;
pub mod provider;
pub mod snapshot;

pub use capability::Capabilities;
pub use compiler::{CompileContext, CompileError, compile_workflow};
pub use context::ExecutionContext;
pub use error::{NodeError, WorkflowError};
pub use execution::{
    EdgeCondition, ExecEdge, ExecNode, ExecutionPlan, NodeRuntime, PlanClassification,
};
pub use nodes::{NodeExecutor, NodeInput, NodeKind, NodeOutput, NodeRegistry, RuntimeValue};
pub use provider::{ProviderEntry, ProviderRegistry};
pub use snapshot::{RuntimeSnapshot, RuntimeSnapshotBuilder};
