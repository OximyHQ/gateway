//! The `Guardrail` trait — the single extension point every rule must implement.

use async_trait::async_trait;

use crate::types::{GuardContext, GuardVerdict};

/// A single content-policy rule.
///
/// Guardrails are cheap to clone (typically behind an `Arc`) and are always
/// evaluated asynchronously so that webhook or async I/O implementations fit
/// naturally.
#[async_trait]
pub trait Guardrail: Send + Sync {
    /// A stable, human-readable identifier for this guardrail (used in
    /// [`crate::chain::ChainVerdict::per_guardrail`] records and logs).
    fn name(&self) -> &str;

    /// Inspect `ctx` and return a [`GuardVerdict`].
    ///
    /// Implementations **must not** panic; return [`GuardVerdict::Allow`] on
    /// unexpected internal errors and log the problem instead.
    async fn check(&self, ctx: &GuardContext) -> GuardVerdict;
}
