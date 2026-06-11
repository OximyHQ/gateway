//! Route configuration types: `RouteTarget`, `Strategy`, and `Route`.

use serde::{Deserialize, Serialize};

/// One upstream target in a route: a (provider_id, model) pair with an
/// optional weight used by the `Weighted` strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteTarget {
    /// Stable provider id (e.g. "openai", "anthropic"). Resolved against the
    /// `ProviderRegistry` at call time by the `TargetExecutor`.
    pub provider_id: String,
    /// Registry model id to send to this provider.
    pub model: String,
    /// Relative weight for `Strategy::Weighted`. Ignored by other strategies.
    /// If absent, defaults to 1.0. Negative weights are treated as 0.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

impl RouteTarget {
    pub fn new(provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        RouteTarget {
            provider_id: provider_id.into(),
            model: model.into(),
            weight: None,
        }
    }

    pub fn with_weight(mut self, w: f64) -> Self {
        self.weight = Some(w);
        self
    }

    /// Effective weight (clamps negative values to 0.0, defaults to 1.0).
    pub fn effective_weight(&self) -> f64 {
        self.weight.unwrap_or(1.0).max(0.0)
    }
}

/// How the router chooses among targets and handles failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// Call the first target; retry on retryable errors without moving to the
    /// next target. Only one target is used.
    #[default]
    Single,
    /// Try targets in order; advance to the next on retryable errors.
    Failover,
    /// Weighted random selection (uses `RouteTarget::weight`); retry on
    /// retryable errors (re-selecting a target each time).
    Weighted,
    /// Track per-target EWMA latency; always pick the target with the lowest
    /// estimated latency. Degrades gracefully to `Failover` order for new
    /// targets with no history.
    LatencyAware,
}

/// A fully configured route: a set of targets + a dispatch strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Ordered list of targets. For `Failover`, order matters (primary first).
    /// For `Weighted`/`LatencyAware`, all are candidates.
    pub targets: Vec<RouteTarget>,
    pub strategy: Strategy,
    /// Maximum total attempts across ALL targets before giving up.
    /// Default: 3.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Base retry delay in milliseconds (exponential backoff doubles this).
    /// Default: 100 ms.
    #[serde(default = "default_base_backoff_ms")]
    pub base_backoff_ms: u64,
    /// Maximum retry delay in milliseconds (caps exponential growth).
    /// Default: 10_000 ms.
    #[serde(default = "default_max_backoff_ms")]
    pub max_backoff_ms: u64,
    /// After this many consecutive failures a target enters cooldown.
    /// Default: 3.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    /// Cooldown window in milliseconds. After this window the target is
    /// considered healthy again. Default: 30_000 ms (30 s).
    #[serde(default = "default_cooldown_ms")]
    pub cooldown_ms: u64,
    /// Hedging: fire a backup call after this many milliseconds if the primary
    /// hasn't responded. `None` disables hedging. Only active for non-streaming
    /// requests (fallback only before first token).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hedge_after_ms: Option<u64>,
}

fn default_max_attempts() -> u32 {
    3
}
fn default_base_backoff_ms() -> u64 {
    100
}
fn default_max_backoff_ms() -> u64 {
    10_000
}
fn default_failure_threshold() -> u32 {
    3
}
fn default_cooldown_ms() -> u64 {
    30_000
}

impl Route {
    pub fn new(targets: Vec<RouteTarget>, strategy: Strategy) -> Self {
        Route {
            targets,
            strategy,
            max_attempts: default_max_attempts(),
            base_backoff_ms: default_base_backoff_ms(),
            max_backoff_ms: default_max_backoff_ms(),
            failure_threshold: default_failure_threshold(),
            cooldown_ms: default_cooldown_ms(),
            hedge_after_ms: None,
        }
    }

    pub fn single(provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        Route::new(vec![RouteTarget::new(provider_id, model)], Strategy::Single)
    }

    pub fn with_hedge(mut self, after_ms: u64) -> Self {
        self.hedge_after_ms = Some(after_ms);
        self
    }
}
