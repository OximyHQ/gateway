//! The shared service container injected into every handler via Axum `State`.
//! It owns the spine governance components (registry, ledger, limiter, key
//! store, audit, clock) and the egress providers + guard seam. Everything is
//! behind `Arc`/interior-mutability so the whole state is cheap to clone per
//! request and safe to share across the Tokio pool.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gateway_guard::GuardChain;
use gateway_mcp::Federation;
use gateway_route::Route;
use gateway_spine::{
    AuditSink, BudgetLedger, Clock, MemoryAudit, ModelRegistry, RateLimiter, SystemClock,
};
use gateway_store::Store;
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
    /// Kept for backward compatibility with tests that assert on ledger directly.
    /// In production, durable budget tracking flows through `store`.
    pub ledger: Arc<BudgetLedger>,
    pub limiter: Arc<RateLimiter<Arc<C>>>,
    pub keys: Arc<dyn KeyStore>,
    pub providers: ProviderRegistry,
    /// The content-guard chain run at `PreRequest` (over the prompt) and
    /// `PostResponse` (over the completion). Blocks secrets, masks PII by default.
    pub guard: Arc<GuardChain>,
    /// Optional per-model route overrides: model id → ordered targets + strategy.
    pub routes: RwLock<HashMap<String, Route>>,
    pub audit: Arc<dyn AuditSink>,
    /// The MCP federation behind the authenticated `POST /mcp` endpoint.
    pub federation: Arc<Federation>,
    pub clock: Arc<C>,
    /// Non-blocking telemetry sink — `try_send` only, never blocks a request.
    pub telemetry: TelemetrySink,
    /// Live Prometheus metrics, rendered by the authenticated `/metrics` handler.
    pub metrics: Arc<GatewayMetrics>,
    /// Optional L1/L2 response cache.
    pub cache: Option<Arc<dyn CacheHandle>>,
    /// The live spend store shared between the telemetry writer and the admin
    /// endpoints.
    pub spend_store: Arc<dyn SpendStore>,
    /// Durable SQLite/Postgres-backed store: keys + spend ledger. Single source
    /// of truth for budget tracking and key persistence — survives restarts.
    pub store: Arc<Store>,
}

impl AppState<SystemClock> {
    /// Production constructor: a system clock, empty registry/providers to be
    /// populated by the binary or a config load.
    pub fn new(keys: Arc<dyn KeyStore>, store: Arc<Store>) -> Self {
        Self::with_parts(
            keys,
            Arc::new(SystemClock),
            ProviderRegistry::new(),
            Arc::new(default_chain()),
            Arc::new(MemoryAudit::new()),
            store,
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
        store: Arc<Store>,
    ) -> Self {
        let metrics = Arc::new(GatewayMetrics::new());
        let spend_store = Arc::new(MemorySpendStore::new());
        let (telemetry, _writer) = spawn(
            Arc::clone(&spend_store),
            Arc::clone(&metrics),
            gateway_telemetry::DEFAULT_CHANNEL_CAPACITY,
        );
        Self::with_parts_and_telemetry(
            keys,
            clock,
            providers,
            guard,
            audit,
            telemetry,
            metrics,
            spend_store,
            store,
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
        store: Arc<Store>,
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
            store,
        }
    }

    /// Install (or replace) the route for a model id.
    pub fn set_route(&self, model: impl Into<String>, route: Route) {
        self.routes.write().unwrap().insert(model.into(), route);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use gateway_spine::MockClock;

    async fn mem_store() -> Arc<Store> {
        Arc::new(Store::connect("sqlite::memory:").await.unwrap())
    }

    #[tokio::test]
    async fn builds_with_a_static_keystore() {
        let mut ks = StaticKeyStore::new();
        ks.bootstrap("sk-x", None);
        let clock = Arc::new(MockClock::new(0));
        let store = mem_store().await;
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
            store,
        );
        assert!(state.keys.resolve("sk-x").is_some());
        assert!(state.registry.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cache_is_none_by_default() {
        let ks = StaticKeyStore::new();
        let clock = Arc::new(MockClock::new(0));
        let store = mem_store().await;
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
            store,
        );
        assert!(state.cache.is_none());
    }

    #[tokio::test]
    async fn spend_store_is_accessible() {
        let ks = StaticKeyStore::new();
        let clock = Arc::new(MockClock::new(0));
        let store = mem_store().await;
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            Arc::new(MemoryAudit::new()),
            store,
        );
        // No rows yet; just check it's wired.
        assert_eq!(state.spend_store.row_count(), 0);
    }
}
