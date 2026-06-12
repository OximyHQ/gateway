//! Resolves a bearer secret → the governing `VirtualKey`. The trait is the seam
//! that P1.6 fills with a persistent, CRUD-backed store; P1.4 ships a static,
//! in-memory store bootstrapped from one secret (single-tenant first boot,
//! design §10 "single-tenant static-key bootstrap"). Lookup is by SHA-256 hash
//! so the raw secret is never stored.
//!
//! P1.8 adds `MutableKeyStore`: a live-mutable, `Arc`-sharable store with an
//! optional persistence hook so `POST /v1/admin/keys` and `POST /v1/admin/keys/{id}/revoke`
//! survive a restart AND the `keys` CLI sees the changes.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gateway_spine::VirtualKey;

// ── read-only trait ───────────────────────────────────────────────────────────

/// Resolve an incoming bearer secret to its `VirtualKey`, or `None` if unknown.
pub trait KeyStore: Send + Sync {
    fn resolve(&self, secret: &str) -> Option<VirtualKey>;

    /// Optionally downcast to a `MutableKeyStore` reference for admin CRUD.
    /// Returns `None` for read-only stores (e.g. `StaticKeyStore`).
    fn as_any_mutable(&self) -> Option<&MutableKeyStore> {
        None
    }
}

// ── static (read-only after boot) store ──────────────────────────────────────

/// In-memory store keyed by the secret's SHA-256 hash. Seeded at boot.
#[derive(Debug, Default, Clone)]
pub struct StaticKeyStore {
    by_hash: HashMap<String, VirtualKey>,
}

impl StaticKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key whose `token_hash` already matches the secret it should
    /// resolve. (The key carries its own hash; we index by it.)
    pub fn insert(&mut self, key: VirtualKey) {
        self.by_hash.insert(key.token_hash.clone(), key);
    }

    /// Convenience for single-tenant bootstrap: build a budget-only key from a
    /// raw secret and register it. Returns the key's id.
    pub fn bootstrap(&mut self, secret: &str, max_budget: Option<gateway_spine::Usd>) -> String {
        let hash = VirtualKey::hash_secret(secret);
        let prefix: String = secret.chars().take(8).collect();
        let key = VirtualKey {
            id: "key_bootstrap".into(),
            token_hash: hash,
            token_prefix: prefix,
            max_budget,
            limits: gateway_spine::RateLimits::default(),
            model_allowlist: None,
            tool_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        };
        let id = key.id.clone();
        self.insert(key);
        id
    }
}

impl KeyStore for StaticKeyStore {
    fn resolve(&self, secret: &str) -> Option<VirtualKey> {
        let hash = VirtualKey::hash_secret(secret);
        self.by_hash.get(&hash).cloned()
    }
}

// ── persistence hook ──────────────────────────────────────────────────────────

/// A callback supplied by the binary to write the in-memory state back to disk
/// after every mutation. The closure receives the full key list (values only).
/// Errors are logged but never propagated to the HTTP caller — the in-memory
/// store is always updated first; persistence is best-effort on the same thread.
pub trait PersistHook: Send + Sync {
    fn persist(&self, keys: &[VirtualKey]) -> anyhow::Result<()>;
}

/// No-op hook used in tests and environments where persistence is not needed.
pub struct NoPersist;

impl PersistHook for NoPersist {
    fn persist(&self, _keys: &[VirtualKey]) -> anyhow::Result<()> {
        Ok(())
    }
}

// ── mutable live store (P1.8 admin API) ───────────────────────────────────────

/// Thread-safe, live-mutable key store that both the request-path (`KeyStore`)
/// and the admin API can mutate. Changes are reflected immediately in auth
/// (via `RwLock`) and optionally persisted via the `PersistHook`.
///
/// Indexed by both:
/// - `token_hash` → key (for auth lookups, O(1))
/// - `id` → key (for admin CRUD, O(1))
pub struct MutableKeyStore {
    inner: RwLock<MutableKeyStoreInner>,
    hook: Arc<dyn PersistHook>,
}

#[derive(Default)]
struct MutableKeyStoreInner {
    by_hash: HashMap<String, VirtualKey>,
    by_id: HashMap<String, VirtualKey>,
}

impl MutableKeyStore {
    /// Create an empty store with a no-op persistence hook.
    pub fn new() -> Self {
        Self::with_hook(Arc::new(NoPersist))
    }

    /// Create an empty store with the supplied persistence hook.
    pub fn with_hook(hook: Arc<dyn PersistHook>) -> Self {
        Self {
            inner: RwLock::new(MutableKeyStoreInner::default()),
            hook,
        }
    }

    /// Seed keys loaded from disk at startup (does NOT call the persist hook).
    pub fn seed(&self, keys: impl IntoIterator<Item = VirtualKey>) {
        let mut g = self.inner.write().unwrap();
        for key in keys {
            g.by_id.insert(key.id.clone(), key.clone());
            g.by_hash.insert(key.token_hash.clone(), key);
        }
    }

    /// Insert or replace a key. Calls the persist hook after the write lock is
    /// released.
    pub fn insert(&self, key: VirtualKey) {
        {
            let mut g = self.inner.write().unwrap();
            g.by_id.insert(key.id.clone(), key.clone());
            g.by_hash.insert(key.token_hash.clone(), key);
        }
        self.call_hook();
    }

