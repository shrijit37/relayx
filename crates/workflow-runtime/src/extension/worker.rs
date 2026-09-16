//! Worker RPC transport for custom node execution.
//!
//! Custom nodes execute out-of-process via a local IPC channel — never
//! `dlopen` in the gateway. This module implements the Unix domain socket
//! transport. TCP can be added later behind the same [`ExtensionExecutor`]
//! trait.
//!
//! # Wire protocol
//!
//! Requests and responses are single JSON messages, length-prefixed with a
//! 4-byte big-endian frame header:
//!
//! ```text
//! [len u32 BE][json body]
//! ```
//!
//! Request body (`ExtensionRequest`):
//!
//! ```json
//! { "kind": "...", "version": 1, "payload": {...}, "input": {...} }
//! ```
//!
//! Response body (`ExtensionResponse`):
//!
//! ```json
//! { "ok": true,  "port": "out", "value": {...} }
//! { "ok": false, "error": "message" }
//! ```

use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::error::{ExtensionError, NodeError};
use crate::nodes::{NodeInput, NodeOutput, RuntimeValue};
use workflow_schema::CustomConfig;

use super::ExtensionExecutor;

/// Default per-call timeout for worker RPC.
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum response frame size (64 MiB). Prevents a misbehaving worker
/// from consuming unbounded memory.
const MAX_RESPONSE_FRAME: usize = 64 * 1024 * 1024;

/// A request frame sent to a worker over the socket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ExtensionRequest {
    /// The extension kind.
    kind: String,
    /// The extension version.
    version: u64,
    /// The opaque configuration payload.
    payload: serde_json::Value,
    /// The node input value (as JSON).
    input: serde_json::Value,
}

/// A response frame received from a worker.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ExtensionResponse {
    /// Whether execution succeeded.
    ok: bool,
    /// Output port on success.
    #[serde(default)]
    port: Option<String>,
    /// Output value on success.
    #[serde(default)]
    value: Option<serde_json::Value>,
    /// Error message on failure.
    #[serde(default)]
    error: Option<String>,
}

/// An `ExtensionExecutor` that talks to a worker process over a Unix
/// domain socket.
///
/// The worker is external (not spawned by the gateway). The gateway
/// connects per call — connection pooling for hot extensions is a later
/// optimization; correctness and isolation come first.
pub struct UnixSocketExecutor {
    socket_path: PathBuf,
    /// Per-call timeout.
    timeout: Duration,
}

impl std::fmt::Debug for UnixSocketExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixSocketExecutor")
            .field("socket_path", &self.socket_path)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl UnixSocketExecutor {
    /// Create an executor for the worker listening at `socket_path`.
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: DEFAULT_RPC_TIMEOUT,
        }
    }

    /// Create an executor with a custom per-call timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The socket path this executor talks to.
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

#[async_trait::async_trait]
impl ExtensionExecutor for UnixSocketExecutor {
    async fn execute(
        &self,
        config: &CustomConfig,
        version: u64,
        input: NodeInput,
    ) -> Result<NodeOutput, NodeError> {
        let request = ExtensionRequest {
            kind: config.kind.clone(),
            version,
            payload: config.payload.clone(),
            input: input.value.to_json(),
        };

        let body = serde_json::to_vec(&request).map_err(|e| {
            NodeError::Extension(ExtensionError::Execution(format!(
                "failed to encode extension request: {e}"
            )))
        })?;

        let response = tokio::time::timeout(self.timeout, self.round_trip(&body))
            .await
            .map_err(|_| {
                NodeError::Extension(ExtensionError::WorkerUnavailable(format!(
                    "extension worker at '{}' timed out after {:?}",
                    self.socket_path.display(),
                    self.timeout
                )))
            })??;

        if response.ok {
            let value = response.value.unwrap_or(serde_json::Value::Null);
            Ok(NodeOutput {
                port: response.port,
                value: RuntimeValue::from_json(value),
            })
        } else {
            let msg = response.error.unwrap_or_else(|| "unknown error".into());
            Err(NodeError::Extension(ExtensionError::Execution(format!(
                "extension worker at '{}' failed: {msg}",
                self.socket_path.display()
            ))))
        }
    }
}

