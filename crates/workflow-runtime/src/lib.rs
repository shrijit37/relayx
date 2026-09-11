//! Node-based workflow runtime for relay-x.
//!
//! Provides the execution engine for compiled workflow IR. Nodes execute
//! in topological order, passing data through typed ports. The runtime
//! supports streaming, cancellation, and typed error propagation.
//!
//! ```text
//! Workflow Definition → Validation → Compilation → Execution IR → Runtime
//! ```

pub mod context;
pub mod error;
pub mod execution;
pub mod nodes;

pub use context::ExecutionContext;
pub use error::{NodeError, WorkflowError};
pub use execution::{ExecEdge, ExecNode, ExecutionPlan, NodeRuntime};
pub use nodes::{NodeInput, NodeKind, NodeOutput};
