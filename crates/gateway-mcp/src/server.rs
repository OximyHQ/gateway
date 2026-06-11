//! `McpServer` — a named upstream MCP server with a transport.
//!
//! This is the gateway-side representation of one upstream server.  It
//! provides high-level methods (`list_tools`, `call_tool`) that speak the MCP
//! JSON-RPC dialect on top of the transport layer.

use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use crate::{
    MCP_PROTOCOL_VERSION,
    error::McpError,
    jsonrpc::{JsonRpcError, JsonRpcRequest, RequestId},
    transport::McpTransport,
    types::{ToolCallParams, ToolCallResult, ToolsListParams, ToolsListResult},
};

// ─── McpServer ───────────────────────────────────────────────────────────────

/// An upstream MCP server registered with the gateway.
pub struct McpServer {
    pub name: String,
    transport: Arc<dyn McpTransport>,
}

impl McpServer {
    pub fn new(name: impl Into<String>, transport: Arc<dyn McpTransport>) -> Self {
        Self {
            name: name.into(),
            transport,
        }
    }

    /// Call `tools/list` and collect all pages.
    pub async fn list_tools(&self) -> Result<ToolsListResult, McpError> {
        let mut all_tools = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = ToolsListParams {
                cursor: cursor.clone(),
            };
            let req = JsonRpcRequest::new(
                RequestId::Num(0), // transport will re-assign id
                "tools/list",
                Some(
                    serde_json::to_value(&params)
                        .map_err(|e| McpError::Serialization(e.to_string()))?,
                ),
            );

            let resp = self.transport.call(req).await?;
            if let Some(err) = resp.error {
                return Err(McpError::UpstreamError(err.to_string()));
            }

            let result_val = resp
                .result
                .ok_or_else(|| McpError::Serialization("tools/list: missing result".into()))?;
            let result: ToolsListResult = serde_json::from_value(result_val)
                .map_err(|e| McpError::Serialization(e.to_string()))?;

            let next = result.next_cursor.clone();
            all_tools.extend(result.tools);

            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }

        debug!(server = %self.name, tool_count = all_tools.len(), "listed tools");
        Ok(ToolsListResult {
            tools: all_tools,
            next_cursor: None,
        })
    }

    /// Call `tools/call` on this server with the **original** (un-namespaced) tool name.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<ToolCallResult, McpError> {
        let params = ToolCallParams {
            name: tool_name.to_string(),
            arguments,
        };
        let req = JsonRpcRequest::new(
            RequestId::Num(0),
            "tools/call",
            Some(
                serde_json::to_value(&params)
                    .map_err(|e| McpError::Serialization(e.to_string()))?,
            ),
        );

        let resp = self.transport.call(req).await?;
        if let Some(err) = resp.error {
            return Err(McpError::UpstreamError(err.to_string()));
        }

        let result_val = resp
            .result
            .ok_or_else(|| McpError::Serialization("tools/call: missing result".into()))?;
        let result: ToolCallResult = serde_json::from_value(result_val)
            .map_err(|e| McpError::Serialization(e.to_string()))?;
        Ok(result)
    }

    /// Perform the `initialize` handshake (required by the 2025-11-25 spec
    /// before any other method).  The gateway sends its own `clientInfo`.
    pub async fn initialize(&self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "oximy-gateway", "version": env!("CARGO_PKG_VERSION") }
        });
        let req = JsonRpcRequest::new(RequestId::Num(0), "initialize", Some(params));
        let resp = self.transport.call(req).await?;
        if let Some(err) = resp.error {
            return Err(McpError::UpstreamError(format!("initialize: {err}")));
        }
        // Send the initialized notification (fire-and-forget, best-effort).
        // The notification path is deferred — for now we just log.
        debug!(server = %self.name, "initialize handshake complete");
        Ok(())
    }

    pub fn transport_label(&self) -> &str {
        self.transport.label()
    }
}

// ─── JSON-RPC error extraction helper ────────────────────────────────────────

/// Extract `JsonRpcError` from a response, or return an internal error.
pub fn extract_error(resp: &crate::jsonrpc::JsonRpcResponse) -> JsonRpcError {
    resp.error
        .clone()
        .unwrap_or_else(|| JsonRpcError::internal("missing both result and error"))
}
