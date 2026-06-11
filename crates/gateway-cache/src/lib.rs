//! # gateway-cache
//!
//! Tenant-scoped exact-match response cache (SHA-256 of model+endpoint+canonical
//! body), 200-only, with per-request controls (TTL/skip/no-store/namespace),
//! HIT/MISS/age status, streaming replay, and hit-rate + $-saved analytics — over
//! an in-memory L1 with an optional Redis L2 behind a trait. Plus a hot-reloading
//! `ModelRegistry` (models.dev JSON + local overrides) with file-watch + atomic
//! swap.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod control;
pub mod entry;
pub mod error;
pub mod hot_registry;
pub mod key;
pub mod layer;
pub mod memory;
pub mod registry_source;
pub mod replay;
pub mod stats;
pub mod status;
pub mod store;
pub mod tiered;
pub mod watcher;

#[cfg(feature = "redis-l2")]
pub mod redis_store;

pub use control::CacheControl;
pub use entry::{CachedBody, CachedResponse};
pub use error::CacheError;
pub use hot_registry::HotRegistry;
pub use key::CacheKey;
pub use layer::CacheLayer;
pub use memory::MemoryStore;
pub use registry_source::{build_registry, build_registry_from_paths};
pub use replay::{replay_stream, replay_unary};
pub use stats::{CacheStats, CacheStatsSnapshot};
pub use status::{CacheOutcome, CacheStatus};
pub use store::CacheStore;
pub use tiered::TieredStore;
pub use watcher::RegistryWatcher;

#[cfg(feature = "redis-l2")]
pub use redis_store::RedisStore;
