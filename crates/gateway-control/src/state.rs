//! The shared service container injected into every handler via Axum `State`.
//! It owns the spine governance components (registry, ledger, limiter, key
//! store, audit, clock) and the egress providers + guard seam. Everything is
//! behind `Arc`/interior-mutability so the whole state is cheap to clone per
//! request and safe to share across the Tokio pool. P1.6 swaps the in-memory
//! stores for persistent ones behind the same field types (trait objects).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gateway_guard::GuardChain;
use gateway_mcp::Federation;
use gateway_route::Route;
use gateway_spine::{
    AuditSink, BudgetLedger, Clock, MemoryAudit, ModelRegistry, RateLimiter, SystemClock,
};
use gateway_telemetry::{GatewayMetrics, MemorySpendStore, SpendStore, TelemetrySink, spawn};

// Re-export so callers that need to build a sink don't depend on gateway-telemetry directly.
pub use gateway_telemetry::{DEFAULT_CHANNEL_CAPACITY, TelemetryWriter};

use crate::cache_handle::CacheHandle;
use crate::guard::default_chain;
use crate::keystore::KeyStore;
use crate::providers::ProviderRegistry;

/// Concrete clock type the rate limiter is parameterized on for the server.
/// `Arc<C>: Clock` (from the spine blanket impl), so `RateLimiter<Arc<C>>`
/// shares the same clock instance as `AppState.clock`.
pub struct AppState<C: Clock = SystemClock> {
    pub registry: RwLock<ModelRegistry>,
    pub ledger: Arc<BudgetLedger>,
    pub limiter: Arc<RateLimiter<Arc<C>>>,
    pub keys: Arc<dyn KeyStore>,
    pub providers: ProviderRegistry,
    /// The content-guard chain run at `PreRequest` (over the prompt) and
    /// `PostResponse` (over the completion). Blocks secrets, masks PII by default.
    pub guard: Arc<GuardChain>,
    /// Optional per-model route overrides: model id → ordered targets + strategy.
    /// A model with NO entry routes as a single target to its registry provider
    /// (behaviour unchanged from the pre-routing path). Populated by config; see
    /// [`AppState::set_route`].
    pub routes: RwLock<HashMap<String, Route>>,
    pub audit: Arc<dyn AuditSink>,
    /// The MCP federation behind the authenticated `POST /mcp` endpoint. Shares
    /// the same `audit` sink so tool-call events land on the one spine. Built
    /// empty (zero upstream servers) — the binary registers servers from config.
    pub federation: Arc<Federation>,
    pub clock: Arc<C>,
    /// Non-blocking telemetry sink — `try_send` only, never blocks a request.
    pub telemetry: TelemetrySink,
    /// Live Prometheus metrics, rendered by the authenticated `/metrics` handler.
    pub metrics: Arc<GatewayMetrics>,
    /// Optional L1/L2 response cache. Clock-erased so it can be stored in a
    /// `C`-generic struct without constraining `C`. `None` → all requests are
    /// cache-bypassed (the default for tests that don't need caching).
    pub cache: Option<Arc<dyn CacheHandle>>,
    /// The live spend store shared between the telemetry writer and the admin
    /// endpoints. The telemetry writer appends rows; the admin endpoints read.
    /// `Arc<dyn SpendStore>` lets tests inject a `MemorySpendStore`.
    pub spend_store: Arc<dyn SpendStore>,
}

impl AppState<SystemClock> {
    /// Production constructor: a system clock, empty registry/providers to be
    /// populated by the binary (P1.8) or a config load (P1.6).
    pub fn new(keys: Arc<dyn KeyStore>) -> Self {
        Self::with_parts(
            keys,
            Arc::new(SystemClock),
            ProviderRegistry::new(),
            Arc::new(default_chain()),
            Arc::new(MemoryAudit::new()),
        )
    }
}

impl<C: Clock> AppState<C> {
    /// Full constructor used by tests and `with_parts_and_telemetry`. Spawns a
    /// default in-memory telemetry writer.
    pub fn with_parts(
        keys: Arc<dyn KeyStore>,
        clock: Arc<C>,
        providers: ProviderRegistry,
        guard: Arc<GuardChain>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        let metrics = Arc::new(GatewayMetrics::new());
        let store = Arc::new(MemorySpendStore::new());
        let (telemetry, _writer) = spawn(
            Arc::clone(&store),
            Arc::clone(&metrics),
            gateway_telemetry::DEFAULT_CHANNEL_CAPACITY,
        );
        Self::with_parts_and_telemetry(
            keys, clock, providers, guard, audit, telemetry, metrics, store,
        )
    }

    /// Full constructor with explicit telemetry injection. Used by the binary
    /// (which pre-builds the metrics + sink) and by integration tests that
    /// assert on `/metrics` content.
    #[allow(clippy::too_many_arguments)]
    pub fn with_parts_and_telemetry(
        keys: Arc<dyn KeyStore>,
        clock: Arc<C>,
        providers: ProviderRegistry,
        guard: Arc<GuardChain>,
        audit: Arc<dyn AuditSink>,
        telemetry: TelemetrySink,
        metrics: Arc<GatewayMetrics>,
        spend_store: Arc<dyn SpendStore>,
    ) -> Self {
        // Arc<C>: Clock via the blanket impl in gateway-spine, so RateLimiter<Arc<C>> works.
        let limiter = Arc::new(RateLimiter::new(clock.clone()));
        // The federation shares the same audit sink so MCP tool calls and LLM
        // requests audit onto one spine.
        let federation = Arc::new(Federation::new(Arc::clone(&audit)));
        Self {
            registry: RwLock::new(ModelRegistry::new()),
            ledger: Arc::new(BudgetLedger::new()),
            limiter,
            keys,
            providers,
            guard,
            routes: RwLock::new(HashMap::new()),
            audit,
            federation,
            clock,
            telemetry,
            metrics,
            cache: None,
            spend_store,
        }
    }

    /// Install (or replace) the route for a model id. A configured route lets a
    /// model fail over / load-balance across multiple (provider, model) targets.
    pub fn set_route(&self, model: impl Into<String>, route: Route) {
        self.routes.write().unwrap().insert(model.into(), route);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use gateway_spine::MockClock;

    #[tokio::test]
    async fn builds_with_a_static_keystore() {
        let mut ks = StaticKeyStore::new();
        ks.bootstrap("sk-x", None);
        let clock = Arc::new(MockClock::new(0));
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        assert!(state.keys.resolve("sk-x").is_some());
        assert!(state.registry.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cache_is_none_by_default() {
        let ks = StaticKeyStore::new();
        let clock = Arc::new(MockClock::new(0));
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        assert!(state.cache.is_none());
    }

    #[tokio::test]
    async fn spend_store_is_accessible() {
        let ks = StaticKeyStore::new();
        let clock = Arc::new(MockClock::new(0));
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        // No rows yet; just check it's wired.
        assert_eq!(state.spend_store.row_count(), 0);
    }
}
