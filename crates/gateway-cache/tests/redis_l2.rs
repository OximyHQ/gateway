//! Optional L2 integration. Compiled only with `--features redis-l2` and `#[ignore]`d
//! so CI without a Redis stays green. Run locally with:
//!   REDIS_URL=redis://127.0.0.1/ cargo test -p gateway-cache --features redis-l2 --test redis_l2 -- --ignored

#![cfg(feature = "redis-l2")]

use std::sync::Arc;

use gateway_cache::entry::{CachedBody, CachedResponse};
use gateway_cache::{CacheStore, MemoryStore, RedisStore, TieredStore};
use gateway_llm::{ChatResponse, FinishReason};
use gateway_spine::{TokenUsage, Usd};

fn entry() -> CachedResponse {
    CachedResponse {
        body: CachedBody::Unary(ChatResponse {
            model: "m".into(),
            content: vec![],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage::default(),
            provider_response_id: None,
        }),
        usage: TokenUsage::default(),
        original_cost: Usd::ZERO,
        stored_at_ms: 0,
        expires_at_ms: None,
    }
}

#[tokio::test]
#[ignore = "requires a live Redis at REDIS_URL"]
async fn redis_l2_promotes_into_l1() {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let l2 = Arc::new(RedisStore::connect(&url).await.unwrap());
    l2.clear().await.unwrap();
    l2.put("k", entry()).await.unwrap();

    let l1: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
    let tier = TieredStore::tiered(Arc::clone(&l1), l2);
    // L1 miss → L2 hit → promote.
    assert!(tier.get("k", 0).await.unwrap().is_some());
    assert!(l1.get("k", 0).await.unwrap().is_some());
}
