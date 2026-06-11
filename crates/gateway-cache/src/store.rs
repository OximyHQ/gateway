//! The storage seam. L1 (`MemoryStore`) is always present; L2 (`RedisStore`,
//! behind the `redis-l2` feature) is optional — its absence is a compile-time
//! guarantee, not a runtime branch. `ping`/`delete`/`clear` are the cache ops
//! surfaced by the admin API (P1.4/P3). All methods are async so an L2 over the
//! network fits the same trait; the in-memory impl just returns ready futures.

use async_trait::async_trait;

use crate::entry::CachedResponse;
use crate::error::CacheError;

#[async_trait]
pub trait CacheStore: Send + Sync {
    /// Fetch a non-expired entry. Expiry is enforced at `now_ms`. Returns `None`
    /// on miss OR expiry. A backend error is surfaced (the layer treats it as a MISS).
    async fn get(&self, key: &str, now_ms: i64) -> Result<Option<CachedResponse>, CacheError>;

    /// Store an entry. The entry already carries its absolute `expires_at_ms`.
    async fn put(&self, key: &str, value: CachedResponse) -> Result<(), CacheError>;

    /// Delete one key. Idempotent: deleting a missing key is `Ok`.
    async fn delete(&self, key: &str) -> Result<(), CacheError>;

    /// Drop every entry. Used by the admin "clear cache" op.
    async fn clear(&self) -> Result<(), CacheError>;

    /// Liveness probe. Memory always returns `Ok(())`; Redis round-trips a PING.
    async fn ping(&self) -> Result<(), CacheError>;

    /// Current live (non-expired) entry count, for analytics. `now_ms` lets the
    /// memory impl exclude expired-but-not-yet-evicted entries.
    async fn len(&self, now_ms: i64) -> Result<usize, CacheError>;
}
