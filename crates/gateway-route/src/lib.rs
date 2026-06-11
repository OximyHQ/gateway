//! # gateway-route
//!
//! Picks deployments per request: failover, weighted random, latency-aware,
//! retry+backoff, per-target cooldown/circuit-breaker, and LLM-aware request
//! hedging (fire a backup target after a delay if the primary is slow; take
//! whichever completes first — fallback only before first token).
//!
//! **Design invariants (from §2 / §6 of the design doc):**
//! - Fallback fires only before the first token — never mid-stream.
//! - The same idempotency key is reused across all retries/failovers of one
//!   logical request so the spine bills once (no-double-billing).
//! - Terminal errors (Auth, 4xx, Unsupported) do NOT retry; only retryable
//!   errors (RateLimited, Transport, upstream 5xx) do.
//! - Injected `Clock` keeps all timing testable without sleeping.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

pub mod error;
pub mod executor;
pub mod route;
pub mod router;
pub mod strategy;

pub use error::RouteError;
pub use executor::TargetExecutor;
pub use route::{Route, RouteTarget, Strategy};
pub use router::{Router, RouterMeta, RouterResult};
