//! Two-tier composition implementing `CacheStore` itself, so `CacheLayer` is
//! tier-agnostic. Read: L1 first; on L1 miss, consult L2 and promote a hit back
//! into L1. Write: write-through to BOTH. The L2 is an `Option<Arc<dyn CacheStore>>`
//! — when `None`, this is just L1, which is the default deployment (Redis optional,
//! never required). `RedisStore` (the typical L2) lives behind the `redis-l2`
//! feature; this composition needs no knowledge of it.

use std::sync::Arc;

use async_trait::async_trait;

use crate::entry::CachedResponse;
use crate::error::CacheError;
use crate::store::CacheStore;

pub struct TieredStore {
    l1: Arc<dyn CacheStore>,
    l2: Option<Arc<dyn CacheStore>>,
}

impl TieredStore {
    /// L1-only (no Redis). The default.
    pub fn l1_only(l1: Arc<dyn CacheStore>) -> Self {
        Self { l1, l2: None }
    }

    /// L1 over an L2 (e.g. Redis).
    pub fn tiered(l1: Arc<dyn CacheStore>, l2: Arc<dyn CacheStore>) -> Self {
        Self { l1, l2: Some(l2) }
    }
}

#[async_trait]
impl CacheStore for TieredStore {
    async fn get(&self, key: &str, now_ms: i64) -> Result<Option<CachedResponse>, CacheError> {
        if let Some(hit) = self.l1.get(key, now_ms).await? {
            return Ok(Some(hit));
        }
        if let Some(l2) = &self.l2
            && let Some(hit) = l2.get(key, now_ms).await?
        {
            // Promote into L1 for the next read.
            let _ = self.l1.put(key, hit.clone()).await;
            return Ok(Some(hit));
        }
        Ok(None)
    }

    async fn put(&self, key: &str, value: CachedResponse) -> Result<(), CacheError> {
        self.l1.put(key, value.clone()).await?;
        if let Some(l2) = &self.l2 {
            let _ = l2.put(key, value).await; // L2 write is best-effort
        }
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        self.l1.delete(key).await?;
        if let Some(l2) = &self.l2 {
            let _ = l2.delete(key).await;
        }
        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        self.l1.clear().await?;
        if let Some(l2) = &self.l2 {
            let _ = l2.clear().await;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), CacheError> {
        self.l1.ping().await?;
        if let Some(l2) = &self.l2 {
            l2.ping().await?;
        }
        Ok(())
    }

    async fn len(&self, now_ms: i64) -> Result<usize, CacheError> {
        // L1 is the authoritative count for analytics (L2 may be shared/larger).
        self.l1.len(now_ms).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::CachedBody;
    use crate::memory::MemoryStore;
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
    async fn l1_only_behaves_like_l1() {
        let l1 = Arc::new(MemoryStore::new());
        let t = TieredStore::l1_only(l1);
        t.put("k", entry()).await.unwrap();
        assert!(t.get("k", 0).await.unwrap().is_some());
        assert!(t.ping().await.is_ok());
    }

    #[tokio::test]
    async fn l2_hit_is_promoted_into_l1() {
        let l1: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        let l2: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        // Seed ONLY L2.
        l2.put("k", entry()).await.unwrap();
        let t = TieredStore::tiered(Arc::clone(&l1), Arc::clone(&l2));
        // First read: L1 miss → L2 hit → promote.
        assert!(t.get("k", 0).await.unwrap().is_some());
        // Now L1 holds it directly.
        assert!(l1.get("k", 0).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn write_through_to_both() {
        let l1: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        let l2: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        let t = TieredStore::tiered(Arc::clone(&l1), Arc::clone(&l2));
        t.put("k", entry()).await.unwrap();
        assert!(l1.get("k", 0).await.unwrap().is_some());
        assert!(l2.get("k", 0).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn clear_clears_both() {
        let l1: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        let l2: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
        let t = TieredStore::tiered(Arc::clone(&l1), Arc::clone(&l2));
        t.put("k", entry()).await.unwrap();
        t.clear().await.unwrap();
        assert!(l1.get("k", 0).await.unwrap().is_none());
        assert!(l2.get("k", 0).await.unwrap().is_none());
    }
}
