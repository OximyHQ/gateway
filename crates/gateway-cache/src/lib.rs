//! # gateway-cache
//!
//! SHA-256 exact cache (tenant-scoped, 200s only), semantic similarity cache, and provider cache_control passthrough with correct cached-token accounting.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! Status: **scaffold**. Implementation tracked by the Phase plans under `docs/plans/`.

#![forbid(unsafe_code)]

/// Placeholder so the crate compiles in the workspace before implementation lands.
pub const CRATE: &str = "gateway-cache";
