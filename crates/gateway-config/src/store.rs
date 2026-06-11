//! Config-plane storage abstraction. The `ConfigStore` trait backs the apply/dump
//! engine; the `MemConfigStore` is a test double. The SQLite implementation
//! (`SqliteConfigStore`) is the default. Gateway-spine's full `Store` trait (with
//! spend tracking, boot restore, degraded mode) is a separate, later milestone
//! (P1.6 spine tasks) and is NOT implemented here.

use async_trait::async_trait;

use crate::error::ConfigError;

/// A stored provider row. The `sealed_api_key` is AEAD ciphertext — never plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProvider {
    pub id: String,
    pub kind: String,
    pub base_url: Option<String>,
    /// AEAD-sealed API key (`MasterKey::seal` output), or `None` for keyless.
    pub sealed_api_key: Option<String>,
}

/// A stored virtual key row (config-plane view: budgets + limits, no secret hash).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredKey {
    pub id: String,
    /// Budget in micro-dollars (µUSD); `None` = unlimited.
    pub max_budget_micros: Option<i64>,
    pub rpm: Option<i64>,
    pub tpm: Option<i64>,
    pub max_parallel: Option<i64>,
    pub model_allowlist: Option<Vec<String>>,
    pub revoked: bool,
}

/// Storage-agnostic config-plane persistence. All writes are fail-closed.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// Apply pending schema migrations (idempotent).
    async fn migrate(&self) -> Result<(), ConfigError>;

    /// Persist (insert or replace) a provider.
    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), ConfigError>;

    /// Load every provider record.
    async fn load_providers(&self) -> Result<Vec<StoredProvider>, ConfigError>;

    /// Persist (insert or replace) a virtual key.
    async fn upsert_key(&self, key: &StoredKey) -> Result<(), ConfigError>;

    /// Load every virtual key.
    async fn load_keys(&self) -> Result<Vec<StoredKey>, ConfigError>;

    /// Mark a key revoked (never hard-delete — spend history must survive).
    async fn revoke_key(&self, key_id: &str) -> Result<(), ConfigError>;

    /// Cheap liveness probe.
    async fn ping(&self) -> Result<(), ConfigError>;
}

// ── In-memory test double ────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Mutex;

/// Pure-in-memory `ConfigStore` for tests. Not persistent.
#[derive(Default)]
pub struct MemConfigStore {
    providers: Mutex<HashMap<String, StoredProvider>>,
    keys: Mutex<HashMap<String, StoredKey>>,
}

