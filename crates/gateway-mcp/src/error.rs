//! Error types for the `gateway-mcp` crate.

use crate::jsonrpc::JsonRpcError;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// JSON-RPC protocol-level error (code + message from the wire).
    #[error("json-rpc error: {0}")]
    JsonRpc(#[from] JsonRpcError),

    /// Transport I/O failure (stdio EOF, HTTP error, etc.).
    #[error("transport error: {0}")]
    TransportError(String),

    /// (De)serialization failure.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The requested tool is not in this server's tool list.
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    /// The caller's key does not have permission to call this tool.
    #[error("tool not allowed for caller: {0}")]
    ToolNotAllowed(String),

    /// The upstream server returned an error response.
    #[error("upstream returned error: {0}")]
    UpstreamError(String),

    /// Tool description changed (rug-pull detection).
    #[error("tool description hash changed for {tool}: expected {expected}, got {got}")]
    DescriptionHashChanged {
        tool: String,
        expected: String,
        got: String,
    },

    /// The server was not found in the federation.
    #[error("unknown server: {0}")]
    UnknownServer(String),
}

impl McpError {
    /// Convert to a JSON-RPC error for wire transmission.
    pub fn to_jsonrpc_error(&self) -> JsonRpcError {
        match self {
            McpError::JsonRpc(e) => e.clone(),
            McpError::UnknownTool(name) => {
                JsonRpcError::invalid_params(format!("unknown tool: {name}"))
            }
            McpError::ToolNotAllowed(name) => {
                JsonRpcError::new(-32000, format!("tool not allowed: {name}"))
            }
            McpError::DescriptionHashChanged { .. } => JsonRpcError::new(-32001, self.to_string()),
            _ => JsonRpcError::internal(self.to_string()),
        }
    }
}
