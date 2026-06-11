//! # gateway-mcp
//!
//! MCP (Model Context Protocol) gateway plane for Oximy Gateway.
//!
//! Federates N upstream MCP servers behind one endpoint; tools are namespaced
//! (`server__tool`); per-key tool ACLs enforced; every tool call emits an
//! audit event on the shared spine; tool description hashes guard against
//! rug-pull attacks.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! ## Status
//! First-cut implementation.  See individual modules for deferral notes.
//!
//! ## Deferred (clearly noted per module)
//! - OAuth 2.1 inbound/outbound brokering
//! - Semantic tool search (`find_tool` / `call_tool` discovery)
//! - Stateless-RC session shim (2026-07-28 protocol)
//! - Streamable-HTTP SSE server side (inbound GET listener)
//! - MCP dollar-metering against the spine budget (P2 integration)
//! - Integration with `gateway-control` / other crates

#![forbid(unsafe_code)]
#![deny(clippy::collapsible_if)]

/// The MCP protocol version this gateway speaks (current stable).
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

pub mod acl;
pub mod error;
pub mod federation;
pub mod hash;
pub mod jsonrpc;
pub mod registry;
pub mod server;
pub mod transport;
pub mod types;

// Re-export the most-used types at the crate root for convenience.
pub use acl::ToolAcl;
pub use error::McpError;
pub use federation::Federation;
pub use hash::description_hash;
pub use jsonrpc::{
    JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId, RpcFrame,
};
pub use registry::{ToolEntry, ToolRegistry};
pub use server::McpServer;
pub use transport::{HttpTransport, McpTransport, StdioTransport};
pub use types::{
    ClientCapabilities, Implementation, InitializeParams, InitializeResult, ServerCapabilities,
    Tool, ToolAnnotations, ToolCallParams, ToolCallResult, ToolContent, ToolsListParams,
    ToolsListResult,
};
