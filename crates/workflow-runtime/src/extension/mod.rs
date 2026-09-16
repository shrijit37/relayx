//! Extension traits and registry for Custom node execution.
//!
//! Custom nodes are externally registered node kinds. The runtime refuses to
//! execute them unless an `ExtensionRegistry` is installed in the
//! `ExecutionContext`. Extensions execute out-of-process via worker RPC —
//! never `dlopen` in the gateway process.
//!
//! # Architecture
//!
//! ```text
//! Workflow JSON (CustomConfig) → ExtensionValidator (optional) →
//! ExtensionRegistry (kind lookup) → ExtensionExecutor (worker RPC) →
//! NodeOutput
//! ```

pub mod worker;

use std::sync::Arc;

use crate::error::{ExtensionError, NodeError};
use crate::nodes::{NodeInput, NodeOutput};
use workflow_schema::CustomConfig;

// ─── Extension traits ──────────────────────────────────────────────────────

/// Validates a `CustomConfig` (optional, non-blocking).
///
/// Validators can be used to check extension configs. A validation failure
/// produces a warning but does **not** reject the workflow — the compiler
/// is lenient to allow draft workflows with unregistered extensions.
///
/// At present, validators are not invoked during synchronous workflow
/// compilation. Full async validation is deferred to the executor at runtime.
///
/// TODO: Wire validators into the execution path when async validation is
/// needed (e.g., before calling `ExtensionExecutor::execute` in
/// `execution.rs` for `NodeKind::Custom`).
#[async_trait::async_trait]
pub trait ExtensionValidator: Send + Sync {
    /// Validate the custom node configuration.
    ///
    /// Return `Ok(())` if the config is valid for this extension kind.
    /// Return `Err(ExtensionError)` with a descriptive message if invalid.
    async fn validate(&self, config: &CustomConfig) -> Result<(), ExtensionError>;
}

/// Executes a `CustomConfig` at runtime via worker RPC.
///
/// The executor is responsible for communicating with the worker process
/// (e.g. over a Unix domain socket). The gateway never loads extension
/// code in-process.
#[async_trait::async_trait]
pub trait ExtensionExecutor: Send + Sync {
    /// Execute the custom node with the given configuration and input.
    ///
    /// `version` is the registered [`ExtensionSpec::version`] of this
    /// extension kind, carried so the worker can verify protocol
    /// compatibility before running.
    ///
    /// Returns `Ok(NodeOutput)` on success, or `Err(NodeError)` on failure.
    /// The executor should map transport errors (worker crash, timeout,
    /// protocol error) into `NodeError::Extension` with a descriptive
    /// message.
    async fn execute(
        &self,
        config: &CustomConfig,
        version: u64,
        input: NodeInput,
    ) -> Result<NodeOutput, NodeError>;
}

// ─── Extension spec ────────────────────────────────────────────────────────

/// Registration spec for one extension kind.
///
/// Each extension kind is identified by a string `kind` and a `version`.
/// The registry stores one spec per kind; version is carried for
/// observability and snapshot identity.
pub struct ExtensionSpec {
    /// The extension kind identifier (matches `CustomConfig::kind`).
    pub kind: String,
    /// Version of this extension (for snapshot identity and observability).
    pub version: u64,
    /// Optional validator. When present, the compiler runs validation on
    /// Custom nodes of this kind (non-blocking, warnings only).
    pub validator: Option<Arc<dyn ExtensionValidator>>,
    /// Executor that performs the actual work via worker RPC.
    pub executor: Arc<dyn ExtensionExecutor>,
}

impl std::fmt::Debug for ExtensionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionSpec")
            .field("kind", &self.kind)
            .field("version", &self.version)
            .field("has_validator", &self.validator.is_some())
            .field(
                "executor_type",
                &std::any::type_name_of_val(&*self.executor),
            )
            .finish()
    }
}

// ─── Extension registry ────────────────────────────────────────────────────

/// Registry of extension specs, keyed by kind.
///
/// Populated at startup or snapshot publication time. The gateway holds
/// one registry per snapshot; request workers observe it through
/// `ExecutionContext::extension_registry`.
#[derive(Debug, Default)]
pub struct ExtensionRegistry {
    specs: std::collections::HashMap<String, ExtensionSpec>,
}

impl ExtensionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an extension spec.
    ///
    /// If a spec with the same `kind` already exists, it is replaced.
    pub fn register(&mut self, spec: ExtensionSpec) {
        self.specs.insert(spec.kind.clone(), spec);
    }

    /// Look up an extension spec by kind.
    pub fn get(&self, kind: &str) -> Option<&ExtensionSpec> {
        self.specs.get(kind)
    }

    /// Iterate over all registered extension kinds.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.specs.keys().map(|s| s.as_str())
    }

    /// Number of registered extensions.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether the registry has no extensions.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

// ─── Snapshot metadata ─────────────────────────────────────────────────────

