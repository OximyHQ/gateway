//! Zero-config first boot: on a fresh data dir, seed exactly one admin
//! `VirtualKey` (full budget, no model allowlist, no expiry) and return its
//! plaintext secret ONCE — the only moment the secret exists outside the user's
//! clipboard. On a non-fresh store, return `None` and never re-derive a secret.
//!
//! The store is abstracted behind `KeyStore` so this logic is testable without
//! SQLite; the persistent store implements the same trait.

use gateway_spine::{Clock, RateLimits, VirtualKey};

/// The minimal store seam first-boot needs. Tests use a fake in-memory store.
pub trait KeyStore {
    /// Persist a key (only the hash + metadata; never the secret).
    fn insert_key(&self, key: &VirtualKey) -> anyhow::Result<()>;
    /// Number of keys currently stored (used to detect an existing root key).
    fn key_count(&self) -> anyhow::Result<usize>;
}

/// The one-time output of a successful seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedKey {
    /// The full plaintext admin secret — shown once, never stored.
    pub secret: String,
    pub key_id: String,
}

/// A cryptographically random admin secret with the `sk-oximy-` prefix.
fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    // 40 bytes of base62 entropy.
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let body: String = (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("sk-oximy-{body}")
}

/// Seed the default admin key iff the store has no keys. Idempotent: a second
/// call on a populated store returns `Ok(None)`.
pub fn ensure_admin_key(
    store: &dyn KeyStore,
    clock: &dyn Clock,
) -> anyhow::Result<Option<MintedKey>> {
    if store.key_count()? > 0 {
        return Ok(None);
    }
    let secret = generate_secret();
    let key_id = format!("key_admin_{}", clock.now_ms());
    let token_prefix: String = secret.chars().take(12).collect();
    let key = VirtualKey {
        id: key_id.clone(),
        token_hash: VirtualKey::hash_secret(&secret),
        token_prefix,
        max_budget: None, // admin: unlimited
        limits: RateLimits::default(),
        model_allowlist: None, // admin: all models
        expires_at: None,
        revoked: false,
        parent_id: None,
    };
    store.insert_key(&key)?;
    Ok(Some(MintedKey { secret, key_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::MockClock;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeStore {
        keys: Mutex<Vec<VirtualKey>>,
    }

    impl KeyStore for FakeStore {
        fn insert_key(&self, key: &VirtualKey) -> anyhow::Result<()> {
            self.keys.lock().unwrap().push(key.clone());
            Ok(())
        }
        fn key_count(&self) -> anyhow::Result<usize> {
            Ok(self.keys.lock().unwrap().len())
        }
    }

    #[test]
    fn seeds_one_admin_key_on_fresh_store() {
        let store = FakeStore::default();
        let clock = MockClock::new(1_700_000_000_000);
        let minted = ensure_admin_key(&store, &clock)
            .unwrap()
            .expect("fresh store seeds a key");
        assert!(minted.secret.starts_with("sk-oximy-"));
        assert_eq!(store.key_count().unwrap(), 1);

        // The persisted key stores ONLY the hash, never the secret.
        let stored = store.keys.lock().unwrap()[0].clone();
        assert_ne!(stored.token_hash, minted.secret);
        assert!(
            stored.verify(&minted.secret),
            "the minted secret verifies against the hash"
        );
        assert!(stored.max_budget.is_none(), "admin key is unlimited budget");
        assert!(
            stored.model_allowlist.is_none(),
            "admin key allows all models"
        );
    }

    #[test]
    fn second_boot_does_not_reseed_or_reveal_secret() {
        let store = FakeStore::default();
        let clock = MockClock::new(1_700_000_000_000);
        let _first = ensure_admin_key(&store, &clock).unwrap().unwrap();
        // Second boot: store already has a key → no new secret.
        let second = ensure_admin_key(&store, &clock).unwrap();
        assert_eq!(second, None, "never re-seeds or re-derives a secret");
        assert_eq!(store.key_count().unwrap(), 1, "no duplicate admin key");
    }

    #[test]
    fn generated_secrets_are_unique() {
        let a = generate_secret();
        let b = generate_secret();
        assert_ne!(a, b);
        assert!(a.len() > 40);
    }
}
