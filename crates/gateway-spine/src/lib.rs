//! # gateway-spine
//!
//! The protocol-agnostic core every request flows through — tokens in, dollars out, policy everywhere. Owns the non-negotiable invariants: fail-closed budgets, no double-billing, no overspend under concurrency, auth-by-default, cost-correctness.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! Status: **scaffold**. Implementation tracked by the Phase plans under `docs/plans/`.

#![forbid(unsafe_code)]

/// Placeholder so the crate compiles in the workspace before implementation lands.
pub const CRATE: &str = "gateway-spine";
