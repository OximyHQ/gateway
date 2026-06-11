//! # gateway-spine
//!
//! The protocol-agnostic core every request flows through — tokens in, dollars
//! out, policy everywhere. Owns the non-negotiable invariants: fail-closed
//! budgets, no double-billing, no overspend under concurrency, auth-by-default,
//! cost-correctness.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod audit;
pub mod budget;
pub mod clock;
pub mod error;
pub mod key;
pub mod money;
pub mod pricing;
pub mod ratelimit;
pub mod registry;
pub mod usage;

pub use audit::{AuditEvent, AuditSink, MemoryAudit};
pub use budget::{BudgetLedger, ReservationId};
pub use clock::{Clock, MockClock, SystemClock};
pub use error::{RateDimension, SpineError};
pub use key::{RateLimits, VirtualKey};
pub use money::Usd;
pub use pricing::ModelPrice;
pub use ratelimit::RateLimiter;
pub use registry::{ModelEntry, ModelRegistry};
pub use usage::TokenUsage;
