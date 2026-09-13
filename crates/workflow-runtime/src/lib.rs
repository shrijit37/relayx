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
pub mod milestone;
pub mod nodes;
pub mod provider;
pub mod publish;
pub mod runner;
pub mod snapshot;

pub use capability::Capabilities;
pub use compiler::{CompileContext, CompileError, compile_workflow};
pub use context::{AsLaneClient, ExecutionContext, ExecutionMetadata, GatewayHttpClient};
pub use error::{NodeError, WorkflowError};
pub use execution::{
    EdgeCondition, ExecEdge, ExecNode, ExecutionPlan, NodeRuntime, PlanClassification,
};
pub use milestone::{MilestoneReporter, NoopReporter};
pub use nodes::{NodeExecutor, NodeInput, NodeKind, NodeOutput, NodeRegistry, RuntimeValue};
pub use provider::ProviderEntry;
pub use publish::{InMemoryPublisher, SnapshotPublisher, SnapshotReader};
pub use runner::{build_snapshot, compile_workflow_with_lanes};
pub use snapshot::{RuntimeSnapshot, RuntimeSnapshotBuilder};
