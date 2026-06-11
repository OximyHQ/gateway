//! # gateway-config
//!
//! The single declarative projection of gateway state. UI = API = CLI = MCP = Git, all through one diff/apply engine. Kills the yaml-vs-DB split brain.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! Status: **scaffold**. Implementation tracked by the Phase plans under `docs/plans/`.

#![forbid(unsafe_code)]

/// Placeholder so the crate compiles in the workspace before implementation lands.
pub const CRATE: &str = "gateway-config";