#[async_trait]
impl ConfigStore for MemConfigStore {
    async fn migrate(&self) -> Result<(), ConfigError> {
        Ok(())
    }

    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), ConfigError> {
        self.providers
            .lock()
            .unwrap()
            .insert(provider.id.clone(), provider.clone());
        Ok(())
    }

    async fn load_providers(&self) -> Result<Vec<StoredProvider>, ConfigError> {
        Ok(self.providers.lock().unwrap().values().cloned().collect())
    }

    async fn upsert_key(&self, key: &StoredKey) -> Result<(), ConfigError> {
        self.keys
            .lock()
            .unwrap()
            .insert(key.id.clone(), key.clone());
        Ok(())
    }

    async fn load_keys(&self) -> Result<Vec<StoredKey>, ConfigError> {
        Ok(self.keys.lock().unwrap().values().cloned().collect())
    }

    async fn revoke_key(&self, key_id: &str) -> Result<(), ConfigError> {
        if let Some(k) = self.keys.lock().unwrap().get_mut(key_id) {
            k.revoked = true;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}

// ── SQLite implementation ────────────────────────────────────────────────────

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

fn db_err(e: sqlx::Error) -> ConfigError {
    ConfigError::Storage {
        detail: e.to_string(),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The default `ConfigStore` impl. SQLite via sqlx (`runtime-tokio`).
pub struct SqliteConfigStore {
    pool: SqlitePool,
}

impl SqliteConfigStore {
    /// Open a pool. `url` is a file path or `:memory:`.
    pub async fn connect(url: &str) -> Result<Self, ConfigError> {
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

const INIT_SQL: &str = "\
CREATE TABLE IF NOT EXISTS schema_migrations (
    id    BIGINT PRIMARY KEY,
    name  TEXT NOT NULL,
    applied_at_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS config_providers (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    base_url        TEXT,
    sealed_api_key  TEXT,
    updated_at_ms   BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS config_keys (
    id                  TEXT PRIMARY KEY,
    max_budget_micros   BIGINT,
    rpm                 BIGINT,
    tpm                 BIGINT,
    max_parallel        BIGINT,
    model_allowlist     TEXT,
    revoked             BIGINT NOT NULL DEFAULT 0,
    updated_at_ms       BIGINT NOT NULL
);";

#[async_trait]
impl ConfigStore for SqliteConfigStore {
    async fn migrate(&self) -> Result<(), ConfigError> {
        // Create the tracking table first if it doesn't exist yet.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations \
             (id BIGINT PRIMARY KEY, name TEXT NOT NULL, applied_at_ms BIGINT NOT NULL)",
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

        if !applied.contains(&1) {
            for stmt in INIT_SQL.split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                sqlx::query(stmt)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
            }
            sqlx::query("INSERT INTO schema_migrations (id, name, applied_at_ms) VALUES (?, ?, ?)")
                .bind(1_i64)
                .bind("init")
                .bind(now_ms())
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    async fn upsert_provider(&self, provider: &StoredProvider) -> Result<(), ConfigError> {
        sqlx::query(
            "INSERT INTO config_providers (id, kind, base_url, sealed_api_key, updated_at_ms)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                kind=excluded.kind, base_url=excluded.base_url,
                sealed_api_key=excluded.sealed_api_key, updated_at_ms=excluded.updated_at_ms",
        )
        .bind(&provider.id)
        .bind(&provider.kind)
        .bind(&provider.base_url)
        .bind(&provider.sealed_api_key)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn load_providers(&self) -> Result<Vec<StoredProvider>, ConfigError> {
        let rows = sqlx::query("SELECT id, kind, base_url, sealed_api_key FROM config_providers")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
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

    async fn upsert_key(&self, key: &StoredKey) -> Result<(), ConfigError> {
        let allowlist = key
            .model_allowlist
            .as_ref()
            .map(|l| serde_json::to_string(l).unwrap_or_default());
        sqlx::query(
            "INSERT INTO config_keys
                (id, max_budget_micros, rpm, tpm, max_parallel, model_allowlist, revoked, updated_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                max_budget_micros=excluded.max_budget_micros, rpm=excluded.rpm,
                tpm=excluded.tpm, max_parallel=excluded.max_parallel,
                model_allowlist=excluded.model_allowlist, revoked=excluded.revoked,
                updated_at_ms=excluded.updated_at_ms",
        )
        .bind(&key.id)
        .bind(key.max_budget_micros)
        .bind(key.rpm)
        .bind(key.tpm)
        .bind(key.max_parallel)
        .bind(allowlist)
        .bind(i64::from(key.revoked))
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn load_keys(&self) -> Result<Vec<StoredKey>, ConfigError> {
        let rows = sqlx::query(
            "SELECT id, max_budget_micros, rpm, tpm, max_parallel, model_allowlist, revoked \
             FROM config_keys",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let allowlist_json: Option<String> = r.get("model_allowlist");
            let model_allowlist = allowlist_json
                .as_deref()
                .map(|s| serde_json::from_str(s).unwrap_or_default());
            out.push(StoredKey {
                id: r.get("id"),
                max_budget_micros: r.get("max_budget_micros"),
                rpm: r.get("rpm"),
                tpm: r.get("tpm"),
                max_parallel: r.get("max_parallel"),
                model_allowlist,
                revoked: r.get::<i64, _>("revoked") != 0,
            });
        }
        Ok(out)
    }

    async fn revoke_key(&self, key_id: &str) -> Result<(), ConfigError> {
        sqlx::query("UPDATE config_keys SET revoked = 1, updated_at_ms = ? WHERE id = ?")
            .bind(now_ms())
            .bind(key_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn ping(&self) -> Result<(), ConfigError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_sqlite() -> SqliteConfigStore {
        let s = SqliteConfigStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn mem_store_is_object_safe_and_roundtrips() {
        let store: Box<dyn ConfigStore> = Box::new(MemConfigStore::default());
        store.migrate().await.unwrap();
        store
            .upsert_provider(&StoredProvider {
                id: "openai".into(),
                kind: "openai".into(),
                base_url: None,
                sealed_api_key: Some("sealed".into()),
            })
            .await
            .unwrap();
        let providers = store.load_providers().await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].sealed_api_key.as_deref(), Some("sealed"));
    }

    #[tokio::test]
    async fn sqlite_migrate_is_idempotent() {
        let s = fresh_sqlite().await;
        s.migrate().await.unwrap(); // second call is a no-op
    }

    #[tokio::test]
    async fn sqlite_provider_roundtrips() {
        let s = fresh_sqlite().await;
        let p = StoredProvider {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            sealed_api_key: Some("opaque-ct".into()),
        };
        s.upsert_provider(&p).await.unwrap();
        let loaded = s.load_providers().await.unwrap();
        assert_eq!(loaded, vec![p]);
    }

    #[tokio::test]
    async fn sqlite_key_roundtrips_with_allowlist() {
        let s = fresh_sqlite().await;
        let k = StoredKey {
            id: "k1".into(),
            max_budget_micros: Some(12_500_000),
            rpm: Some(100),
            tpm: None,
            max_parallel: None,
            model_allowlist: Some(vec!["gpt-4o".into()]),
            revoked: false,
        };
        s.upsert_key(&k).await.unwrap();
        let loaded = s.load_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].max_budget_micros, Some(12_500_000));
        assert_eq!(
            loaded[0].model_allowlist.as_deref(),
            Some(["gpt-4o".to_string()].as_slice())
        );
    }

    #[tokio::test]
    async fn sqlite_revoke_sets_flag() {
        let s = fresh_sqlite().await;
        s.upsert_key(&StoredKey {
            id: "k1".into(),
            max_budget_micros: None,
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
            revoked: false,
        })
        .await
        .unwrap();
        s.revoke_key("k1").await.unwrap();
        assert!(s.load_keys().await.unwrap()[0].revoked);
    }

    #[tokio::test]
    async fn sqlite_ping_ok() {
        let s = fresh_sqlite().await;
        s.ping().await.unwrap();
    }
}
