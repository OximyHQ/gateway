//! Optional Redis L2. Compiled ONLY under `--features redis-l2`. Entries are
//! JSON-serialized `CachedResponse`s under the hex CacheKey; TTL is enforced both
//! at the application layer (`is_expired`) AND as a Redis key TTL via PEXPIREAT so
//! Redis evicts dead entries on its own. This file is feature-gated so a default
//! build neither links nor requires Redis.

use async_trait::async_trait;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;

use crate::entry::CachedResponse;
use crate::error::CacheError;
use crate::store::CacheStore;

const PREFIX: &str = "oximy:cache:";

pub struct RedisStore {
    conn: ConnectionManager,
}

impl RedisStore {
    /// Connect with a Redis URL (e.g. `redis://127.0.0.1/`).
    pub async fn connect(url: &str) -> Result<Self, CacheError> {
        let client = redis::Client::open(url).map_err(|e| CacheError::Backend(e.to_string()))?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(Self { conn })
    }

    fn k(key: &str) -> String {
        format!("{PREFIX}{key}")
    }
}

#[async_trait]
impl CacheStore for RedisStore {
    async fn get(&self, key: &str, now_ms: i64) -> Result<Option<CachedResponse>, CacheError> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .get(Self::k(key))
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        match raw {
            Some(s) => {
                let entry: CachedResponse = serde_json::from_str(&s)?;
                if entry.is_expired(now_ms) {
                    let _: Result<(), _> = conn.del(Self::k(key)).await;
                    Ok(None)
                } else {
                    Ok(Some(entry))
                }
            }
            None => Ok(None),
        }
    }

    async fn put(&self, key: &str, value: CachedResponse) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(&value)?;
        match value.expires_at_ms {
            Some(exp) => {
                // SET then PEXPIREAT so Redis self-evicts at the same instant.
                let _: () = conn
                    .set(Self::k(key), payload)
                    .await
                    .map_err(|e| CacheError::Backend(e.to_string()))?;
                let _: bool = conn
                    .pexpire_at(Self::k(key), exp)
                    .await
                    .map_err(|e| CacheError::Backend(e.to_string()))?;
            }
            None => {
                let _: () = conn
                    .set(Self::k(key), payload)
                    .await
                    .map_err(|e| CacheError::Backend(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let _: i64 = conn
            .del(Self::k(key))
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let keys: Vec<String> = conn
            .keys(format!("{PREFIX}*"))
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        if !keys.is_empty() {
            let _: i64 = conn
                .del(keys)
                .await
                .map_err(|e| CacheError::Backend(e.to_string()))?;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        if pong == "PONG" {
            Ok(())
        } else {
            Err(CacheError::Backend(format!(
                "unexpected ping reply: {pong}"
            )))
        }
    }

    async fn len(&self, _now_ms: i64) -> Result<usize, CacheError> {
        let mut conn = self.conn.clone();
        let keys: Vec<String> = conn
            .keys(format!("{PREFIX}*"))
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(keys.len())
    }
}
