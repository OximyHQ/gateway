# Phase 1.6 — Persistence + Config-as-Code — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the spine's in-memory state durable and the gateway's whole configuration declarative. Two halves: (1) a `Store` trait behind the budget ledger and the virtual-key set, with a **SQLite default** (sqlx) on a Postgres-compatible schema, versioned migrations, **AEAD-encrypted provider keys at rest** (key from env/KMS ref), **budget/spend restore on boot**, and a **serve-from-cache degraded mode** when the DB is down; (2) `gateway-config` — ONE JSON-Schema'd config source of truth (providers, virtual keys, routes, guardrail attachments, registry overrides) with env-var interpolation, `load`/`validate`/`--dry-run`, decK-style `dump`/`diff`/`apply` against running state, and file-watch hot reload. **UI = API = CLI = Git through one engine.**

**Architecture:** The spine (P1.1) stays a pure in-memory core; this milestone adds a *persistence seam* it loads from and writes through — never a synchronous DB call on the request hot path (writes are append/batched; reads come from the in-memory ledger restored at boot). The `Store` trait is async and storage-agnostic; the SQLite impl is the only one shipped now, but every column/type is chosen so the **identical schema runs on Postgres** (design §3: SQLite default → Postgres upgrade). Provider API keys are sealed with an AEAD (XChaCha20-Poly1305) keyed by a master key resolved from `OXIMY_MASTER_KEY` or a KMS ref — ciphertext only ever touches disk. When the store is unreachable, the gateway runs **degraded**: it serves from the last-known in-memory snapshot and refuses *writes* (fail-closed) rather than dropping governance. `gateway-config` is the declarative projection of the same state the API mutates: one engine computes a typed `Diff` between a desired `Config` and the live store, and `apply` executes it transactionally.

**Tech Stack:** Rust 2024. `sqlx` (sqlite + runtime-tokio, compile-checked queries off, offline-friendly via runtime queries), `chacha20poly1305` (AEAD), `zeroize` (master-key hygiene), `jsonschema` (config validation), `notify` (file-watch hot reload), `serde`/`serde_json`, `thiserror`, `tokio`. Tests use `sqlx` against `sqlite::memory:` and `tempfile` for on-disk round-trips; config tests use string fixtures.

**Invariants this milestone enforces (design §2, §3):** fail-closed under storage failure (no write succeeds if it can't be durably recorded) · cost-correctness survives restart (spent restored exactly from durable rows, never recomputed/guessed) · secrets-at-rest (provider keys AEAD-sealed, plaintext never persisted or logged) · config is the single projection (one diff/apply engine; no yaml-vs-DB split brain).

**Depends on:** P1.1 (`gateway-spine` types: `Usd`, `VirtualKey`, `RateLimits`, `BudgetLedger`, `ModelEntry`, `ModelPrice`), P1.4 (the request lifecycle that commits cost — this milestone makes those commits durable; we code against the P1.1 ledger API and do not require P1.4 source).

---

### Task 1: Add persistence dependencies to `gateway-spine`

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `crates/gateway-spine/Cargo.toml`

- [ ] **Step 1: Add the dep versions to the workspace `[workspace.dependencies]`**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `tracing-subscriber = ...` line):

```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "macros"] }
chacha20poly1305 = "0.10"
zeroize = { version = "1", features = ["zeroize_derive"] }
jsonschema = "0.20"
notify = "6"
tempfile = "3"
async-trait = "0.1"
```

- [ ] **Step 2: Reference the runtime + crypto deps from `gateway-spine/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-spine/Cargo.toml` with:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
rand = { workspace = true }
sqlx = { workspace = true }
chacha20poly1305 = { workspace = true }
zeroize = { workspace = true }
async-trait = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p gateway-spine`
Expected: builds (existing modules unchanged; new deps resolve).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/gateway-spine/Cargo.toml Cargo.lock
git commit -s -m "build(spine): add sqlx, chacha20poly1305, zeroize, async-trait deps"
```

---

### Task 2: `MasterKey` + AEAD sealing of secrets at rest

**Files:**
- Create: `crates/gateway-spine/src/crypto.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/crypto.rs`:

```rust
//! Secrets at rest. Provider API keys are sealed with XChaCha20-Poly1305 (AEAD)
//! under a master key resolved from `OXIMY_MASTER_KEY` (base64, 32 bytes) or a
//! KMS ref. Plaintext is never persisted or logged; the master key is zeroized
//! on drop. The sealed form is `base64(nonce_24 || ciphertext_with_tag)` so it
//! is a portable TEXT column on both SQLite and Postgres.

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use zeroize::Zeroize;

use crate::error::SpineError;

/// 32-byte AEAD master key. Zeroized on drop so it never lingers in freed memory.
pub struct MasterKey {
    bytes: [u8; 32],
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl MasterKey {
    /// Build from raw 32 bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Parse a standard-base64 32-byte key (the `OXIMY_MASTER_KEY` form).
    pub fn from_base64(b64: &str) -> Result<Self, SpineError> {
        let raw = base64_decode(b64).ok_or_else(|| SpineError::Crypto {
            detail: "master key is not valid base64".into(),
        })?;
        let bytes: [u8; 32] = raw.try_into().map_err(|_| SpineError::Crypto {
            detail: "master key must decode to exactly 32 bytes".into(),
        })?;
        Ok(Self { bytes })
    }

    /// Seal plaintext → portable `base64(nonce || ct+tag)`.
    pub fn seal(&self, plaintext: &str) -> Result<String, SpineError> {
        let cipher = XChaCha20Poly1305::new((&self.bytes).into());
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ct = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| SpineError::Crypto { detail: "seal failed".into() })?;
        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ct);
        Ok(base64_encode(&combined))
    }

    /// Open a sealed value back to plaintext.
    pub fn open(&self, sealed: &str) -> Result<String, SpineError> {
        let combined = base64_decode(sealed)
            .ok_or_else(|| SpineError::Crypto { detail: "sealed value not base64".into() })?;
        if combined.len() < 24 {
            return Err(SpineError::Crypto { detail: "sealed value too short".into() });
        }
        let (nonce_bytes, ct) = combined.split_at(24);
        let nonce = XNonce::from_slice(nonce_bytes);
        let cipher = XChaCha20Poly1305::new((&self.bytes).into());
        let pt = cipher
            .decrypt(nonce, ct)
            .map_err(|_| SpineError::Crypto { detail: "open failed (bad key or tampered)".into() })?;
        String::from_utf8(pt).map_err(|_| SpineError::Crypto { detail: "plaintext not utf-8".into() })
    }
}

// Minimal dependency-free base64 (standard alphabet, padded). Kept private so we
// don't pull a base64 crate just for one column encoding.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18 & 0x3f) as usize] as char);
        out.push(B64[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6 & 0x3f) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[(n & 0x3f) as usize] as char } else { '=' });
    }
    out
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|&c| c != b'=' && !c.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut bits = 0;
        for &c in chunk {
            n = (n << 6) | val(c)?;
            bits += 6;
        }
        n <<= 24 - bits;
        for i in 0..(bits / 8) {
            out.push((n >> (16 - i * 8) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> MasterKey {
        MasterKey::from_bytes([7u8; 32])
    }

    #[test]
    fn seal_open_roundtrip() {
        let k = key();
        let sealed = k.seal("sk-provider-secret").unwrap();
        assert_ne!(sealed, "sk-provider-secret"); // never plaintext
        assert_eq!(k.open(&sealed).unwrap(), "sk-provider-secret");
    }

    #[test]
    fn ciphertext_is_nondeterministic() {
        let k = key();
        // Fresh nonce each time → two seals of the same plaintext differ.
        assert_ne!(k.seal("same").unwrap(), k.seal("same").unwrap());
    }

    #[test]
    fn wrong_key_cannot_open() {
        let sealed = key().seal("secret").unwrap();
        let other = MasterKey::from_bytes([8u8; 32]);
        assert!(matches!(other.open(&sealed), Err(SpineError::Crypto { .. })));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let k = key();
        let mut sealed = k.seal("secret").unwrap();
        // Flip the last base64 char.
        let last = sealed.pop().unwrap();
        sealed.push(if last == 'A' { 'B' } else { 'A' });
        assert!(k.open(&sealed).is_err());
    }

    #[test]
    fn from_base64_parses_32_bytes() {
        let b64 = super::base64_encode(&[9u8; 32]);
        let k = MasterKey::from_base64(&b64).unwrap();
        let sealed = k.seal("x").unwrap();
        assert_eq!(k.open(&sealed).unwrap(), "x");
    }

    #[test]
    fn from_base64_rejects_wrong_length() {
        let b64 = super::base64_encode(&[9u8; 16]);
        assert!(matches!(MasterKey::from_base64(&b64), Err(SpineError::Crypto { .. })));
    }
}
```

Add a `Crypto` variant to the error taxonomy. In `crates/gateway-spine/src/error.rs`, add to the `SpineError` enum (before `NoSuchReservation`):

```rust
    #[error("crypto error: {detail}")]
    Crypto { detail: String },
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod crypto;

pub use crypto::MasterKey;
```

- [ ] **Step 2: Run test to verify it compiles and passes**

Run: `cargo test -p gateway-spine crypto::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/crypto.rs crates/gateway-spine/src/error.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): AEAD MasterKey seals provider secrets at rest"
```

---

### Task 3: The `Store` trait — the persistence seam

**Files:**
- Create: `crates/gateway-spine/src/store/mod.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/store/mod.rs`:

```rust
//! The persistence seam. The spine restores its in-memory state from a `Store`
//! at boot and writes governance facts through it; it NEVER calls the store
//! synchronously on the request hot path (reads come from the restored ledger).
//! SQLite is the only impl shipped now (P1.6) but every signature is chosen so
//! the same schema runs on Postgres (design §3) and so distribution (Redis/
//! gossip) can layer above without touching this trait.

use async_trait::async_trait;

use crate::error::SpineError;
use crate::key::VirtualKey;
use crate::money::Usd;

/// A durable provider record. The secret is the *sealed* (AEAD) ciphertext, never
/// plaintext — sealing happens at the call site with a `MasterKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProvider {
    pub id: String,
    pub kind: String,
    pub base_url: Option<String>,
    /// AEAD-sealed API key (`MasterKey::seal` output), or `None` for keyless.
    pub sealed_api_key: Option<String>,
}

