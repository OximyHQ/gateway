//! # gateway-dash
//!
//! A thin client of the REST API with no capability the API lacks. Compiled into
//! the binary so a single command boots the gateway and opens the dashboard.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

pub mod embed;
pub mod router;

pub use embed::{asset_count, index_present};
pub use router::dash_router;
