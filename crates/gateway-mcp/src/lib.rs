//! # gateway-mcp
//!
//! Federates N MCP servers behind one endpoint; stdio/SSE/streamable-HTTP bridging; stateless-first with a compat shim; per-key tool ACLs and dollar-metered tool calls on the shared spine.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! Status: **scaffold**. Implementation tracked by the Phase plans under `docs/plans/`.

#![forbid(unsafe_code)]

/// Placeholder so the crate compiles in the workspace before implementation lands.
pub const CRATE: &str = "gateway-mcp";