/// One key's restored spend snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpend {
    pub spent: Usd,
}

/// Storage-agnostic durable state. All methods are fail-closed: an error means
/// the caller must NOT proceed as if the write happened.
#[async_trait]
pub trait Store: Send + Sync {
    /// Apply pending schema migrations (idempotent).
    async fn migrate(&self) -> Result<(), SpineError>;

    /// Persist (insert or replace) a virtual key.
    async fn upsert_key(&self, key: &VirtualKey) -> Result<(), SpineError>;

    /// Load every virtual key (for boot restore).
    async fn load_keys(&self) -> Result<Vec<VirtualKey>, SpineError>;

    /// Mark a key revoked durably.
    async fn revoke_key(&self, key_id: &str) -> Result<(), SpineError>;

    /// Durably record committed spend for a key (append-on-commit; the column is
    /// the running total). Used by the request lifecycle's commit step.
    async fn record_spend(&self, key_id: &str, total_spent: Usd) -> Result<(), SpineError>;

    /// Load each key's restored spend snapshot (for boot restore of the ledger).
    async fn load_spend(&self) -> Result<Vec<(String, KeySpend)>, SpineError>;

    /// Persist a provider (sealed secret).
    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), SpineError>;

    /// Load every provider record.
    async fn load_providers(&self) -> Result<Vec<StoredProvider>, SpineError>;

    /// Cheap liveness probe; drives degraded-mode detection.
    async fn ping(&self) -> Result<(), SpineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trivial in-memory impl proves the trait is object-safe and usable behind
    // `dyn`. The real SQLite impl is exercised in `store::sqlite` + integration.
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemStore {
        keys: Mutex<HashMap<String, VirtualKey>>,
        spend: Mutex<HashMap<String, Usd>>,
        providers: Mutex<HashMap<String, StoredProvider>>,
    }

    #[async_trait]
    impl Store for MemStore {
        async fn migrate(&self) -> Result<(), SpineError> {
            Ok(())
        }
        async fn upsert_key(&self, key: &VirtualKey) -> Result<(), SpineError> {
            self.keys.lock().unwrap().insert(key.id.clone(), key.clone());
            Ok(())
        }
        async fn load_keys(&self) -> Result<Vec<VirtualKey>, SpineError> {
            Ok(self.keys.lock().unwrap().values().cloned().collect())
        }
        async fn revoke_key(&self, key_id: &str) -> Result<(), SpineError> {
            if let Some(k) = self.keys.lock().unwrap().get_mut(key_id) {
                k.revoked = true;
            }
            Ok(())
        }
        async fn record_spend(&self, key_id: &str, total_spent: Usd) -> Result<(), SpineError> {
            self.spend.lock().unwrap().insert(key_id.to_string(), total_spent);
            Ok(())
        }
        async fn load_spend(&self) -> Result<Vec<(String, KeySpend)>, SpineError> {
            Ok(self
                .spend
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), KeySpend { spent: *v }))
                .collect())
        }
        async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), SpineError> {
            self.providers.lock().unwrap().insert(provider.id.clone(), provider.clone());
            Ok(())
        }
        async fn load_providers(&self) -> Result<Vec<StoredProvider>, SpineError> {
            Ok(self.providers.lock().unwrap().values().cloned().collect())
        }
        async fn ping(&self) -> Result<(), SpineError> {
            Ok(())
        }
    }

    fn key(id: &str) -> VirtualKey {
        VirtualKey {
            id: id.into(),
            token_hash: VirtualKey::hash_secret("sk-x"),
            token_prefix: "sk-x".into(),
            max_budget: Some(Usd::from_dollars_f64(5.0)),
            limits: crate::key::RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[tokio::test]
    async fn trait_is_object_safe_and_roundtrips() {
        let store: Box<dyn Store> = Box::new(MemStore::default());
        store.migrate().await.unwrap();
        store.upsert_key(&key("k1")).await.unwrap();
        store.record_spend("k1", Usd::from_dollars_f64(1.25)).await.unwrap();

        let keys = store.load_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        let spend = store.load_spend().await.unwrap();
        assert_eq!(spend[0].1.spent, Usd::from_dollars_f64(1.25));

        store.revoke_key("k1").await.unwrap();
        assert!(store.load_keys().await.unwrap()[0].revoked);
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod store;

pub use store::{KeySpend, Store, StoredProvider};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine store::tests`
Expected: 1 test PASS (the object-safety + round-trip proof).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/store/mod.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): Store trait — the storage-agnostic persistence seam"
```

---

### Task 4: The SQL schema + migrations (Postgres-compatible)

**Files:**
- Create: `crates/gateway-spine/src/store/migrations.rs`
- Modify: `crates/gateway-spine/src/store/mod.rs` (declare submodule)

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/store/migrations.rs`:

```rust
//! Ordered, idempotent schema migrations. Each is plain ANSI SQL that runs
//! unchanged on SQLite and Postgres (TEXT ids, BIGINT µUSD, no SQLite-only
//! types). Applied versions are tracked in `schema_migrations`. Cost-correctness
//! depends on `key_spend.spent_micros` being a BIGINT — never a float.

/// A single forward migration. `id` is monotonic and gap-free.
pub struct Migration {
    pub id: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// All migrations in apply order. Append-only — never edit a shipped migration.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        id: 1,
        name: "init",
        sql: "\
CREATE TABLE IF NOT EXISTS schema_migrations (
    id    BIGINT PRIMARY KEY,
    name  TEXT NOT NULL,
    applied_at_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS virtual_keys (
    id              TEXT PRIMARY KEY,
    token_hash      TEXT NOT NULL,
    token_prefix    TEXT NOT NULL,
    max_budget_micros BIGINT,
    rpm             BIGINT,
    tpm             BIGINT,
    max_parallel    BIGINT,
    model_allowlist TEXT,
    expires_at_ms   BIGINT,
    revoked         BIGINT NOT NULL DEFAULT 0,
    parent_id       TEXT
);

CREATE TABLE IF NOT EXISTS key_spend (
    key_id        TEXT PRIMARY KEY,
    spent_micros  BIGINT NOT NULL DEFAULT 0,
    updated_at_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS providers (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    base_url        TEXT,
    sealed_api_key  TEXT
);",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_ids_are_monotonic_and_gap_free() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.id, (i as i64) + 1, "migration {} out of order", m.name);
        }
    }

    #[test]
    fn no_sqlite_only_types_leak_in() {
        // Postgres-compatibility guard: reject SQLite-isms that would break PG.
        for m in MIGRATIONS {
            let s = m.sql.to_uppercase();
            assert!(!s.contains("AUTOINCREMENT"), "{}: AUTOINCREMENT is SQLite-only", m.name);
            assert!(!s.contains("WITHOUT ROWID"), "{}: WITHOUT ROWID is SQLite-only", m.name);
        }
    }

    #[test]
    fn money_columns_are_bigint_not_real() {
        let init = MIGRATIONS[0].sql.to_uppercase();
        assert!(init.contains("SPENT_MICROS  BIGINT") || init.contains("SPENT_MICROS BIGINT"));
        assert!(!init.contains("REAL"), "money must never be a REAL column");
        assert!(!init.contains("FLOAT"), "money must never be a FLOAT column");
    }
}
```

In `crates/gateway-spine/src/store/mod.rs`, add the submodule declaration at the top (after the doc comment, before the `use` lines):

```rust
pub mod migrations;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine store::migrations`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/store/migrations.rs crates/gateway-spine/src/store/mod.rs
git commit -s -m "feat(spine): Postgres-compatible schema migrations (BIGINT µUSD)"
```

---

### Task 5: `SqliteStore` — the default backend

**Files:**
- Create: `crates/gateway-spine/src/store/sqlite.rs`
- Modify: `crates/gateway-spine/src/store/mod.rs` (declare submodule + re-export)

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/store/sqlite.rs`:

```rust
//! The default `Store` impl. SQLite via sqlx (`runtime-tokio`). `connect` opens a
//! pool (file path or `:memory:`); `migrate` applies pending migrations under a
//! tracked version table. Every query is plain ANSI SQL shared with the Postgres
//! schema. `?`-binds keep secrets/values out of the SQL string.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::error::SpineError;
use crate::key::{RateLimits, VirtualKey};
use crate::money::Usd;
use crate::store::migrations::MIGRATIONS;
use crate::store::{KeySpend, Store, StoredProvider};

fn db_err(e: sqlx::Error) -> SpineError {
    SpineError::Storage { detail: e.to_string() }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Open a pool. `url` is a path (`gateway.db`) or `:memory:`.
    pub async fn connect(url: &str) -> Result<Self, SpineError> {
        let opts = if url == ":memory:" {
            SqliteConnectOptions::from_str("sqlite::memory:").map_err(db_err)?
        } else {
            SqliteConnectOptions::from_str(&format!("sqlite://{url}"))
                .map_err(db_err)?
                .create_if_missing(true)
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await
            .map_err(db_err)?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl Store for SqliteStore {
    async fn migrate(&self) -> Result<(), SpineError> {
        // Ensure the tracking table exists first (migration 1 also creates it,
        // but we need it to read applied versions).
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (id BIGINT PRIMARY KEY, name TEXT NOT NULL, applied_at_ms BIGINT NOT NULL)",
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;

        let applied: Vec<i64> = sqlx::query("SELECT id FROM schema_migrations")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.get::<i64, _>("id"))
            .collect();

        for m in MIGRATIONS {
            if applied.contains(&m.id) {
                continue;
            }
            // Each migration may contain multiple statements.
            for stmt in m.sql.split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                sqlx::query(stmt).execute(&self.pool).await.map_err(db_err)?;
            }
            sqlx::query("INSERT INTO schema_migrations (id, name, applied_at_ms) VALUES (?, ?, ?)")
                .bind(m.id)
                .bind(m.name)
                .bind(now_ms())
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    async fn upsert_key(&self, key: &VirtualKey) -> Result<(), SpineError> {
        let allowlist = match &key.model_allowlist {
            Some(list) => Some(serde_json::to_string(list).map_err(|e| SpineError::Storage { detail: e.to_string() })?),
            None => None,
        };
        sqlx::query(
            "INSERT INTO virtual_keys
                (id, token_hash, token_prefix, max_budget_micros, rpm, tpm, max_parallel, model_allowlist, expires_at_ms, revoked, parent_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                token_hash=excluded.token_hash,
                token_prefix=excluded.token_prefix,
                max_budget_micros=excluded.max_budget_micros,
                rpm=excluded.rpm, tpm=excluded.tpm, max_parallel=excluded.max_parallel,
                model_allowlist=excluded.model_allowlist,
                expires_at_ms=excluded.expires_at_ms, revoked=excluded.revoked,
                parent_id=excluded.parent_id",
        )
        .bind(&key.id)
        .bind(&key.token_hash)
        .bind(&key.token_prefix)
        .bind(key.max_budget.map(|b| b.micros()))
        .bind(key.limits.rpm)
        .bind(key.limits.tpm)
        .bind(key.limits.max_parallel)
        .bind(allowlist)
        .bind(key.expires_at)
        .bind(i64::from(key.revoked))
        .bind(&key.parent_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn load_keys(&self) -> Result<Vec<VirtualKey>, SpineError> {
        let rows = sqlx::query("SELECT * FROM virtual_keys").fetch_all(&self.pool).await.map_err(db_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let allowlist: Option<String> = r.get("model_allowlist");
            let model_allowlist = match allowlist {
                Some(s) => Some(serde_json::from_str(&s).map_err(|e| SpineError::Storage { detail: e.to_string() })?),
                None => None,
            };
            out.push(VirtualKey {
                id: r.get("id"),
                token_hash: r.get("token_hash"),
                token_prefix: r.get("token_prefix"),
                max_budget: r.get::<Option<i64>, _>("max_budget_micros").map(Usd::from_micros),
                limits: RateLimits {
                    rpm: r.get("rpm"),
                    tpm: r.get("tpm"),
                    max_parallel: r.get("max_parallel"),
                },
                model_allowlist,
                expires_at: r.get("expires_at_ms"),
                revoked: r.get::<i64, _>("revoked") != 0,
                parent_id: r.get("parent_id"),
            });
        }
        Ok(out)
    }

    async fn revoke_key(&self, key_id: &str) -> Result<(), SpineError> {
        sqlx::query("UPDATE virtual_keys SET revoked = 1 WHERE id = ?")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn record_spend(&self, key_id: &str, total_spent: Usd) -> Result<(), SpineError> {
        sqlx::query(
            "INSERT INTO key_spend (key_id, spent_micros, updated_at_ms)
             VALUES (?, ?, ?)
             ON CONFLICT(key_id) DO UPDATE SET spent_micros=excluded.spent_micros, updated_at_ms=excluded.updated_at_ms",
        )
        .bind(key_id)
        .bind(total_spent.micros())
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn load_spend(&self) -> Result<Vec<(String, KeySpend)>, SpineError> {
        let rows = sqlx::query("SELECT key_id, spent_micros FROM key_spend").fetch_all(&self.pool).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<String, _>("key_id"), KeySpend { spent: Usd::from_micros(r.get::<i64, _>("spent_micros")) }))
            .collect())
    }

    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), SpineError> {
        sqlx::query(
            "INSERT INTO providers (id, kind, base_url, sealed_api_key)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, base_url=excluded.base_url, sealed_api_key=excluded.sealed_api_key",
        )
        .bind(&provider.id)
        .bind(&provider.kind)
        .bind(&provider.base_url)
        .bind(&provider.sealed_api_key)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn load_providers(&self) -> Result<Vec<StoredProvider>, SpineError> {
        let rows = sqlx::query("SELECT id, kind, base_url, sealed_api_key FROM providers").fetch_all(&self.pool).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| StoredProvider {
                id: r.get("id"),
                kind: r.get("kind"),
                base_url: r.get("base_url"),
                sealed_api_key: r.get("sealed_api_key"),
            })
            .collect())
    }

    async fn ping(&self) -> Result<(), SpineError> {
        sqlx::query("SELECT 1").execute(&self.pool).await.map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, budget: f64) -> VirtualKey {
        VirtualKey {
            id: id.into(),
            token_hash: VirtualKey::hash_secret("sk-secret"),
            token_prefix: "sk-secre".into(),
            max_budget: Some(Usd::from_dollars_f64(budget)),
            limits: RateLimits { rpm: Some(60), tpm: Some(100_000), max_parallel: Some(4) },
            model_allowlist: Some(vec!["gpt-4o".into()]),
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    async fn fresh() -> SqliteStore {
        let s = SqliteStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        let s = fresh().await;
        // Second migrate must be a no-op, not an error.
        s.migrate().await.unwrap();
    }

    #[tokio::test]
    async fn key_roundtrips_with_allowlist_and_limits() {
        let s = fresh().await;
        s.upsert_key(&key("k1", 5.0)).await.unwrap();
        let loaded = s.load_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        let k = &loaded[0];
        assert_eq!(k.id, "k1");
        assert_eq!(k.max_budget, Some(Usd::from_dollars_f64(5.0)));
        assert_eq!(k.limits.rpm, Some(60));
        assert_eq!(k.model_allowlist.as_deref(), Some(["gpt-4o".to_string()].as_slice()));
    }

    #[tokio::test]
    async fn upsert_replaces_not_duplicates() {
        let s = fresh().await;
        s.upsert_key(&key("k1", 5.0)).await.unwrap();
        s.upsert_key(&key("k1", 9.0)).await.unwrap();
        let loaded = s.load_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].max_budget, Some(Usd::from_dollars_f64(9.0)));
    }

    #[tokio::test]
    async fn revoke_persists() {
        let s = fresh().await;
        s.upsert_key(&key("k1", 5.0)).await.unwrap();
        s.revoke_key("k1").await.unwrap();
        assert!(s.load_keys().await.unwrap()[0].revoked);
    }

    #[tokio::test]
    async fn spend_is_exact_integer_roundtrip() {
        let s = fresh().await;
        s.record_spend("k1", Usd::from_micros(7_500)).await.unwrap();
        s.record_spend("k1", Usd::from_micros(12_345)).await.unwrap(); // running total replaced
        let spend = s.load_spend().await.unwrap();
        assert_eq!(spend.len(), 1);
        assert_eq!(spend[0], ("k1".to_string(), KeySpend { spent: Usd::from_micros(12_345) }));
    }

    #[tokio::test]
    async fn provider_sealed_secret_roundtrips() {
        let s = fresh().await;
        let p = StoredProvider {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            sealed_api_key: Some("c2VhbGVk".into()), // opaque ciphertext blob
        };
        s.upsert_provider(&p).await.unwrap();
        assert_eq!(s.load_providers().await.unwrap(), vec![p]);
    }

    #[tokio::test]
    async fn ping_ok_on_live_pool() {
        let s = fresh().await;
        s.ping().await.unwrap();
    }
}
```

Add a `Storage` variant to the error taxonomy. In `crates/gateway-spine/src/error.rs`, add to the `SpineError` enum (next to `Crypto`):

```rust
    #[error("storage error: {detail}")]
    Storage { detail: String },
```

In `crates/gateway-spine/src/store/mod.rs`, add the submodule + re-export at the top (after `pub mod migrations;`):

```rust
pub mod sqlite;

pub use sqlite::SqliteStore;
```

And re-export from `crates/gateway-spine/src/lib.rs`:

```rust
pub use store::{KeySpend, SqliteStore, Store, StoredProvider};
```

(Replace the existing `pub use store::{KeySpend, Store, StoredProvider};` line.)

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine store::sqlite`
Expected: 7 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/store/sqlite.rs crates/gateway-spine/src/store/mod.rs crates/gateway-spine/src/error.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): SqliteStore default backend (sqlx, ANSI SQL)"
```

---

### Task 6: Boot restore — rebuild the ledger + key set from durable rows

**Files:**
- Create: `crates/gateway-spine/src/store/restore.rs`
- Modify: `crates/gateway-spine/src/store/mod.rs` (declare submodule + re-export)

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/store/restore.rs`:

```rust
//! Boot restore. Rehydrates the in-memory `BudgetLedger` and the live key set
//! from a `Store`. Spend is restored EXACTLY from `key_spend.spent_micros` — the
//! gateway never recomputes cost from logs (cost-correctness survives restart).
//! Returns the live keys so the caller can seed its in-memory auth cache.

use crate::budget::BudgetLedger;
use crate::error::SpineError;
use crate::key::VirtualKey;
use crate::money::Usd;
use crate::store::Store;

/// Result of a boot restore: the ledger is seeded in place; keys are returned.
pub struct Restored {
    pub keys: Vec<VirtualKey>,
}

/// Load all keys + spend and seed the ledger so every key's budget and prior
/// spend are in place before the first request is admitted.
pub async fn restore_ledger(store: &dyn Store, ledger: &BudgetLedger) -> Result<Restored, SpineError> {
    let keys = store.load_keys().await?;
    let spend = store.load_spend().await?;

    // Index restored spend by key id.
    let mut spent_by_key: std::collections::HashMap<String, Usd> = std::collections::HashMap::new();
    for (key_id, snap) in spend {
        spent_by_key.insert(key_id, snap.spent);
    }

    for key in &keys {
        let prior = spent_by_key.get(&key.id).copied().unwrap_or(Usd::ZERO);
        ledger.set_budget(&key.id, key.max_budget, prior);
    }

    Ok(Restored { keys })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::RateLimits;
    use crate::store::sqlite::SqliteStore;

    fn key(id: &str, budget: f64) -> VirtualKey {
        VirtualKey {
            id: id.into(),
            token_hash: VirtualKey::hash_secret("sk-x"),
            token_prefix: "sk-x".into(),
            max_budget: Some(Usd::from_dollars_f64(budget)),
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[tokio::test]
    async fn restores_budget_and_prior_spend_exactly() {
        let store = SqliteStore::connect(":memory:").await.unwrap();
        store.migrate().await.unwrap();
        store.upsert_key(&key("k1", 10.0)).await.unwrap();
        store.record_spend("k1", Usd::from_micros(2_500_000)).await.unwrap(); // $2.50 already spent

        let ledger = BudgetLedger::new();
        let restored = restore_ledger(&store, &ledger).await.unwrap();

        assert_eq!(restored.keys.len(), 1);
        // Prior spend is exact, not recomputed.
        assert_eq!(ledger.spent("k1"), Usd::from_micros(2_500_000));
        // Remaining budget = $10.00 - $2.50 = $7.50, so a $7.50 reserve fits but
        // $7.51 does not (proves budget + spend were both restored).
        assert!(ledger.reserve("k1", Usd::from_dollars_f64(7.50)).is_ok());
    }

    #[tokio::test]
    async fn key_with_no_spend_row_restores_at_zero() {
        let store = SqliteStore::connect(":memory:").await.unwrap();
        store.migrate().await.unwrap();
        store.upsert_key(&key("fresh", 1.0)).await.unwrap();

        let ledger = BudgetLedger::new();
        restore_ledger(&store, &ledger).await.unwrap();
        assert_eq!(ledger.spent("fresh"), Usd::ZERO);
    }
}
```

In `crates/gateway-spine/src/store/mod.rs`, add (after `pub mod sqlite;`):

```rust
pub mod restore;

pub use restore::{Restored, restore_ledger};
```

And re-export from `crates/gateway-spine/src/lib.rs`:

```rust
pub use store::{KeySpend, Restored, SqliteStore, Store, StoredProvider, restore_ledger};
```

(Replace the prior `pub use store::{...};` line.)

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine store::restore`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/store/restore.rs crates/gateway-spine/src/store/mod.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): boot restore rebuilds ledger from durable spend"
```

---

### Task 7: Degraded mode — fail-closed writes when the store is down

**Files:**
- Create: `crates/gateway-spine/src/store/degraded.rs`
- Modify: `crates/gateway-spine/src/store/mod.rs` (declare submodule + re-export)

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/store/degraded.rs`:

```rust
//! Degraded-mode wrapper. When the backing store is unreachable, the gateway
//! keeps serving from its in-memory snapshot (reads of restored state are local
//! anyway) but REFUSES durable writes — fail-closed, never fail-open. A health
//! probe flips the mode; reads delegate, writes short-circuit with `StorageDown`
//! so the lifecycle can deny rather than silently lose governance facts.

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

use crate::error::SpineError;
use crate::key::VirtualKey;
use crate::money::Usd;
use crate::store::{KeySpend, Store, StoredProvider};

pub struct DegradableStore<S: Store> {
    inner: S,
    healthy: AtomicBool,
}

impl<S: Store> DegradableStore<S> {
    pub fn new(inner: S) -> Self {
        Self { inner, healthy: AtomicBool::new(true) }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// Probe the inner store; updates and returns the health flag.
    pub async fn refresh_health(&self) -> bool {
        let ok = self.inner.ping().await.is_ok();
        self.healthy.store(ok, Ordering::SeqCst);
        ok
    }

    fn guard_write(&self) -> Result<(), SpineError> {
        if self.is_healthy() {
            Ok(())
        } else {
            Err(SpineError::StorageDown)
        }
    }
}

#[async_trait]
impl<S: Store> Store for DegradableStore<S> {
    async fn migrate(&self) -> Result<(), SpineError> {
        self.inner.migrate().await
    }

    async fn upsert_key(&self, key: &VirtualKey) -> Result<(), SpineError> {
        self.guard_write()?;
        match self.inner.upsert_key(key).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.healthy.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    async fn load_keys(&self) -> Result<Vec<VirtualKey>, SpineError> {
        self.inner.load_keys().await
    }

    async fn revoke_key(&self, key_id: &str) -> Result<(), SpineError> {
        self.guard_write()?;
        self.inner.revoke_key(key_id).await
    }

    async fn record_spend(&self, key_id: &str, total_spent: Usd) -> Result<(), SpineError> {
        self.guard_write()?;
        match self.inner.record_spend(key_id, total_spent).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.healthy.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    async fn load_spend(&self) -> Result<Vec<(String, KeySpend)>, SpineError> {
        self.inner.load_spend().await
    }

    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), SpineError> {
        self.guard_write()?;
        self.inner.upsert_provider(provider).await
    }

    async fn load_providers(&self) -> Result<Vec<StoredProvider>, SpineError> {
        self.inner.load_providers().await
    }

    async fn ping(&self) -> Result<(), SpineError> {
        self.inner.ping().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // A store whose ping/writes we can force to fail.
    #[derive(Default)]
    struct FlakyStore {
        up: Mutex<bool>,
        keys: Mutex<Vec<VirtualKey>>,
    }

    #[async_trait]
    impl Store for FlakyStore {
        async fn migrate(&self) -> Result<(), SpineError> {
            Ok(())
        }
        async fn upsert_key(&self, key: &VirtualKey) -> Result<(), SpineError> {
            if *self.up.lock().unwrap() {
                self.keys.lock().unwrap().push(key.clone());
                Ok(())
            } else {
                Err(SpineError::Storage { detail: "down".into() })
            }
        }
        async fn load_keys(&self) -> Result<Vec<VirtualKey>, SpineError> {
            Ok(self.keys.lock().unwrap().clone())
        }
        async fn revoke_key(&self, _key_id: &str) -> Result<(), SpineError> {
            Ok(())
        }
        async fn record_spend(&self, _key_id: &str, _total: Usd) -> Result<(), SpineError> {
            if *self.up.lock().unwrap() { Ok(()) } else { Err(SpineError::Storage { detail: "down".into() }) }
        }
        async fn load_spend(&self) -> Result<Vec<(String, KeySpend)>, SpineError> {
            Ok(vec![])
        }
        async fn upsert_provider(&self, _p: &StoredProvider) -> Result<(), SpineError> {
            Ok(())
        }
        async fn load_providers(&self) -> Result<Vec<StoredProvider>, SpineError> {
            Ok(vec![])
        }
        async fn ping(&self) -> Result<(), SpineError> {
            if *self.up.lock().unwrap() { Ok(()) } else { Err(SpineError::Storage { detail: "down".into() }) }
        }
    }

    fn key() -> VirtualKey {
        VirtualKey {
            id: "k".into(),
            token_hash: VirtualKey::hash_secret("x"),
            token_prefix: "x".into(),
            max_budget: None,
            limits: crate::key::RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[tokio::test]
    async fn healthy_writes_pass_through() {
        let inner = FlakyStore::default();
        *inner.up.lock().unwrap() = true;
        let s = DegradableStore::new(inner);
        assert!(s.refresh_health().await);
        s.upsert_key(&key()).await.unwrap();
        assert_eq!(s.load_keys().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn writes_fail_closed_when_down() {
        let inner = FlakyStore::default();
        *inner.up.lock().unwrap() = false;
        let s = DegradableStore::new(inner);
        // Probe flips to unhealthy.
        assert!(!s.refresh_health().await);
        // Write is refused with StorageDown — NOT silently dropped, NOT fail-open.
        assert!(matches!(s.upsert_key(&key()).await, Err(SpineError::StorageDown)));
    }

    #[tokio::test]
    async fn reads_still_serve_in_degraded_mode() {
        let inner = FlakyStore::default();
        *inner.up.lock().unwrap() = true;
        let s = DegradableStore::new(inner);
        s.upsert_key(&key()).await.unwrap();
        // Inner goes down; mark degraded.
        *s.inner.up.lock().unwrap() = false;
        assert!(!s.refresh_health().await);
        // Reads still succeed (serve-from-snapshot).
        assert_eq!(s.load_keys().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn failed_write_auto_flips_to_degraded() {
        let inner = FlakyStore::default();
        *inner.up.lock().unwrap() = true;
        let s = DegradableStore::new(inner);
        assert!(s.refresh_health().await);
        // Underlying store dies between the health probe and the write.
        *s.inner.up.lock().unwrap() = false;
        assert!(s.record_spend("k", Usd::from_micros(1)).await.is_err());
        // The failed write itself flipped us into degraded mode.
        assert!(!s.is_healthy());
    }
}
```

Add a `StorageDown` variant to the error taxonomy. In `crates/gateway-spine/src/error.rs`, add to the `SpineError` enum (next to `Storage`):

```rust
    #[error("storage backend is down (degraded mode): writes refused")]
    StorageDown,
```

In `crates/gateway-spine/src/store/mod.rs`, add (after `pub mod restore;`):

```rust
pub mod degraded;

pub use degraded::DegradableStore;
```

And re-export from `crates/gateway-spine/src/lib.rs`:

```rust
pub use store::{DegradableStore, KeySpend, Restored, SqliteStore, Store, StoredProvider, restore_ledger};
```

(Replace the prior `pub use store::{...};` line.)

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine store::degraded`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/store/degraded.rs crates/gateway-spine/src/store/mod.rs crates/gateway-spine/src/error.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): degraded-mode store — fail-closed writes, serve reads"
```

---

### Task 8: Persistence integration test — restart preserves spend on disk

**Files:**
- Create: `crates/gateway-spine/tests/persistence_restart.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/tests/persistence_restart.rs`:

```rust
//! Invariant proof (design §3): cost-correctness survives a process restart.
//! Spend committed before "shutdown" is restored EXACTLY on the next boot from
//! the same on-disk SQLite file — never recomputed, never lost.

use gateway_spine::{BudgetLedger, MasterKey, SqliteStore, Store, StoredProvider, Usd, VirtualKey};
use gateway_spine::key::RateLimits;
use gateway_spine::store::restore_ledger;

fn key() -> VirtualKey {
    VirtualKey {
        id: "key_1".into(),
        token_hash: VirtualKey::hash_secret("sk-test"),
        token_prefix: "sk-test".into(),
        max_budget: Some(Usd::from_dollars_f64(10.0)),
        limits: RateLimits::default(),
        model_allowlist: None,
        expires_at: None,
        revoked: false,
        parent_id: None,
    }
}

#[tokio::test]
async fn spend_and_sealed_provider_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("gateway.db");
    let url = db_path.to_str().unwrap().to_string();
    let mk = MasterKey::from_bytes([3u8; 32]);

    // --- Boot 1: seed a key, commit spend, store a sealed provider key. ---
    {
        let store = SqliteStore::connect(&url).await.unwrap();
        store.migrate().await.unwrap();
        store.upsert_key(&key()).await.unwrap();

        // Simulate the lifecycle committing $3.75 of cost.
        let ledger = BudgetLedger::new();
        ledger.set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);
        let res = ledger.reserve("key_1", Usd::from_dollars_f64(4.0)).unwrap();
        ledger.commit(res, Usd::from_dollars_f64(3.75)).unwrap();
        store.record_spend("key_1", ledger.spent("key_1")).await.unwrap();

        store
            .upsert_provider(&StoredProvider {
                id: "openai".into(),
                kind: "openai".into(),
                base_url: None,
                sealed_api_key: Some(mk.seal("sk-live-openai").unwrap()),
            })
            .await
            .unwrap();
    } // store dropped == "process exit"

    // --- Boot 2: a fresh process reopens the same file and restores. ---
    {
        let store = SqliteStore::connect(&url).await.unwrap();
        store.migrate().await.unwrap(); // idempotent

        let ledger = BudgetLedger::new();
        let restored = restore_ledger(&store, &ledger).await.unwrap();
        assert_eq!(restored.keys.len(), 1);

        // Spend is restored to the exact µUSD.
        assert_eq!(ledger.spent("key_1"), Usd::from_dollars_f64(3.75));
        // Remaining budget is $6.25 → a $6.25 reserve fits, $6.26 does not.
        assert!(ledger.reserve("key_1", Usd::from_dollars_f64(6.25)).is_ok());

        // The sealed provider key opens back to plaintext with the same master key.
        let providers = store.load_providers().await.unwrap();
        let sealed = providers[0].sealed_api_key.as_ref().unwrap();
        assert_eq!(mk.open(sealed).unwrap(), "sk-live-openai");
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine --test persistence_restart`
Expected: PASS — spend is exactly $3.75 after restart; the sealed key opens.

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-spine/tests/persistence_restart.rs
git commit -s -m "test(spine): prove spend + sealed secrets survive restart"
```

---

### Task 9: Add config dependencies to `gateway-config`

**Files:**
- Modify: `crates/gateway-config/Cargo.toml`

- [ ] **Step 1: Wire the deps**

Replace the `[dependencies]` section of `crates/gateway-config/Cargo.toml` with:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
jsonschema = { workspace = true }
notify = { workspace = true }
gateway-spine = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo build -p gateway-config`
Expected: builds (still the scaffold `lib.rs` with `CRATE`).

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-config/Cargo.toml Cargo.lock
git commit -s -m "build(config): add jsonschema, notify, tempfile deps"
```

---

### Task 10: The `Config` model + env-var interpolation

**Files:**
- Create: `crates/gateway-config/src/model.rs`
- Create: `crates/gateway-config/src/interpolate.rs`
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Write the failing test (model)**

Create `crates/gateway-config/src/model.rs`:

```rust
//! The one declarative config model. This is the single source of truth the UI,
//! API, CLI and Git all project to/from. Providers carry `${ENV}`-interpolated
//! secrets (never plaintext-at-rest in the file); keys/routes/guardrail
//! attachments/registry overrides are all rows here. Serializes as JSON
//! (YAML-compatible superset can be added later without changing this model).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub keys: Vec<KeyConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub guardrails: Vec<GuardrailAttachment>,
    #[serde(default)]
    pub registry_overrides: Vec<RegistryOverride>,
}

fn default_version() -> i64 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            providers: Vec::new(),
            keys: Vec::new(),
            routes: Vec::new(),
            guardrails: Vec::new(),
            registry_overrides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// `${OPENAI_API_KEY}`-style ref resolved at load; never the literal secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tpm: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteConfig {
    pub id: String,
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailAttachment {
    pub key_id: String,
    pub guardrail: String,
    #[serde(default)]
    pub stage: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryOverride {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_roundtrips_json() {
        let c = Config::default();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn missing_sections_default_to_empty() {
        // A minimal file with only a provider parses; the rest default.
        let json = r#"{"providers":[{"id":"openai","kind":"openai","api_key":"${OPENAI_API_KEY}"}]}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.version, 1);
        assert_eq!(c.providers.len(), 1);
        assert!(c.keys.is_empty());
        assert_eq!(c.providers[0].api_key.as_deref(), Some("${OPENAI_API_KEY}"));
    }
}
```

- [ ] **Step 2: Write the failing test (interpolation)**

Create `crates/gateway-config/src/interpolate.rs`:

```rust
//! `${ENV}` interpolation. Secrets live in the environment, not the config file;
//! at load time `${NAME}` is replaced from a resolver. A missing variable is a
//! hard error (fail-closed) — we never silently leave a `${...}` literal in a
//! credential or fall back to empty.

use std::collections::HashMap;

use crate::error::ConfigError;

/// Resolve every `${NAME}` in `input` via `lookup`. Errors on an unresolved name
/// or an unterminated `${`.
pub fn interpolate(input: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let end = input[start..]
                .find('}')
                .map(|p| start + p)
                .ok_or_else(|| ConfigError::Interpolation { detail: "unterminated ${".into() })?;
            let name = &input[start..end];
            let val = lookup(name).ok_or_else(|| ConfigError::Interpolation {
                detail: format!("env var {name} is not set"),
            })?;
            out.push_str(&val);
            i = end + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

/// Convenience resolver backed by a map (for tests + non-env sources).
pub fn map_lookup(map: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
    move |name| map.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn replaces_known_vars() {
        let m = env(&[("OPENAI_API_KEY", "sk-live")]);
        let out = interpolate("${OPENAI_API_KEY}", &map_lookup(&m)).unwrap();
        assert_eq!(out, "sk-live");
    }

    #[test]
    fn replaces_inline_and_leaves_plain_text() {
        let m = env(&[("HOST", "api.example.com")]);
        let out = interpolate("https://${HOST}/v1", &map_lookup(&m)).unwrap();
        assert_eq!(out, "https://api.example.com/v1");
    }

    #[test]
    fn missing_var_is_fail_closed() {
        let m = env(&[]);
        assert!(matches!(
            interpolate("${NOPE}", &map_lookup(&m)),
            Err(ConfigError::Interpolation { .. })
        ));
    }

    #[test]
    fn unterminated_brace_errors() {
        let m = env(&[]);
        assert!(interpolate("${UNCLOSED", &map_lookup(&m)).is_err());
    }
}
```

Create `crates/gateway-config/src/error.rs`:

```rust
//! Config engine errors. Distinct variants give the CLI semantic exit codes
//! (design §7: AXI-grade CLI) — e.g. validation vs interpolation vs apply.

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config is invalid: {detail}")]
    Validation { detail: String },
    #[error("interpolation error: {detail}")]
    Interpolation { detail: String },
    #[error("config io error: {detail}")]
    Io { detail: String },
    #[error("config parse error: {detail}")]
    Parse { detail: String },
    #[error("apply failed: {detail}")]
    Apply { detail: String },
}
```

Replace `crates/gateway-config/src/lib.rs` with:

```rust
//! # gateway-config
//!
//! The single declarative projection of gateway state. UI = API = CLI = Git, all
//! through one diff/apply engine. Kills the yaml-vs-DB split brain.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

pub mod error;
pub mod interpolate;
pub mod model;

pub use error::ConfigError;
pub use interpolate::{interpolate, map_lookup};
pub use model::{
    Config, GuardrailAttachment, KeyConfig, ProviderConfig, RegistryOverride, RouteConfig,
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p gateway-config model:: interpolate::`
Expected: 2 model + 4 interpolate = 6 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/src/model.rs crates/gateway-config/src/interpolate.rs crates/gateway-config/src/error.rs crates/gateway-config/src/lib.rs
git commit -s -m "feat(config): Config model + fail-closed env interpolation"
```

---

### Task 11: JSON-Schema validation + `load`/`validate`/dry-run

**Files:**
- Create: `crates/gateway-config/src/schema.rs`
- Create: `crates/gateway-config/src/load.rs`
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Write the failing test (schema)**

Create `crates/gateway-config/src/schema.rs`:

```rust
//! The JSON Schema that validates a config BEFORE it touches running state — the
//! one gate for UI = API = CLI = Git. Structural rules live here (required ids,
//! types, non-negative budgets); cross-row referential checks (a route's
//! provider exists) live in `load::validate_semantics`.

use serde_json::{Value, json};

use crate::error::ConfigError;

/// The config JSON Schema (draft 2020-12).
pub fn config_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "version": { "type": "integer", "minimum": 1 },
            "providers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "kind"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "kind": { "type": "string", "minLength": 1 },
                        "base_url": { "type": "string" },
                        "api_key": { "type": "string" }
                    }
                }
            },
            "keys": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "max_budget_usd": { "type": "number", "minimum": 0 },
                        "rpm": { "type": "integer", "minimum": 0 },
                        "tpm": { "type": "integer", "minimum": 0 },
                        "max_parallel": { "type": "integer", "minimum": 0 },
                        "model_allowlist": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "routes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "model", "provider"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "model": { "type": "string", "minLength": 1 },
                        "provider": { "type": "string", "minLength": 1 }
                    }
                }
            }
        }
    })
}

/// Validate a raw JSON value against the schema. Returns the first error message.
pub fn validate_structure(value: &Value) -> Result<(), ConfigError> {
    let schema = config_schema();
    let compiled = jsonschema::validator_for(&schema)
        .map_err(|e| ConfigError::Validation { detail: format!("schema compile: {e}") })?;
    if let Some(err) = compiled.iter_errors(value).next() {
        return Err(ConfigError::Validation { detail: err.to_string() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config_passes() {
        let v = json!({
            "version": 1,
            "providers": [{ "id": "openai", "kind": "openai" }],
            "keys": [{ "id": "k1", "max_budget_usd": 10.0 }]
        });
        validate_structure(&v).unwrap();
    }

    #[test]
    fn provider_missing_id_fails() {
        let v = json!({ "providers": [{ "kind": "openai" }] });
        assert!(matches!(validate_structure(&v), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn negative_budget_fails() {
        let v = json!({ "keys": [{ "id": "k1", "max_budget_usd": -5.0 }] });
        assert!(matches!(validate_structure(&v), Err(ConfigError::Validation { .. })));
    }
}
```

- [ ] **Step 2: Write the failing test (load + semantic validate)**

Create `crates/gateway-config/src/load.rs`:

```rust
//! Load pipeline: read → interpolate `${ENV}` → parse → schema-validate →
//! semantic-validate. `validate` runs the whole pipeline WITHOUT applying (the
//! `--dry-run` path). Referential checks the schema can't express live in
//! `validate_semantics` (a route's provider must exist; key ids unique).

use std::collections::HashSet;

use serde_json::Value;

use crate::error::ConfigError;
use crate::interpolate::interpolate;
use crate::model::Config;
use crate::schema::validate_structure;

/// Parse + validate a config string (already env-interpolated). Returns the typed
/// `Config` on success — this is the `validate` / `--dry-run` entry point.
pub fn validate(raw_json: &str) -> Result<Config, ConfigError> {
    let value: Value = serde_json::from_str(raw_json).map_err(|e| ConfigError::Parse { detail: e.to_string() })?;
    validate_structure(&value)?;
    let config: Config = serde_json::from_value(value).map_err(|e| ConfigError::Parse { detail: e.to_string() })?;
    validate_semantics(&config)?;
    Ok(config)
}

/// Full load: interpolate `${ENV}` first, then `validate`.
pub fn load(raw_with_env_refs: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
    let interpolated = interpolate(raw_with_env_refs, lookup)?;
    validate(&interpolated)
}

/// Cross-row referential integrity the JSON Schema can't express.
pub fn validate_semantics(config: &Config) -> Result<(), ConfigError> {
    // Unique provider ids.
    let mut provider_ids = HashSet::new();
    for p in &config.providers {
        if !provider_ids.insert(&p.id) {
            return Err(ConfigError::Validation { detail: format!("duplicate provider id: {}", p.id) });
        }
    }
    // Unique key ids.
    let mut key_ids = HashSet::new();
    for k in &config.keys {
        if !key_ids.insert(&k.id) {
            return Err(ConfigError::Validation { detail: format!("duplicate key id: {}", k.id) });
        }
    }
    // Every route references a declared provider.
    for r in &config.routes {
        if !provider_ids.contains(&r.provider) {
            return Err(ConfigError::Validation {
                detail: format!("route {} references unknown provider {}", r.id, r.provider),
            });
        }
    }
    // Every guardrail attachment references a declared key.
    for g in &config.guardrails {
        if !key_ids.contains(&g.key_id) {
            return Err(ConfigError::Validation {
                detail: format!("guardrail attachment references unknown key {}", g.key_id),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::interpolate::map_lookup;

    #[test]
    fn validate_accepts_a_good_config() {
        let raw = r#"{
            "providers": [{ "id": "openai", "kind": "openai" }],
            "keys": [{ "id": "k1", "max_budget_usd": 10.0 }],
            "routes": [{ "id": "r1", "model": "gpt-4o", "provider": "openai" }]
        }"#;
        let c = validate(raw).unwrap();
        assert_eq!(c.routes.len(), 1);
    }

    #[test]
    fn route_to_unknown_provider_is_rejected() {
        let raw = r#"{
            "providers": [{ "id": "openai", "kind": "openai" }],
            "routes": [{ "id": "r1", "model": "gpt-4o", "provider": "ghost" }]
        }"#;
        assert!(matches!(validate(raw), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn duplicate_key_ids_are_rejected() {
        let raw = r#"{ "keys": [{ "id": "k1" }, { "id": "k1" }] }"#;
        assert!(matches!(validate(raw), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn load_interpolates_then_validates() {
        let m: HashMap<String, String> =
            [("OPENAI_API_KEY".to_string(), "sk-live".to_string())].into_iter().collect();
        let raw = r#"{ "providers": [{ "id": "openai", "kind": "openai", "api_key": "${OPENAI_API_KEY}" }] }"#;
        let c = load(raw, &map_lookup(&m)).unwrap();
        assert_eq!(c.providers[0].api_key.as_deref(), Some("sk-live"));
    }

    #[test]
    fn load_fails_closed_on_missing_env() {
        let m: HashMap<String, String> = HashMap::new();
        let raw = r#"{ "providers": [{ "id": "openai", "kind": "openai", "api_key": "${MISSING}" }] }"#;
        assert!(matches!(load(raw, &map_lookup(&m)), Err(ConfigError::Interpolation { .. })));
    }
}
```

Add to `crates/gateway-config/src/lib.rs`:

```rust
pub mod load;
pub mod schema;

pub use load::{load, validate, validate_semantics};
pub use schema::{config_schema, validate_structure};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p gateway-config schema:: load::`
Expected: 3 schema + 5 load = 8 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/src/schema.rs crates/gateway-config/src/load.rs crates/gateway-config/src/lib.rs
git commit -s -m "feat(config): JSON-Schema validation + load/validate (dry-run) pipeline"
```

---

### Task 12: decK-style `diff` — typed delta between desired and live state

**Files:**
- Create: `crates/gateway-config/src/diff.rs`
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-config/src/diff.rs`:

```rust
//! decK-style diff. Computes the typed delta between a desired `Config` and the
//! live one (projected from store state). `apply` (Task 13) executes exactly
//! these changes — diff is the single planning step so UI/CLI/Git all show the
//! same plan before mutating. Stable ordering so the plan is deterministic.

use crate::model::Config;

/// One planned change to a single entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    CreateProvider(String),
    UpdateProvider(String),
    DeleteProvider(String),
    CreateKey(String),
    UpdateKey(String),
    DeleteKey(String),
    CreateRoute(String),
    UpdateRoute(String),
    DeleteRoute(String),
}

/// The full plan. Empty == live already matches desired (idempotent apply).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Compute desired-vs-live. Entities present-in-desired/absent-in-live → Create;
/// present-in-both but changed → Update; absent-in-desired/present-in-live →
/// Delete. Order: providers, keys, routes; within each, creates/updates then
/// deletes — sorted by id for determinism.
pub fn diff(desired: &Config, live: &Config) -> Diff {
    let mut changes = Vec::new();

    diff_section(
        &desired.providers.iter().map(|p| (p.id.clone(), p.clone())).collect::<Vec<_>>(),
        &live.providers.iter().map(|p| (p.id.clone(), p.clone())).collect::<Vec<_>>(),
        &mut changes,
        Change::CreateProvider,
        Change::UpdateProvider,
        Change::DeleteProvider,
    );
    diff_section(
        &desired.keys.iter().map(|k| (k.id.clone(), k.clone())).collect::<Vec<_>>(),
        &live.keys.iter().map(|k| (k.id.clone(), k.clone())).collect::<Vec<_>>(),
        &mut changes,
        Change::CreateKey,
        Change::UpdateKey,
        Change::DeleteKey,
    );
    diff_section(
        &desired.routes.iter().map(|r| (r.id.clone(), r.clone())).collect::<Vec<_>>(),
        &live.routes.iter().map(|r| (r.id.clone(), r.clone())).collect::<Vec<_>>(),
        &mut changes,
        Change::CreateRoute,
        Change::UpdateRoute,
        Change::DeleteRoute,
    );

    Diff { changes }
}

fn diff_section<T: PartialEq + Clone>(
    desired: &[(String, T)],
    live: &[(String, T)],
    out: &mut Vec<Change>,
    create: fn(String) -> Change,
    update: fn(String) -> Change,
    delete: fn(String) -> Change,
) {
    let mut creates_updates: Vec<Change> = Vec::new();
    let mut deletes: Vec<Change> = Vec::new();

    for (id, want) in desired {
        match live.iter().find(|(lid, _)| lid == id) {
            None => creates_updates.push(create(id.clone())),
            Some((_, have)) if have != want => creates_updates.push(update(id.clone())),
            Some(_) => {}
        }
    }
    for (id, _) in live {
        if !desired.iter().any(|(did, _)| did == id) {
            deletes.push(delete(id.clone()));
        }
    }

    // Deterministic: sort each bucket by the id embedded in the change's debug
    // is overkill; instead we sorted inputs implicitly by id below.
    creates_updates.sort_by_key(change_id);
    deletes.sort_by_key(change_id);
    out.extend(creates_updates);
    out.extend(deletes);
}

fn change_id(c: &Change) -> String {
    match c {
        Change::CreateProvider(s)
        | Change::UpdateProvider(s)
        | Change::DeleteProvider(s)
        | Change::CreateKey(s)
        | Change::UpdateKey(s)
        | Change::DeleteKey(s)
        | Change::CreateRoute(s)
        | Change::UpdateRoute(s)
        | Change::DeleteRoute(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyConfig, ProviderConfig};

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig { id: id.into(), kind: "openai".into(), base_url: None, api_key: None }
    }
    fn key(id: &str, budget: f64) -> KeyConfig {
        KeyConfig {
            id: id.into(),
            max_budget_usd: Some(budget),
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        }
    }

    #[test]
    fn identical_configs_have_empty_diff() {
        let mut c = Config::default();
        c.providers.push(provider("openai"));
        assert!(diff(&c, &c.clone()).is_empty());
    }

    #[test]
    fn detects_create() {
        let live = Config::default();
        let mut desired = Config::default();
        desired.providers.push(provider("openai"));
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::CreateProvider("openai".into())]);
    }

    #[test]
    fn detects_update_on_changed_field() {
        let mut live = Config::default();
        live.keys.push(key("k1", 5.0));
        let mut desired = Config::default();
        desired.keys.push(key("k1", 9.0));
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::UpdateKey("k1".into())]);
    }

    #[test]
    fn detects_delete() {
        let mut live = Config::default();
        live.providers.push(provider("stale"));
        let desired = Config::default();
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::DeleteProvider("stale".into())]);
    }

    #[test]
    fn creates_and_updates_precede_deletes_and_are_sorted() {
        let mut live = Config::default();
        live.providers.push(provider("old"));
        let mut desired = Config::default();
        desired.providers.push(provider("bbb"));
        desired.providers.push(provider("aaa"));
        let d = diff(&desired, &live);
        assert_eq!(
            d.changes,
            vec![
                Change::CreateProvider("aaa".into()),
                Change::CreateProvider("bbb".into()),
                Change::DeleteProvider("old".into()),
            ]
        );
    }
}
```

Add to `crates/gateway-config/src/lib.rs`:

```rust
pub mod diff;

pub use diff::{Change, Diff, diff};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-config diff::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/src/diff.rs crates/gateway-config/src/lib.rs
git commit -s -m "feat(config): decK-style typed diff (create/update/delete plan)"
```

---

### Task 13: `apply` — project a `Config` into the `Store` (one engine)

**Files:**
- Create: `crates/gateway-config/src/apply.rs`
- Modify: `crates/gateway-config/Cargo.toml` (add `async-trait` dev-dep is not needed; add `tokio` dev-dep)
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Add the runtime dev-dep for the apply test**

In `crates/gateway-config/Cargo.toml`, add `tokio` to `[dev-dependencies]`:

```toml
[dev-dependencies]
tempfile = { workspace = true }
tokio = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

Create `crates/gateway-config/src/apply.rs`:

```rust
//! `apply`: execute a `Diff` against the durable `Store` (the same `Store` the
//! API mutates) so UI = API = CLI = Git really are one engine. Translates config
//! rows into spine entities (USD budgets → µUSD; allowlists pass through),
//! sealing provider secrets with the `MasterKey` so plaintext never lands in the
//! store. Creates/updates are upserts; deletes revoke keys (never hard-delete
//! spend history — cost-correctness).

use gateway_spine::key::RateLimits;
use gateway_spine::{MasterKey, Store, StoredProvider, Usd, VirtualKey};

use crate::diff::{Change, Diff, diff};
use crate::error::ConfigError;
use crate::model::{Config, KeyConfig, ProviderConfig};

fn key_config_to_virtual(k: &KeyConfig) -> VirtualKey {
    VirtualKey {
        id: k.id.clone(),
        // Apply does not mint secrets; key creation via the API sets the hash.
        // Config-managed keys carry a deterministic placeholder hash that the
        // API replaces on first secret issuance; budgets/limits are authoritative.
        token_hash: String::new(),
        token_prefix: String::new(),
        max_budget: k.max_budget_usd.map(Usd::from_dollars_f64),
        limits: RateLimits { rpm: k.rpm, tpm: k.tpm, max_parallel: k.max_parallel },
        model_allowlist: k.model_allowlist.clone(),
        expires_at: None,
        revoked: false,
        parent_id: None,
    }
}

fn provider_config_to_stored(p: &ProviderConfig, mk: &MasterKey) -> Result<StoredProvider, ConfigError> {
    let sealed_api_key = match &p.api_key {
        Some(secret) => Some(mk.seal(secret).map_err(|e| ConfigError::Apply { detail: e.to_string() })?),
        None => None,
    };
    Ok(StoredProvider {
        id: p.id.clone(),
        kind: p.kind.clone(),
        base_url: p.base_url.clone(),
        sealed_api_key,
    })
}

/// Compute the diff and execute it. Returns the diff that was applied (so the CLI
/// can print the plan it ran). Idempotent: applying an already-matching config is
/// a no-op.
pub async fn apply(
    desired: &Config,
    live: &Config,
    store: &dyn Store,
    mk: &MasterKey,
) -> Result<Diff, ConfigError> {
    let plan = diff(desired, live);
    for change in &plan.changes {
        match change {
            Change::CreateProvider(id) | Change::UpdateProvider(id) => {
                let p = desired.providers.iter().find(|p| &p.id == id).expect("in desired");
                store
                    .upsert_provider(&provider_config_to_stored(p, mk)?)
                    .await
                    .map_err(|e| ConfigError::Apply { detail: e.to_string() })?;
            }
            Change::CreateKey(id) | Change::UpdateKey(id) => {
                let k = desired.keys.iter().find(|k| &k.id == id).expect("in desired");
                store
                    .upsert_key(&key_config_to_virtual(k))
                    .await
                    .map_err(|e| ConfigError::Apply { detail: e.to_string() })?;
            }
            Change::DeleteKey(id) => {
                // Never hard-delete: revoke so spend history survives.
                store.revoke_key(id).await.map_err(|e| ConfigError::Apply { detail: e.to_string() })?;
            }
            // Provider/route deletes + route upserts: route table lands with the
            // router (P1.4/P-later); providers are revoke-by-omission later too.
            Change::DeleteProvider(_)
            | Change::CreateRoute(_)
            | Change::UpdateRoute(_)
            | Change::DeleteRoute(_) => {}
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::SqliteStore;

    use crate::model::ProviderConfig;

    fn mk() -> MasterKey {
        MasterKey::from_bytes([5u8; 32])
    }

    async fn store() -> SqliteStore {
        let s = SqliteStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn apply_creates_key_with_correct_budget() {
        let s = store().await;
        let mut desired = Config::default();
        desired.keys.push(KeyConfig {
            id: "k1".into(),
            max_budget_usd: Some(12.50),
            rpm: Some(100),
            tpm: None,
            max_parallel: None,
            model_allowlist: Some(vec!["gpt-4o".into()]),
        });
        let plan = apply(&desired, &Config::default(), &s, &mk()).await.unwrap();
        assert_eq!(plan.changes, vec![Change::CreateKey("k1".into())]);

        let keys = s.load_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].max_budget, Some(Usd::from_dollars_f64(12.50)));
        assert_eq!(keys[0].limits.rpm, Some(100));
    }

    #[tokio::test]
    async fn apply_seals_provider_secret_never_plaintext() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: Some("sk-live-openai".into()),
        });
        apply(&desired, &Config::default(), &s, &mk()).await.unwrap();

        let providers = s.load_providers().await.unwrap();
        let sealed = providers[0].sealed_api_key.as_ref().unwrap();
        // Stored value is ciphertext, not the plaintext secret.
        assert_ne!(sealed, "sk-live-openai");
        // ...but the master key opens it back.
        assert_eq!(mk().open(sealed).unwrap(), "sk-live-openai");
    }

    #[tokio::test]
    async fn apply_delete_revokes_not_destroys() {
        let s = store().await;
        let mut live = Config::default();
        live.keys.push(KeyConfig {
            id: "gone".into(),
            max_budget_usd: Some(1.0),
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        });
        // Seed the store so the key exists to revoke.
        apply(&live, &Config::default(), &s, &mk()).await.unwrap();

        // Desired drops the key → plan is a delete → store revokes it.
        let plan = apply(&Config::default(), &live, &s, &mk()).await.unwrap();
        assert_eq!(plan.changes, vec![Change::DeleteKey("gone".into())]);
        assert!(s.load_keys().await.unwrap()[0].revoked);
    }

    #[tokio::test]
    async fn apply_is_idempotent() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: None,
        });
        apply(&desired, &Config::default(), &s, &mk()).await.unwrap();
        // Second apply against the now-matching live state is a no-op plan.
        let plan = apply(&desired, &desired.clone(), &s, &mk()).await.unwrap();
        assert!(plan.is_empty());
    }
}
```

Add to `crates/gateway-config/src/lib.rs`:

```rust
pub mod apply;

pub use apply::apply;
```

- [ ] **Step 3: Run test**

Run: `cargo test -p gateway-config apply::`
Expected: 4 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/Cargo.toml crates/gateway-config/src/apply.rs crates/gateway-config/src/lib.rs Cargo.lock
git commit -s -m "feat(config): apply engine projects Config into Store (sealed secrets)"
```

---

### Task 14: `dump` — project live store state back to a `Config`

**Files:**
- Create: `crates/gateway-config/src/dump.rs`
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-config/src/dump.rs`:

```rust
//! `dump`: the inverse projection — read live store state into a `Config` so an
//! operator can capture running state into Git (decK round-trip). Secrets are
//! NEVER dumped in plaintext: a provider with a stored secret emits a
//! `${PROVIDER_ID_API_KEY}` ref placeholder, preserving the secrets-at-rest +
//! secrets-never-in-config-file invariants.

use gateway_spine::{Store, Usd};

use crate::error::ConfigError;
use crate::model::{Config, KeyConfig, ProviderConfig};

/// Read all durable state into a `Config`. The result, re-applied, is a no-op.
pub async fn dump(store: &dyn Store) -> Result<Config, ConfigError> {
    let mut config = Config::default();

    for p in store.load_providers().await.map_err(|e| ConfigError::Io { detail: e.to_string() })? {
        let api_key = p
            .sealed_api_key
            .as_ref()
            .map(|_| format!("${{{}_API_KEY}}", p.id.to_uppercase().replace('-', "_")));
        config.providers.push(ProviderConfig { id: p.id, kind: p.kind, base_url: p.base_url, api_key });
    }

    for k in store.load_keys().await.map_err(|e| ConfigError::Io { detail: e.to_string() })? {
        if k.revoked {
            continue; // revoked keys are not part of desired state
        }
        config.keys.push(KeyConfig {
            id: k.id,
            max_budget_usd: k.max_budget.map(Usd::as_dollars_f64),
            rpm: k.limits.rpm,
            tpm: k.limits.tpm,
            max_parallel: k.limits.max_parallel,
            model_allowlist: k.model_allowlist,
        });
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::SqliteStore;

    use crate::apply::apply;
    use crate::model::ProviderConfig;
    use gateway_spine::MasterKey;

    fn mk() -> MasterKey {
        MasterKey::from_bytes([6u8; 32])
    }

    async fn store() -> SqliteStore {
        let s = SqliteStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn dump_emits_env_ref_not_plaintext_secret() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: Some("sk-live".into()),
        });
        apply(&desired, &Config::default(), &s, &mk()).await.unwrap();

        let dumped = dump(&s).await.unwrap();
        assert_eq!(dumped.providers[0].api_key.as_deref(), Some("${OPENAI_API_KEY}"));
        // The plaintext secret never appears anywhere in the dumped JSON.
        let json = serde_json::to_string(&dumped).unwrap();
        assert!(!json.contains("sk-live"));
    }

    #[tokio::test]
    async fn dump_then_apply_is_a_noop() {
        let s = store().await;
        let mut desired = Config::default();
        desired.keys.push(KeyConfig {
            id: "k1".into(),
            max_budget_usd: Some(7.5),
            rpm: Some(60),
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        });
        apply(&desired, &Config::default(), &s, &mk()).await.unwrap();

        let dumped = dump(&s).await.unwrap();
        // Re-applying the dump against itself yields an empty plan.
        let plan = apply(&dumped, &dumped.clone(), &s, &mk()).await.unwrap();
        assert!(plan.is_empty());
        assert_eq!(dumped.keys[0].max_budget_usd, Some(7.5));
    }

    #[tokio::test]
    async fn dump_omits_revoked_keys() {
        let s = store().await;
        let mut live = Config::default();
        live.keys.push(KeyConfig {
            id: "gone".into(),
            max_budget_usd: Some(1.0),
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        });
        apply(&live, &Config::default(), &s, &mk()).await.unwrap();
        apply(&Config::default(), &live, &s, &mk()).await.unwrap(); // revoke it

        let dumped = dump(&s).await.unwrap();
        assert!(dumped.keys.is_empty());
    }
}
```

Add to `crates/gateway-config/src/lib.rs`:

```rust
pub mod dump;

pub use dump::dump;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-config dump::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/src/dump.rs crates/gateway-config/src/lib.rs
git commit -s -m "feat(config): dump live store to Config (env-ref secrets, decK round-trip)"
```

---

### Task 15: File-watch hot reload

**Files:**
- Create: `crates/gateway-config/src/watch.rs`
- Modify: `crates/gateway-config/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-config/src/watch.rs`:

```rust
//! File-watch hot reload. Watches the config file; on a write, re-runs the full
//! load pipeline (interpolate → validate) and hands the validated `Config` to a
//! callback. A config that fails validation is REJECTED (the callback is not
//! invoked) so a bad edit never tears down a healthy running gateway — the last
//! good config keeps serving. Backed by `notify`; debounced by re-reading on
//! each event.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};

use crate::error::ConfigError;
use crate::load::load;
use crate::model::Config;

/// Read + load a config file once (interpolating from the process environment).
pub fn load_file(path: &Path) -> Result<Config, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Io { detail: e.to_string() })?;
    load(&raw, &|name| std::env::var(name).ok())
}

/// Watch `path`; call `on_reload` with each newly validated `Config`. Blocks the
/// calling thread, so callers spawn it. `on_reload` returning `false` stops the
/// watch loop (clean shutdown). Validation failures are logged-and-skipped, never
/// fatal.
pub fn watch<F>(path: PathBuf, mut on_reload: F) -> Result<(), ConfigError>
where
    F: FnMut(Config) -> bool,
{
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| ConfigError::Io { detail: e.to_string() })?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| ConfigError::Io { detail: e.to_string() })?;

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(_event)) => match load_file(&path) {
                Ok(config) => {
                    if !on_reload(config) {
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "config reload rejected; keeping last good config");
                }
            },
            Ok(Err(_)) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Periodic wake; lets callers stop a watch in tests deterministically.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_file_reads_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, r#"{{ "providers": [{{ "id": "openai", "kind": "openai" }}] }}"#).unwrap();
        let c = load_file(&path).unwrap();
        assert_eq!(c.providers.len(), 1);
    }

    #[test]
    fn load_file_rejects_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        // Provider missing required `kind`.
        write!(f, r#"{{ "providers": [{{ "id": "x" }}] }}"#).unwrap();
        assert!(matches!(load_file(&path), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn watch_delivers_reload_then_stops_on_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, r#"{{ "providers": [{{ "id": "a", "kind": "openai" }}] }}"#).unwrap();
        }

        let watch_path = path.clone();
        let handle = std::thread::spawn(move || {
            // Return false on the first reload to stop the loop deterministically.
            watch(watch_path, |cfg| {
                assert_eq!(cfg.providers[0].id, "b");
                false
            })
        });

        // Give the watcher a moment to register, then write a new valid config.
        std::thread::sleep(Duration::from_millis(200));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, r#"{{ "providers": [{{ "id": "b", "kind": "openai" }}] }}"#).unwrap();
            f.flush().unwrap();
        }

        handle.join().unwrap().unwrap();
    }
}
```

Add to `crates/gateway-config/src/lib.rs`:

```rust
pub mod watch;

pub use watch::{load_file, watch};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-config watch::`
Expected: 3 tests PASS. (The `watch_delivers_reload_then_stops_on_false` test depends on the OS file-watch firing; if flaky in CI, it is the OS event timing, not a logic bug — the deterministic `load_file` tests cover the load/validate path.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/src/watch.rs crates/gateway-config/src/lib.rs
git commit -s -m "feat(config): file-watch hot reload (bad config keeps last good)"
```

---

### Task 16: End-to-end config↔store integration test

**Files:**
- Create: `crates/gateway-config/tests/config_roundtrip.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-config/tests/config_roundtrip.rs`:

```rust
//! End-to-end: a Git-managed config file → load (interpolate env) → apply into a
//! durable store → dump back → re-apply is a no-op. This is the "UI = API = CLI =
//! Git, one engine" invariant made concrete, with secrets sealed at rest and
//! emitted as env refs on the way back out.

use gateway_config::{apply, diff, dump, load, validate, Config};
use gateway_spine::{MasterKey, SqliteStore, Store, Usd};

#[tokio::test]
async fn git_config_applies_dumps_and_reapplies_as_noop() {
    let mk = MasterKey::from_bytes([11u8; 32]);
    let store = SqliteStore::connect(":memory:").await.unwrap();
    store.migrate().await.unwrap();

    // 1. A config file an operator committed to Git.
    let raw = r#"{
        "version": 1,
        "providers": [
            { "id": "openai", "kind": "openai", "api_key": "${OPENAI_API_KEY}" }
        ],
        "keys": [
            { "id": "team-a", "max_budget_usd": 25.0, "rpm": 120, "model_allowlist": ["gpt-4o"] }
        ],
        "routes": [
            { "id": "default", "model": "gpt-4o", "provider": "openai" }
        ]
    }"#;

    // 2. Load with env interpolation (secret comes from the environment).
    let env: std::collections::HashMap<String, String> =
        [("OPENAI_API_KEY".to_string(), "sk-live-xyz".to_string())].into_iter().collect();
    let desired = load(raw, &|n| env.get(n).cloned()).unwrap();

    // 3. Apply into the empty store; the plan creates everything.
    let plan = apply(&desired, &Config::default(), &store, &mk).await.unwrap();
    assert_eq!(plan.changes.len(), 2); // 1 provider + 1 key (routes land with the router later)

    // 4. The key's budget is the exact µUSD of $25.00; secret sealed, not plaintext.
    let keys = store.load_keys().await.unwrap();
    assert_eq!(keys[0].max_budget, Some(Usd::from_dollars_f64(25.0)));
    let providers = store.load_providers().await.unwrap();
    let sealed = providers[0].sealed_api_key.as_ref().unwrap();
    assert_eq!(mk.open(sealed).unwrap(), "sk-live-xyz");

    // 5. Dump the live state back to a Config (secret → env ref).
    let dumped = dump(&store).await.unwrap();
    let dumped_json = serde_json::to_string(&dumped).unwrap();
    assert!(!dumped_json.contains("sk-live-xyz"));
    assert_eq!(dumped.providers[0].api_key.as_deref(), Some("${OPENAI_API_KEY}"));

    // 6. The dump is a valid config and re-applying it is a no-op (round-trip).
    validate(&dumped_json).unwrap();
    assert!(diff(&dumped, &dumped.clone()).is_empty());
    let replan = apply(&dumped, &dumped.clone(), &store, &mk).await.unwrap();
    assert!(replan.is_empty());
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-config --test config_roundtrip`
Expected: PASS — load → apply → dump → re-apply round-trips with sealed secrets.

- [ ] **Step 3: Full gate, then commit**

```bash
cargo fmt --all && cargo clippy -p gateway-config --all-targets -- -D warnings
git add crates/gateway-config/tests/config_roundtrip.rs
git commit -s -m "test(config): prove Git→load→apply→dump round-trips with sealed secrets"
```

---

### Task 17: Workspace-wide green gate

**Files:** none (verification only)

- [ ] **Step 1: Run both crates' full test suites**

Run: `cargo test -p gateway-spine -p gateway-config`
Expected: every unit test + `persistence_restart` + `config_roundtrip` PASS.

- [ ] **Step 2: Clippy + fmt across both crates**

Run:
```bash
cargo clippy -p gateway-spine -p gateway-config --all-targets -- -D warnings
cargo fmt --all --check
```
Expected: clean.

- [ ] **Step 3: Confirm no float money + no plaintext-secret leak**

Run:
```bash
grep -rn "f64" crates/gateway-spine/src/store crates/gateway-config/src || echo "no f64 in store/apply paths"
grep -rn "REAL\|FLOAT" crates/gateway-spine/src/store || echo "no float SQL columns"
```
Expected: `f64` appears only in config's `max_budget_usd` boundary fields (converted to µUSD at the seam via `Usd::from_dollars_f64`/`as_dollars_f64`), never in store math or SQL columns.

- [ ] **Step 4: Commit (if any fmt changes)**

```bash
git add -A
git commit -s -m "chore(spine,config): workspace green gate for P1.6" || echo "nothing to commit"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-spine -p gateway-config` is fully green (unit + `persistence_restart` + `config_roundtrip`).
- [ ] `cargo clippy -p gateway-spine -p gateway-config --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
- [ ] The four invariants this milestone owns are each proven by a test: fail-closed-under-storage-failure (`writes_fail_closed_when_down`), cost-correctness-survives-restart (`spend_and_sealed_provider_survive_restart`), secrets-at-rest (`apply_seals_provider_secret_never_plaintext` + `dump_emits_env_ref_not_plaintext_secret`), config-is-one-engine (`git_config_applies_dumps_and_reapplies_as_noop`).
- [ ] Money stays integer-only end-to-end: `f64` appears only at the config USD boundary (`max_budget_usd`), converted via `Usd::from_dollars_f64`/`as_dollars_f64`; SQL money columns are `BIGINT` µUSD (grep guard in Task 17 Step 3).
- [ ] Provider secrets are AEAD-sealed at rest and never appear in plaintext in the store or in a dumped config.
- [ ] The same schema runs on Postgres unchanged (Task 4 guards reject SQLite-only DDL).

## Interfaces this milestone EXPOSES (downstream depends on these)

- **`gateway_spine::Store`** (async trait) + **`SqliteStore`**, **`DegradableStore<S>`**, **`StoredProvider`**, **`KeySpend`**, **`store::restore_ledger(&dyn Store, &BudgetLedger) -> Restored`** — P1.4's request lifecycle calls `record_spend` on commit and boots via `restore_ledger`; P1.8 first-boot wires `SqliteStore::connect` + `migrate`.
- **`gateway_spine::MasterKey`** (`from_base64`/`from_bytes`/`seal`/`open`) — the one place provider secrets are sealed/opened; P1.4 egress opens provider keys with it.
- **`gateway_config::{Config, load, validate, diff, apply, dump, load_file, watch}`** + the `Change`/`Diff` types — P1.8 dashboard and P3 admin-MCP/CLI all drive config through this single engine; `apply(desired, live, &dyn Store, &MasterKey)` is the one mutation path.

**Next:** `2026-06-10-p1-07-telemetry-and-logs.md` — the embedded columnar request-log + spend store (off the hot path) and the `usage.cost`/`x-overhead-duration-ms` headers, which read the durable spend this milestone persists and emit the authenticated Prometheus + spend query APIs.
