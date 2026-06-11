//! In-process L1 store. A `Mutex<HashMap>` keyed by the hex CacheKey string;
//! TTL is enforced lazily at read time (an expired entry returns `None` and is
//! evicted). Eviction of dead keys also happens on `len`. This is the default
//! store and the only one required to exist.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::entry::CachedResponse;
use crate::error::CacheError;
use crate::store::CacheStore;

#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, CachedResponse>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CacheStore for MemoryStore {
    async fn get(&self, key: &str, now_ms: i64) -> Result<Option<CachedResponse>, CacheError> {
        let mut g = self.map.lock().unwrap();
        if let Some(entry) = g.get(key) {
            if entry.is_expired(now_ms) {
                g.remove(key);
                return Ok(None);
            }
            return Ok(Some(entry.clone()));
        }
        Ok(None)
    }

    async fn put(&self, key: &str, value: CachedResponse) -> Result<(), CacheError> {
        self.map.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        self.map.lock().unwrap().clear();
        Ok(())
    }

    async fn ping(&self) -> Result<(), CacheError> {
        Ok(())
    }

    async fn len(&self, now_ms: i64) -> Result<usize, CacheError> {
        let mut g = self.map.lock().unwrap();
        g.retain(|_, v| !v.is_expired(now_ms));
        Ok(g.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::CachedBody;
    use gateway_llm::{ChatResponse, FinishReason};
    use gateway_spine::{TokenUsage, Usd};

    fn entry(stored_at: i64, expires_at: Option<i64>) -> CachedResponse {
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
            stored_at_ms: stored_at,
            expires_at_ms: expires_at,
        }
    }

    #[tokio::test]
    async fn put_then_get_hits() {
        let s = MemoryStore::new();
        s.put("k", entry(0, Some(1000))).await.unwrap();
        assert!(s.get("k", 500).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn missing_key_is_none() {
        let s = MemoryStore::new();
        assert!(s.get("nope", 0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_entry_is_evicted_on_read() {
        let s = MemoryStore::new();
        s.put("k", entry(0, Some(1000))).await.unwrap();
        assert!(s.get("k", 1000).await.unwrap().is_none()); // expired
        assert_eq!(s.len(2000).await.unwrap(), 0); // and gone
    }

    #[tokio::test]
    async fn delete_and_clear() {
        let s = MemoryStore::new();
        s.put("a", entry(0, None)).await.unwrap();
        s.put("b", entry(0, None)).await.unwrap();
        s.delete("a").await.unwrap();
        assert!(s.get("a", 0).await.unwrap().is_none());
        assert_eq!(s.len(0).await.unwrap(), 1);
        s.clear().await.unwrap();
        assert_eq!(s.len(0).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn ping_is_ok() {
        assert!(MemoryStore::new().ping().await.is_ok());
    }

    #[tokio::test]
    async fn len_excludes_expired() {
        let s = MemoryStore::new();
        s.put("live", entry(0, Some(10_000))).await.unwrap();
        s.put("dead", entry(0, Some(100))).await.unwrap();
        assert_eq!(s.len(5_000).await.unwrap(), 1);
    }
}
