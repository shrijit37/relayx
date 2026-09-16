//! Workflow and node error types.

use thiserror::Error;

/// Errors produced during workflow compilation, validation, or execution.
#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("workflow validation failed: {0}")]
    Validation(String),

    #[error("node configuration error: {0}")]
    NodeConfig(String),

    #[error("runtime execution error in node '{node_id}': {source}")]
    Runtime { node_id: String, source: NodeError },

    #[error("protocol engine error: {0}")]
    Protocol(#[from] protocol_core::error::ProtocolEngineError),

    #[error("workflow cancelled")]
    Cancelled,

    #[error("workflow timed out after {0:?}")]
    Timeout(std::time::Duration),
}

/// Errors from individual node execution.
#[derive(Debug, Error)]
pub enum NodeError {
    #[error("provider error: {0}")]
    Provider(#[from] protocol_core::error::ProtocolEngineError),

    #[error("input mismatch: expected {expected}, got {got}")]
    InputMismatch { expected: String, got: String },

    #[error("missing required input: {0}")]
    MissingInput(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("extension error: {0}")]
    Extension(#[from] ExtensionError),
}

/// Errors from extension validation or execution.
#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("extension worker unavailable: {0}")]
    WorkerUnavailable(String),

    #[error("extension not registered: {0}")]
    NotRegistered(String),

    #[error("extension validation failed: {0}")]
    Validation(String),

    #[error("extension execution failed: {0}")]
    Execution(String),
}
