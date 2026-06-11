//! # gateway-llm
//!
//! Normalizes OpenAI / Anthropic /v1/messages / Gemini / Responses ingress to a unified shape; dispatches to ~30 provider transports; conformance-tested translation with a per-pair fidelity matrix.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.
//!
//! Status: **scaffold**. Implementation tracked by the Phase plans under `docs/plans/`.

#![forbid(unsafe_code)]

/// Placeholder so the crate compiles in the workspace before implementation lands.
pub const CRATE: &str = "gateway-llm";
