//! A Clock-erased facade over `CacheLayer` for use in the generic `AppState<C>`.
//! `CacheLayer<C>` is parameterized on a clock, which conflicts with the
//! `AppState<C>` generic parameter in tests (which use `MockClock`).  Instead of
//! storing `CacheLayer<C>` directly in `AppState`, the production binary wraps it
//! in a `CacheHandleImpl<SystemClock>` and stores it as `Arc<dyn CacheHandle>`.
//! Tests that don't need caching leave the field as `None`.

use std::sync::Arc;

use async_trait::async_trait;
use gateway_cache::{CacheControl, CacheOutcome, CacheStatus, MemoryStore};
use gateway_llm::ChatResponse;
use gateway_spine::{Clock, TokenUsage, Usd};

/// Arguments for a cache write. Bundled into a struct so the trait stays below
/// clippy's 7-argument limit.
pub struct StoreArgs<'a> {
    pub tenant_id: &'a str,
    pub endpoint: &'a str,
    pub model: &'a str,
    pub body: &'a serde_json::Value,
    pub ctl: &'a CacheControl,
    pub response: ChatResponse,
    pub usage: TokenUsage,
    pub original_cost: Usd,
}

/// The cache seam exposed to the request lifecycle.
#[async_trait]
pub trait CacheHandle: Send + Sync {
    async fn lookup(
        &self,
        tenant_id: &str,
        endpoint: &str,
        model: &str,
        body: &serde_json::Value,
        ctl: &CacheControl,
    ) -> CacheOutcome;

    async fn store_unary(&self, args: StoreArgs<'_>);
}

/// Concrete impl wrapping a `CacheLayer<C>`.
pub struct CacheHandleImpl<C: Clock> {
    pub layer: gateway_cache::CacheLayer<C>,
}

#[async_trait]
impl<C: Clock + Send + Sync + 'static> CacheHandle for CacheHandleImpl<C> {
    async fn lookup(
        &self,
        tenant_id: &str,
        endpoint: &str,
        model: &str,
        body: &serde_json::Value,
        ctl: &CacheControl,
    ) -> CacheOutcome {
        self.layer
            .lookup(tenant_id, endpoint, model, body, ctl)
            .await
    }

    async fn store_unary(&self, args: StoreArgs<'_>) {
        self.layer
            .store_unary(
                args.tenant_id,
                args.endpoint,
                args.model,
                args.body,
                args.ctl,
                args.response,
                args.usage,
                args.original_cost,
            )
            .await;
    }
}

/// Convenience constructor: build an L1-only memory-backed cache handle.
pub fn memory_cache_handle<C: Clock + Send + Sync + 'static>(
    clock: C,
    default_ttl_secs: i64,
) -> Arc<dyn CacheHandle> {
    Arc::new(CacheHandleImpl {
        layer: gateway_cache::CacheLayer::new(
            Arc::new(MemoryStore::new()),
            clock,
            default_ttl_secs,
        ),
    })
}

/// Parse `x-oximy-cache` request header into a `CacheControl`. Returns default
/// (no directives) if the header is absent.
pub fn parse_cache_control(headers: &axum::http::HeaderMap) -> CacheControl {
    headers
        .get("x-oximy-cache")
        .and_then(|v| v.to_str().ok())
        .map(CacheControl::from_header)
        .unwrap_or_default()
}

/// Build the `x-cache` header value from an outcome status.
pub fn x_cache_header(status: CacheStatus) -> &'static str {
    match status {
        CacheStatus::Hit => "HIT",
        CacheStatus::Miss => "MISS",
        CacheStatus::Bypass => "BYPASS",
    }
}