/// Snapshot-level extension metadata (kind + version).
///
/// Carried in `RuntimeSnapshot` for observability. The actual trait
/// objects (`ExtensionValidator`, `ExtensionExecutor`) are not
/// serializable and live only in the runtime, not the snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtensionSpecSnapshot {
    /// The extension kind identifier.
    pub kind: String,
    /// Version of this extension.
    pub version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_schema::CustomConfig;

    /// A no-op validator for testing.
    struct NoopValidator;

    #[async_trait::async_trait]
    impl ExtensionValidator for NoopValidator {
        async fn validate(&self, _config: &CustomConfig) -> Result<(), ExtensionError> {
            Ok(())
        }
    }

    /// A failing validator for testing.
    struct FailingValidator;

    #[async_trait::async_trait]
    impl ExtensionValidator for FailingValidator {
        async fn validate(&self, _config: &CustomConfig) -> Result<(), ExtensionError> {
            Err(ExtensionError::Execution("test validation failure".into()))
        }
    }

    /// A stub executor for testing.
    struct StubExecutor;

    #[async_trait::async_trait]
    impl ExtensionExecutor for StubExecutor {
        async fn execute(
            &self,
            _config: &CustomConfig,
            _version: u64,
            input: NodeInput,
        ) -> Result<NodeOutput, NodeError> {
            Ok(NodeOutput::message(input.value))
        }
    }

    #[test]
    fn registry_is_empty_initially() {
        let registry = ExtensionRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.get("test").is_none());
    }

    #[test]
    fn register_and_lookup() {
        let mut registry = ExtensionRegistry::new();
        registry.register(ExtensionSpec {
            kind: "test-ext".into(),
            version: 1,
            validator: Some(Arc::new(NoopValidator)),
            executor: Arc::new(StubExecutor),
        });

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        let spec = match registry.get("test-ext") {
            Some(s) => s,
            None => panic!("spec should exist"),
        };
        assert_eq!(spec.kind, "test-ext");
        assert_eq!(spec.version, 1);
        assert!(spec.validator.is_some());
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn register_replaces_existing() {
        let mut registry = ExtensionRegistry::new();
        registry.register(ExtensionSpec {
            kind: "test-ext".into(),
            version: 1,
            validator: Some(Arc::new(NoopValidator)),
            executor: Arc::new(StubExecutor),
        });
        registry.register(ExtensionSpec {
            kind: "test-ext".into(),
            version: 2,
            validator: None,
            executor: Arc::new(StubExecutor),
        });

        assert_eq!(registry.len(), 1);
        let spec = match registry.get("test-ext") {
            Some(s) => s,
            None => panic!("spec should exist"),
        };
        assert_eq!(spec.version, 2);
        assert!(spec.validator.is_none());
    }

    #[test]
    fn kinds_iterator() {
        let mut registry = ExtensionRegistry::new();
        registry.register(ExtensionSpec {
            kind: "alpha".into(),
            version: 1,
            validator: None,
            executor: Arc::new(StubExecutor),
        });
        registry.register(ExtensionSpec {
            kind: "beta".into(),
            version: 1,
            validator: None,
            executor: Arc::new(StubExecutor),
        });

        let mut kinds: Vec<&str> = registry.kinds().collect();
        kinds.sort();
        assert_eq!(kinds, vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn noop_validator_succeeds() {
        let validator = NoopValidator;
        let config = CustomConfig {
            kind: "test".into(),
            payload: serde_json::Value::Null,
        };
        assert!(validator.validate(&config).await.is_ok());
    }

    #[tokio::test]
    async fn failing_validator_returns_error() {
        let validator = FailingValidator;
        let config = CustomConfig {
            kind: "test".into(),
            payload: serde_json::Value::Null,
        };
        let result = validator.validate(&config).await;
        assert!(result.is_err());
        match result {
            Err(ExtensionError::Execution(msg)) => {
                assert_eq!(msg, "test validation failure");
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stub_executor_passthrough() {
        let executor = StubExecutor;
        let config = CustomConfig {
            kind: "test".into(),
            payload: serde_json::json!({"key": "value"}),
        };
        let input = NodeInput::message(crate::nodes::RuntimeValue::String("hello".into()));
        let output = executor
            .execute(&config, 1, input)
            .await
            .unwrap_or_else(|e| panic!("executor should succeed: {e}"));
        assert_eq!(
            output.value,
            crate::nodes::RuntimeValue::String("hello".into())
        );
    }

    #[test]
    fn snapshot_metadata_roundtrip() {
        let spec = ExtensionSpecSnapshot {
            kind: "test-ext".into(),
            version: 42,
        };
        let json = serde_json::to_string(&spec).unwrap_or_else(|e| panic!("serialize failed: {e}"));
        let parsed: ExtensionSpecSnapshot =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("deserialize failed: {e}"));
        assert_eq!(parsed.kind, "test-ext");
        assert_eq!(parsed.version, 42);
    }
}
