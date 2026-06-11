//! The `McpTransport` trait and its two concrete impls:
//!   • `StdioTransport` — speaks JSON-RPC over a child process's stdin/stdout.
//!   • `HttpTransport`  — POST JSON-RPC to a streamable-HTTP endpoint.
//!
//! Both impls fulfil the *upstream server* contract only (the gateway is the
//! client side).  Inbound (gateway-as-server) transport is deferred.
//!
//! # Deferred
//! - Streamable-HTTP SSE server side (inbound SSE streams, GET listeners).
//! - OAuth 2.1 inbound/outbound brokering.
//! - Session affinity / `MCP-Session-Id` round-trip.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};
use tracing::{debug, error, warn};

use crate::{
    error::McpError,
    jsonrpc::{JsonRpcRequest, JsonRpcResponse, RequestId},
};

// ─── Trait ───────────────────────────────────────────────────────────────────

/// A connection to a single upstream MCP server.
///
/// Implementors must be `Send + Sync` so they can live in `Arc<dyn McpTransport>`.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a JSON-RPC request and wait for the matching response.
    async fn call(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError>;

    /// Human-readable label for logging/audit (e.g. "stdio:my-server").
    fn label(&self) -> &str;
}

// ─── Stdio ───────────────────────────────────────────────────────────────────

/// State shared across concurrent callers on one stdio child process.
struct StdioState {
    child: Child,
    next_id: i64,
}

/// JSON-RPC over a child process's stdin/stdout.
///
/// Each `call` serialises the request as a newline-terminated JSON line,
/// then reads lines until it finds the matching `id`.  This is correct for
/// single-threaded callers; for concurrent use the federation serialises
/// calls via the `Mutex` (acceptable for a first cut — proper mux is P2+).
pub struct StdioTransport {
    label: String,
    state: Arc<Mutex<StdioState>>,
}

impl StdioTransport {
    /// Spawn `cmd` and wrap its stdio.
    pub async fn spawn(label: impl Into<String>, cmd: &mut Command) -> Result<Self, McpError> {
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let child = cmd
            .spawn()
            .map_err(|e| McpError::TransportError(format!("spawn failed: {e}")))?;

        let label = label.into();
        debug!(transport = %label, "stdio transport spawned");
        Ok(Self {
            label,
            state: Arc::new(Mutex::new(StdioState { child, next_id: 1 })),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    fn label(&self) -> &str {
        &self.label
    }

    async fn call(&self, mut req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let mut guard = self.state.lock().await;
        let id = guard.next_id;
        guard.next_id += 1;
        req.id = RequestId::Num(id);

        // --- write ---
        let line = {
            let mut s =
                serde_json::to_string(&req).map_err(|e| McpError::Serialization(e.to_string()))?;
            s.push('\n');
            s
        };

        let stdin = guard
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| McpError::TransportError("stdin closed".into()))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::TransportError(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::TransportError(e.to_string()))?;

        // --- read matching response ---
        let stdout = guard
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| McpError::TransportError("stdout closed".into()))?;
        let mut reader = BufReader::new(stdout);
        let target_id = RequestId::Num(id);
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .await
                .map_err(|e| McpError::TransportError(e.to_string()))?;
            if n == 0 {
                return Err(McpError::TransportError("stdout EOF".into()));
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<JsonRpcResponse>(line) {
                Ok(resp) if resp.id == target_id => return Ok(resp),
                Ok(resp) => {
                    warn!(transport = %self.label, "dropping out-of-order response id={:?}", resp.id);
                }
                Err(e) => {
                    error!(transport = %self.label, parse_err = %e, line = %line, "non-response line from server");
                }
            }
        }
    }
}

// ─── HTTP ────────────────────────────────────────────────────────────────────

/// JSON-RPC over streamable HTTP (POST JSON-RPC, expect JSON response).
///
/// The full SSE server-push path (GET listener) is deferred; we handle the
/// simple request→JSON-response path which covers `initialize`, `tools/list`,
/// and `tools/call`.
pub struct HttpTransport {
    label: String,
    endpoint: String,
    client: reqwest::Client,
    next_id: Mutex<i64>,
}

impl HttpTransport {
    pub fn new(label: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            endpoint: endpoint.into(),
            client: reqwest::Client::new(),
            next_id: Mutex::new(1),
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    fn label(&self) -> &str {
        &self.label
    }

    async fn call(&self, mut req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let id = {
            let mut g = self.next_id.lock().await;
            let id = *g;
            *g += 1;
            id
        };
        req.id = RequestId::Num(id);
        debug!(transport = %self.label, method = %req.method, id, "http call");

        let resp = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&req)
            .send()
            .await
            .map_err(|e| McpError::TransportError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(McpError::TransportError(format!(
                "HTTP {status} from {label}",
                label = self.label
            )));
        }

        // For the first cut we only handle `application/json` replies.
        // SSE replies are deferred (seam left open by the trait).
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("text/event-stream") {
            return Err(McpError::TransportError(
                "SSE response path not yet implemented (deferred)".into(),
            ));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| McpError::Serialization(e.to_string()))?;
        let rpc_resp: JsonRpcResponse =
            serde_json::from_value(body).map_err(|e| McpError::Serialization(e.to_string()))?;
        Ok(rpc_resp)
    }
}

// ─── Mock (test-only) ────────────────────────────────────────────────────────

/// A fully in-process mock transport for unit tests.
///
/// Register method handlers with `on(method, handler)`.  Unknown methods
/// return a JSON-RPC `MethodNotFound` error response.
#[cfg(test)]
pub mod mock {
    use super::*;
    use crate::jsonrpc::JsonRpcError;
    use std::collections::HashMap;

    type HandlerFn = Box<dyn Fn(Option<Value>) -> Value + Send + Sync>;

    pub struct MockTransport {
        label: String,
        handlers: HashMap<String, HandlerFn>,
    }

    impl MockTransport {
        pub fn new(label: impl Into<String>) -> Self {
            Self {
                label: label.into(),
                handlers: HashMap::new(),
            }
        }

        pub fn on(
            mut self,
            method: impl Into<String>,
            f: impl Fn(Option<Value>) -> Value + Send + Sync + 'static,
        ) -> Self {
            self.handlers.insert(method.into(), Box::new(f));
            self
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        fn label(&self) -> &str {
            &self.label
        }

        async fn call(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
            match self.handlers.get(&req.method) {
                Some(f) => {
                    let result = f(req.params.clone());
                    Ok(JsonRpcResponse::success(req.id, result))
                }
                None => Ok(JsonRpcResponse::error(
                    req.id,
                    JsonRpcError::method_not_found(&req.method),
                )),
            }
        }
    }
}
