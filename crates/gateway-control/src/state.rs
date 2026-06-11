//! The shared service container injected into every handler via Axum `State`.
//! It owns the spine governance components (registry, ledger, limiter, key
//! store, audit, clock) and the egress providers + guard seam. Everything is
//! behind `Arc`/interior-mutability so the whole state is cheap to clone per
//! request and safe to share across the Tokio pool. P1.6 swaps the in-memory
//! stores for persistent ones behind the same field types (trait objects).

use std::sync::{Arc, RwLock};

use gateway_spine::{
    AuditSink, BudgetLedger, Clock, MemoryAudit, ModelRegistry, RateLimiter, SystemClock,
};

use crate::guard::{AllowAll, GuardHook};
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
    pub guard: Arc<dyn GuardHook>,
    pub audit: Arc<dyn AuditSink>,
    pub clock: Arc<C>,
}

impl AppState<SystemClock> {
    /// Production constructor: a system clock, empty registry/providers to be
    /// populated by the binary (P1.8) or a config load (P1.6).
    pub fn new(keys: Arc<dyn KeyStore>) -> Self {
        Self::with_parts(
            keys,
            Arc::new(SystemClock),
            ProviderRegistry::new(),
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        )
    }
}

impl<C: Clock> AppState<C> {
    /// Full constructor used by tests (injects a `MockClock` + a `MockProvider`
    /// registry + a seeded key store).
    pub fn with_parts(
        keys: Arc<dyn KeyStore>,
        clock: Arc<C>,
        providers: ProviderRegistry,
        guard: Arc<dyn GuardHook>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        // Arc<C>: Clock via the blanket impl in gateway-spine, so RateLimiter<Arc<C>> works.
        let limiter = Arc::new(RateLimiter::new(clock.clone()));
        Self {
            registry: RwLock::new(ModelRegistry::new()),
            ledger: Arc::new(BudgetLedger::new()),
            limiter,
            keys,
            providers,
            guard,
            audit,
            clock,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use gateway_spine::MockClock;

    #[test]
    fn builds_with_a_static_keystore() {
        let mut ks = StaticKeyStore::new();
        ks.bootstrap("sk-x", None);
        let clock = Arc::new(MockClock::new(0));
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        );
        assert!(state.keys.resolve("sk-x").is_some());
        assert!(state.registry.read().unwrap().is_empty());
    }
}