    /// Mark a key as revoked by id. Returns `false` if the id is not found.
    /// Calls the persist hook on success.
    pub fn revoke(&self, id: &str) -> bool {
        let found = {
            let mut g = self.inner.write().unwrap();
            // Extract the token hash first to avoid borrow conflict.
            let token_hash = g.by_id.get(id).map(|k| k.token_hash.clone());
            if let Some(hash) = token_hash {
                g.by_id.get_mut(id).unwrap().revoked = true;
                if let Some(hk) = g.by_hash.get_mut(&hash) {
                    hk.revoked = true;
                }
                true
            } else {
                false
            }
        };
        if found {
            self.call_hook();
        }
        found
    }

    /// Return a snapshot of all keys (any order). Used by admin list and persist.
    pub fn all_keys(&self) -> Vec<VirtualKey> {
        self.inner.read().unwrap().by_id.values().cloned().collect()
    }

    /// Lookup by id (admin use only; not the hot auth path).
    pub fn get_by_id(&self, id: &str) -> Option<VirtualKey> {
        self.inner.read().unwrap().by_id.get(id).cloned()
    }

    /// Total non-revoked key count.
    pub fn active_count(&self) -> u64 {
        self.inner
            .read()
            .unwrap()
            .by_id
            .values()
            .filter(|k| !k.revoked)
            .count() as u64
    }

    fn call_hook(&self) {
        let keys = self.all_keys();
        if let Err(e) = self.hook.persist(&keys) {
            tracing::warn!(err = %e, "key-store persist hook failed");
        }
    }
}

impl Default for MutableKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyStore for MutableKeyStore {
    fn resolve(&self, secret: &str) -> Option<VirtualKey> {
        let hash = VirtualKey::hash_secret(secret);
        self.inner.read().unwrap().by_hash.get(&hash).cloned()
    }

    fn as_any_mutable(&self) -> Option<&MutableKeyStore> {
        Some(self)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::Usd;

    #[test]
    fn bootstrap_resolves_only_the_right_secret() {
        let mut store = StaticKeyStore::new();
        let id = store.bootstrap("sk-live-abcdefgh", Some(Usd::from_dollars_f64(10.0)));
        assert_eq!(id, "key_bootstrap");

        let resolved = store
            .resolve("sk-live-abcdefgh")
            .expect("known secret resolves");
        assert_eq!(resolved.id, "key_bootstrap");
        assert_eq!(resolved.max_budget, Some(Usd::from_dollars_f64(10.0)));
        assert_eq!(resolved.token_prefix, "sk-live-");

        assert!(store.resolve("sk-wrong").is_none());
    }

    #[test]
    fn never_stores_the_raw_secret() {
        let mut store = StaticKeyStore::new();
        store.bootstrap("super-secret", None);
        // The stored key's hash is not the plaintext.
        let k = store.resolve("super-secret").unwrap();
        assert_ne!(k.token_hash, "super-secret");
        assert_eq!(k.token_hash.len(), 64);
    }

    // ── MutableKeyStore tests ─────────────────────────────────────────────────

    fn vkey(id: &str, secret: &str) -> VirtualKey {
        VirtualKey {
            id: id.into(),
            token_hash: VirtualKey::hash_secret(secret),
            token_prefix: secret.chars().take(8).collect(),
            max_budget: None,
            limits: gateway_spine::RateLimits::default(),
            model_allowlist: None,
            tool_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[test]
    fn mutable_store_resolves_after_insert() {
        let store = MutableKeyStore::new();
        store.insert(vkey("k1", "ogw_abc"));
        let k = store.resolve("ogw_abc").expect("inserted key resolves");
        assert_eq!(k.id, "k1");
        assert!(!k.revoked);
    }

    #[test]
    fn mutable_store_revoke_blocks_auth_immediately() {
        let store = MutableKeyStore::new();
        store.insert(vkey("k1", "ogw_abc"));
        assert!(store.resolve("ogw_abc").is_some());

        let ok = store.revoke("k1");
        assert!(ok, "revoke returns true when found");

        // Key still resolves but is marked revoked.
        let k = store.resolve("ogw_abc").expect("still in map");
        assert!(k.revoked, "revoked flag set");
    }

    #[test]
    fn revoke_missing_id_returns_false() {
        let store = MutableKeyStore::new();
        assert!(!store.revoke("nonexistent"));
    }

    #[test]
    fn seed_does_not_call_hook() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingHook(Arc<AtomicUsize>);
        impl PersistHook for CountingHook {
            fn persist(&self, _: &[VirtualKey]) -> anyhow::Result<()> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let counter = Arc::new(AtomicUsize::new(0));
        let store = MutableKeyStore::with_hook(Arc::new(CountingHook(Arc::clone(&counter))));
        store.seed(vec![vkey("k1", "ogw_seed")]);
        assert_eq!(counter.load(Ordering::SeqCst), 0, "seed must not call hook");
    }

    #[test]
    fn insert_calls_hook_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingHook(Arc<AtomicUsize>);
        impl PersistHook for CountingHook {
            fn persist(&self, _: &[VirtualKey]) -> anyhow::Result<()> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let counter = Arc::new(AtomicUsize::new(0));
        let store = MutableKeyStore::with_hook(Arc::new(CountingHook(Arc::clone(&counter))));
        store.insert(vkey("k1", "ogw_test1"));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        store.insert(vkey("k2", "ogw_test2"));
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn active_count_excludes_revoked() {
        let store = MutableKeyStore::new();
        store.insert(vkey("k1", "ogw_a1"));
        store.insert(vkey("k2", "ogw_a2"));
        store.insert(vkey("k3", "ogw_a3"));
        assert_eq!(store.active_count(), 3);
        store.revoke("k2");
        assert_eq!(store.active_count(), 2);
    }
}
