//! Resolves a bearer secret → the governing `VirtualKey`. The trait is the seam
//! that P1.6 fills with a persistent, CRUD-backed store; P1.4 ships a static,
//! in-memory store bootstrapped from one secret (single-tenant first boot,
//! design §10 "single-tenant static-key bootstrap"). Lookup is by SHA-256 hash
//! so the raw secret is never stored.

use std::collections::HashMap;

use gateway_spine::VirtualKey;

/// Resolve an incoming bearer secret to its `VirtualKey`, or `None` if unknown.
pub trait KeyStore: Send + Sync {
    fn resolve(&self, secret: &str) -> Option<VirtualKey>;
}

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
}