impl UnixSocketExecutor {
    // TODO: pool connections per socket_path to avoid reconnect overhead.
    /// One connect -> write -> read -> close round trip.
    async fn round_trip(&self, body: &[u8]) -> Result<ExtensionResponse, NodeError> {
        let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            NodeError::Extension(ExtensionError::WorkerUnavailable(format!(
                "cannot connect to extension worker at '{}': {e}",
                self.socket_path.display()
            )))
        })?;

        // Frame: 4-byte big-endian length + JSON body.
        let len = u32::try_from(body.len()).map_err(|_| {
            NodeError::Extension(ExtensionError::Execution(
                "extension request body exceeds u32 frame limit".into(),
            ))
        })?;
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(body);

        stream.write_all(&frame).await.map_err(|e| {
            NodeError::Extension(ExtensionError::Execution(format!(
                "failed to write extension request to '{}': {e}",
                self.socket_path.display()
            )))
        })?;

        // Read the response frame header.
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await.map_err(|e| {
            NodeError::Extension(ExtensionError::Execution(format!(
                "failed to read extension response header from '{}': {e}",
                self.socket_path.display()
            )))
        })?;
        let resp_len = u32::from_be_bytes(header);
        let resp_len = usize::try_from(resp_len).map_err(|_| {
            NodeError::Extension(ExtensionError::Execution(
                "response frame length overflow".into(),
            ))
        })?;
        if resp_len > MAX_RESPONSE_FRAME {
            return Err(NodeError::Extension(ExtensionError::Execution(format!(
                "extension worker at '{}' returned an oversized frame ({resp_len} bytes)",
                self.socket_path.display()
            ))));
        }

        let mut resp_body = vec![0u8; resp_len];
        stream.read_exact(&mut resp_body).await.map_err(|e| {
            if e.kind() == ErrorKind::UnexpectedEof {
                NodeError::Extension(ExtensionError::WorkerUnavailable(format!(
                    "extension worker at '{}' closed the connection mid-response (crashed?)",
                    self.socket_path.display()
                )))
            } else {
                NodeError::Extension(ExtensionError::Execution(format!(
                    "failed to read extension response body from '{}': {e}",
                    self.socket_path.display()
                )))
            }
        })?;

        serde_json::from_slice(&resp_body).map_err(|e| {
            NodeError::Extension(ExtensionError::Execution(format!(
                "invalid extension response from '{}': {e}",
                self.socket_path.display()
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    /// Serve a single request over a Unix socket connection, echoing the
    /// input back and recording what was received.
    async fn serve_one(stream: UnixStream) -> Result<ExtensionRequest, String> {
        let mut stream = stream;
        let mut header = [0u8; 4];
        stream
            .read_exact(&mut header)
            .await
            .map_err(|e| e.to_string())?;
        let len = u32::from_be_bytes(header);
        if len > MAX_RESPONSE_FRAME as u32 {
            return Err("frame length overflow".into());
        }
        let mut body = vec![0u8; len as usize];
        stream
            .read_exact(&mut body)
            .await
            .map_err(|e| e.to_string())?;
        let req: ExtensionRequest = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

        // Echo input back as the output value.
        let resp = ExtensionResponse {
            ok: true,
            port: Some("out".into()),
            value: Some(req.input.clone()),
            error: None,
        };
        let resp_body = serde_json::to_vec(&resp).map_err(|e| e.to_string())?;
        let len = u32::try_from(resp_body.len()).map_err(|e| e.to_string())?;
        let mut frame = Vec::new();
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&resp_body);
        stream.write_all(&frame).await.map_err(|e| e.to_string())?;
        Ok(req)
    }

    /// A test helper for socket paths.
    fn test_socket_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        dir.join(format!("relayx-ext-test-{name}-{}", std::process::id()))
    }

    #[tokio::test]
    async fn unix_socket_executor_round_trip() {
        let path = test_socket_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => panic!("bind failed: {e}"),
        };

        let task = tokio::spawn(async move {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => panic!("accept failed: {e}"),
            };
            serve_one(stream).await
        });

        let executor = UnixSocketExecutor::new(path.clone());
        let config = CustomConfig {
            kind: "echo".into(),
            payload: serde_json::json!({"x": 1}),
        };
        let input = NodeInput::message(RuntimeValue::Json(serde_json::json!({"hello": "world"})));
        let output = match executor.execute(&config, 1, input).await {
            Ok(o) => o,
            Err(e) => panic!("execute failed: {e}"),
        };
        assert_eq!(
            output.value.to_json(),
            serde_json::json!({"hello": "world"})
        );

        let received = match task.await {
            Ok(r) => r,
            Err(e) => panic!("task failed: {e}"),
        };
        let received = match received {
            Ok(r) => r,
            Err(e) => panic!("server failed: {e}"),
        };
        assert_eq!(received.kind, "echo");
        assert_eq!(received.payload, serde_json::json!({"x": 1}));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn unix_socket_executor_connection_refused() {
        let path = test_socket_path("nobody-listens");
        let _ = std::fs::remove_file(&path);
        let executor = UnixSocketExecutor::new(path.clone()).with_timeout(Duration::from_secs(5));
        let config = CustomConfig {
            kind: "echo".into(),
            payload: serde_json::Value::Null,
        };
        let input = NodeInput::message(RuntimeValue::Null);
        let result = executor.execute(&config, 1, input).await;
        assert!(result.is_err(), "connection to a dead socket must fail");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn unix_socket_executor_error_response() {
        let path = test_socket_path("error");
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => panic!("bind failed: {e}"),
        };

        let task = tokio::spawn(async move {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => panic!("accept failed: {e}"),
            };
            let mut stream = stream;
            let mut header = [0u8; 4];
            stream
                .read_exact(&mut header)
                .await
                .map_err(|e| panic!("read header: {e}"))
                .ok();
            let len = u32::from_be_bytes(header);
            let mut body = vec![0u8; len as usize];
            stream
                .read_exact(&mut body)
                .await
                .map_err(|e| panic!("read body: {e}"))
                .ok();

            let resp = ExtensionResponse {
                ok: false,
                port: None,
                value: None,
                error: Some("worker exploded".into()),
            };
            let resp_body = match serde_json::to_vec(&resp) {
                Ok(b) => b,
                Err(e) => panic!("encode response: {e}"),
            };
            let len = match u32::try_from(resp_body.len()) {
                Ok(l) => l,
                Err(e) => panic!("length fits: {e}"),
            };
            let mut frame = Vec::new();
            frame.extend_from_slice(&len.to_be_bytes());
            frame.extend_from_slice(&resp_body);
            stream
                .write_all(&frame)
                .await
                .map_err(|e| panic!("write response: {e}"))
                .ok();
        });

        let executor = UnixSocketExecutor::new(path.clone());
        let config = CustomConfig {
            kind: "echo".into(),
            payload: serde_json::Value::Null,
        };
        let input = NodeInput::message(RuntimeValue::Null);
        let result = executor.execute(&config, 1, input).await;
        let _ = task.await;

        match result {
            Err(NodeError::Extension(ExtensionError::Execution(msg))) => {
                assert!(msg.contains("worker exploded"), "unexpected message: {msg}");
            }
            other => panic!("expected ExtensionError::Execution, got: {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }
}
