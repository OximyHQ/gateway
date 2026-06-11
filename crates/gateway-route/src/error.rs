//! Error types for the routing engine.

use gateway_llm::ProviderError;

/// Top-level error returned by [`crate::Router::call`].
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    /// The route has no targets configured.
    #[error("route has no targets")]
    NoTargets,

    /// Every target was tried and exhausted (all failed or in cooldown).
    #[error("all targets exhausted after {attempts} attempt(s)")]
    AllTargetsExhausted { attempts: u32 },

    /// A terminal (non-retryable) provider error was encountered.
    /// The router stops immediately without trying further targets.
    #[error("terminal provider error: {0}")]
    TerminalError(#[source] ProviderError),

    /// Internal logic error (should not happen in normal operation).
    #[error("internal router error: {0}")]
    Internal(String),
}
