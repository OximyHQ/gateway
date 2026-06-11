#![forbid(unsafe_code)]

use sqlx::postgres::PgPool;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{ReservationId, StoreError, StoredKey};
use gateway_spine::Usd;

enum PoolKind {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

pub struct Store {
    pool: PoolKind,
}

const MIGRATION_SQL: &str = include_str!("../migrations/0001_init.sql");

impl Store {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        if url.starts_with("postgres") || url.starts_with("postgresql") {
            let pool = PgPool::connect(url)
                .await
                .map_err(|e| StoreError::Db(e.to_string()))?;
            Self::run_migrations_pg(&pool).await?;
            Ok(Store {
                pool: PoolKind::Postgres(pool),
            })
        } else {
            // SQLite — support both `sqlite:<path>` and `sqlite::memory:`
            use std::str::FromStr as _;
            let raw = if url.starts_with("sqlite:") {
                url.to_string()
            } else {
                format!("sqlite:{url}")
            };
            let options = sqlx::sqlite::SqliteConnectOptions::from_str(&raw)
                .map_err(|e| StoreError::Db(e.to_string()))?
                .create_if_missing(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .busy_timeout(std::time::Duration::from_millis(30_000));

            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(16)
                .connect_with(options)
                .await
                .map_err(|e| StoreError::Db(e.to_string()))?;

            Self::run_migrations_sqlite(&pool).await?;
            Ok(Store {
                pool: PoolKind::Sqlite(pool),
            })
        }
    }

    async fn run_migrations_sqlite(pool: &SqlitePool) -> Result<(), StoreError> {
        for stmt in MIGRATION_SQL.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt)
                    .execute(pool)
                    .await
                    .map_err(|e| StoreError::Migration(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn run_migrations_pg(pool: &PgPool) -> Result<(), StoreError> {
        for stmt in MIGRATION_SQL.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt)
                    .execute(pool)
                    .await
                    .map_err(|e| StoreError::Migration(e.to_string()))?;
            }
        }
        Ok(())
    }

    // ── Durable spend ledger ──────────────────────────────────────────────────

    /// Reserve `estimate` µUSD before an upstream call. Returns a reservation id.
    /// Fail-closed: if the budget would be exceeded the reservation is rejected.
    pub async fn reserve(&self, key_id: &str, estimate: Usd) -> Result<ReservationId, StoreError> {
        let estimate_micros = estimate.micros();
        let now_ms = now_millis();
        let res_id = Uuid::new_v4().to_string();

        match &self.pool {
            PoolKind::Sqlite(pool) => {
                // BEGIN IMMEDIATE: SQLite write lock so concurrent reserves serialize
                let mut conn = pool.acquire().await?;
                sqlx::query("BEGIN IMMEDIATE")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                let key_row =
                    sqlx::query("SELECT budget_micros, spent_micros FROM keys WHERE id = ?")
                        .bind(key_id)
                        .fetch_optional(&mut *conn)
                        .await
                        .map_err(|e| StoreError::Db(e.to_string()))?;

                let key_row = match key_row {
                    Some(r) => r,
                    None => {
                        sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
                        return Err(StoreError::NotFound);
                    }
                };

                let budget_micros: Option<i64> = key_row.try_get("budget_micros")?;
                let spent_micros: i64 = key_row.try_get("spent_micros")?;

                let reserved_row = sqlx::query(
                    "SELECT COALESCE(SUM(estimate_micros), 0) as total \
                     FROM reservations WHERE key_id = ?",
                )
                .bind(key_id)
                .fetch_one(&mut *conn)
                .await
                .map_err(|e| StoreError::Db(e.to_string()))?;

                let reserved_micros: i64 = reserved_row.try_get("total")?;

                if let Some(budget) = budget_micros
                    && spent_micros + reserved_micros + estimate_micros > budget
                {
                    sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
                    return Err(StoreError::BudgetExceeded {
                        budget_micros: budget,
                        spent_micros,
                        reserved_micros,
                        requested_micros: estimate_micros,
                    });
                }

                sqlx::query(
                    "INSERT INTO reservations \
                     (id, key_id, estimate_micros, created_at_ms) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&res_id)
                .bind(key_id)
                .bind(estimate_micros)
                .bind(now_ms)
                .execute(&mut *conn)
                .await
                .map_err(|e| StoreError::Db(e.to_string()))?;

                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                Ok(res_id)
            }

            PoolKind::Postgres(pool) => {
                let mut tx = pool.begin().await?;

                let key_row = sqlx::query(
                    "SELECT budget_micros, spent_micros \
                     FROM keys WHERE id = $1 FOR UPDATE",
                )
                .bind(key_id)
                .fetch_optional(&mut *tx)
                .await?;

                let key_row = match key_row {
                    Some(r) => r,
                    None => {
                        tx.rollback().await.ok();
                        return Err(StoreError::NotFound);
                    }
                };

                let budget_micros: Option<i64> = key_row.try_get("budget_micros")?;
                let spent_micros: i64 = key_row.try_get("spent_micros")?;

                let reserved_row = sqlx::query(
                    "SELECT COALESCE(SUM(estimate_micros), 0) as total \
                     FROM reservations WHERE key_id = $1",
                )
                .bind(key_id)
                .fetch_one(&mut *tx)
                .await?;

                let reserved_micros: i64 = reserved_row.try_get("total")?;

                if let Some(budget) = budget_micros
                    && spent_micros + reserved_micros + estimate_micros > budget
                {
                    tx.rollback().await.ok();
                    return Err(StoreError::BudgetExceeded {
                        budget_micros: budget,
                        spent_micros,
                        reserved_micros,
                        requested_micros: estimate_micros,
                    });
                }

                sqlx::query(
                    "INSERT INTO reservations \
                     (id, key_id, estimate_micros, created_at_ms) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(&res_id)
                .bind(key_id)
                .bind(estimate_micros)
                .bind(now_ms)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;
                Ok(res_id)
            }
        }
    }

    /// True-up: record the real cost and release the reservation atomically.
    pub async fn commit(&self, res_id: &str, actual: Usd) -> Result<(), StoreError> {
        let actual_micros = actual.micros();

        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let mut conn = pool.acquire().await?;
                sqlx::query("BEGIN IMMEDIATE")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                let row = sqlx::query("SELECT key_id FROM reservations WHERE id = ?")
                    .bind(res_id)
                    .fetch_optional(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                let row = match row {
                    Some(r) => r,
                    None => {
                        sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
                        return Err(StoreError::NotFound);
                    }
                };

                let key_id: String = row.try_get("key_id")?;

                sqlx::query("UPDATE keys SET spent_micros = spent_micros + ? WHERE id = ?")
                    .bind(actual_micros)
                    .bind(&key_id)
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                sqlx::query("DELETE FROM reservations WHERE id = ?")
                    .bind(res_id)
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;

                Ok(())
            }

            PoolKind::Postgres(pool) => {
                let mut tx = pool.begin().await?;

                let row = sqlx::query("SELECT key_id FROM reservations WHERE id = $1 FOR UPDATE")
                    .bind(res_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(StoreError::NotFound)?;

                let key_id: String = row.try_get("key_id")?;

                sqlx::query("UPDATE keys SET spent_micros = spent_micros + $1 WHERE id = $2")
                    .bind(actual_micros)
                    .bind(&key_id)
                    .execute(&mut *tx)
                    .await?;

                sqlx::query("DELETE FROM reservations WHERE id = $1")
                    .bind(res_id)
                    .execute(&mut *tx)
                    .await?;

                tx.commit().await?;
                Ok(())
            }
        }
    }

    /// Release a reservation without billing (request failed before billing).
    pub async fn release(&self, res_id: &str) -> Result<(), StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let result = sqlx::query("DELETE FROM reservations WHERE id = ?")
                    .bind(res_id)
                    .execute(pool)
                    .await?;
                if result.rows_affected() == 0 {
                    return Err(StoreError::NotFound);
                }
                Ok(())
            }
            PoolKind::Postgres(pool) => {
                let result = sqlx::query("DELETE FROM reservations WHERE id = $1")
                    .bind(res_id)
                    .execute(pool)
                    .await?;
                if result.rows_affected() == 0 {
                    return Err(StoreError::NotFound);
                }
                Ok(())
            }
        }
    }

    /// Sweep reservations older than `ttl_ms`. Call periodically (e.g. every 60s).
    pub async fn sweep_stale_reservations(
        &self,
        ttl_ms: i64,
        now_ms: i64,
    ) -> Result<u64, StoreError> {
        let cutoff_ms = now_ms - ttl_ms;
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let result = sqlx::query("DELETE FROM reservations WHERE created_at_ms < ?")
                    .bind(cutoff_ms)
                    .execute(pool)
                    .await?;
                Ok(result.rows_affected())
            }
            PoolKind::Postgres(pool) => {
                let result = sqlx::query("DELETE FROM reservations WHERE created_at_ms < $1")
                    .bind(cutoff_ms)
                    .execute(pool)
                    .await?;
                Ok(result.rows_affected())
            }
        }
    }

    /// Total durable spend for a key.
    pub async fn spent(&self, key_id: &str) -> Result<Usd, StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let row = sqlx::query("SELECT spent_micros FROM keys WHERE id = ?")
                    .bind(key_id)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                let micros: i64 = row.try_get("spent_micros")?;
                Ok(Usd::from_micros(micros))
            }
            PoolKind::Postgres(pool) => {
                let row = sqlx::query("SELECT spent_micros FROM keys WHERE id = $1")
                    .bind(key_id)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                let micros: i64 = row.try_get("spent_micros")?;
                Ok(Usd::from_micros(micros))
            }
        }
    }

    /// Sum of in-flight reservations for a key (derived, not stored).
    pub async fn reserved(&self, key_id: &str) -> Result<Usd, StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let row = sqlx::query(
                    "SELECT COALESCE(SUM(estimate_micros), 0) as total \
                     FROM reservations WHERE key_id = ?",
                )
                .bind(key_id)
                .fetch_one(pool)
                .await?;
                let total: i64 = row.try_get("total")?;
                Ok(Usd::from_micros(total))
            }
            PoolKind::Postgres(pool) => {
                let row = sqlx::query(
                    "SELECT COALESCE(SUM(estimate_micros), 0) as total \
                     FROM reservations WHERE key_id = $1",
                )
                .bind(key_id)
                .fetch_one(pool)
                .await?;
                let total: i64 = row.try_get("total")?;
                Ok(Usd::from_micros(total))
            }
        }
    }

    // ── Key management ────────────────────────────────────────────────────────

    pub async fn upsert_key(&self, key: &StoredKey) -> Result<(), StoreError> {
        let allowlist_json = key
            .model_allowlist
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        match &self.pool {
            PoolKind::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO keys \
                     (id, name, token_hash, token_prefix, budget_micros, spent_micros, \
                      rpm, tpm, max_parallel, model_allowlist, expires_at_ms, \
                      revoked, parent_id, created_at_ms) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                       name=excluded.name, token_hash=excluded.token_hash, \
                       token_prefix=excluded.token_prefix, \
                       budget_micros=excluded.budget_micros, \
                       rpm=excluded.rpm, tpm=excluded.tpm, \
                       max_parallel=excluded.max_parallel, \
                       model_allowlist=excluded.model_allowlist, \
                       expires_at_ms=excluded.expires_at_ms, \
                       revoked=excluded.revoked, parent_id=excluded.parent_id",
                )
                .bind(&key.id)
                .bind(&key.name)
                .bind(&key.token_hash)
                .bind(&key.token_prefix)
                .bind(key.budget_micros)
                .bind(key.spent_micros)
                .bind(key.rpm)
                .bind(key.tpm)
                .bind(key.max_parallel)
                .bind(&allowlist_json)
                .bind(key.expires_at_ms)
                .bind(key.revoked as i32)
                .bind(&key.parent_id)
                .bind(key.created_at_ms)
                .execute(pool)
                .await?;
                Ok(())
            }
            PoolKind::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO keys \
                     (id, name, token_hash, token_prefix, budget_micros, spent_micros, \
                      rpm, tpm, max_parallel, model_allowlist, expires_at_ms, \
                      revoked, parent_id, created_at_ms) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
                     ON CONFLICT(id) DO UPDATE SET \
                       name=EXCLUDED.name, token_hash=EXCLUDED.token_hash, \
                       token_prefix=EXCLUDED.token_prefix, \
                       budget_micros=EXCLUDED.budget_micros, \
                       rpm=EXCLUDED.rpm, tpm=EXCLUDED.tpm, \
                       max_parallel=EXCLUDED.max_parallel, \
                       model_allowlist=EXCLUDED.model_allowlist, \
                       expires_at_ms=EXCLUDED.expires_at_ms, \
                       revoked=EXCLUDED.revoked, parent_id=EXCLUDED.parent_id",
                )
                .bind(&key.id)
                .bind(&key.name)
                .bind(&key.token_hash)
                .bind(&key.token_prefix)
                .bind(key.budget_micros)
                .bind(key.spent_micros)
                .bind(key.rpm)
                .bind(key.tpm)
                .bind(key.max_parallel)
                .bind(&allowlist_json)
                .bind(key.expires_at_ms)
                .bind(key.revoked as i32)
                .bind(&key.parent_id)
                .bind(key.created_at_ms)
                .execute(pool)
                .await?;
                Ok(())
            }
        }
    }

    pub async fn get_key(&self, id: &str) -> Result<StoredKey, StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let row = sqlx::query("SELECT * FROM keys WHERE id = ?")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                Ok(StoredKey {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    token_hash: row.try_get("token_hash")?,
                    token_prefix: row.try_get("token_prefix")?,
                    budget_micros: row.try_get("budget_micros")?,
                    spent_micros: row.try_get("spent_micros")?,
                    rpm: row.try_get("rpm")?,
                    tpm: row.try_get("tpm")?,
                    max_parallel: row.try_get("max_parallel")?,
                    model_allowlist: {
                        let s: Option<String> = row.try_get("model_allowlist")?;
                        s.and_then(|s| serde_json::from_str(&s).ok())
                    },
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    revoked: {
                        let v: i64 = row.try_get("revoked")?;
                        v != 0
                    },
                    parent_id: row.try_get("parent_id")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            }
            PoolKind::Postgres(pool) => {
                let row = sqlx::query("SELECT * FROM keys WHERE id = $1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                Ok(StoredKey {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    token_hash: row.try_get("token_hash")?,
                    token_prefix: row.try_get("token_prefix")?,
                    budget_micros: row.try_get("budget_micros")?,
                    spent_micros: row.try_get("spent_micros")?,
                    rpm: row.try_get("rpm")?,
                    tpm: row.try_get("tpm")?,
                    max_parallel: row.try_get("max_parallel")?,
                    model_allowlist: {
                        let s: Option<String> = row.try_get("model_allowlist")?;
                        s.and_then(|s| serde_json::from_str(&s).ok())
                    },
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    revoked: {
                        let v: i64 = row.try_get("revoked")?;
                        v != 0
                    },
                    parent_id: row.try_get("parent_id")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            }
        }
    }

    pub async fn list_keys(&self) -> Result<Vec<StoredKey>, StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let rows = sqlx::query("SELECT * FROM keys ORDER BY created_at_ms DESC")
                    .fetch_all(pool)
                    .await?;
                rows.iter()
                    .map(|row| {
                        Ok(StoredKey {
                            id: row.try_get("id")?,
                            name: row.try_get("name")?,
                            token_hash: row.try_get("token_hash")?,
                            token_prefix: row.try_get("token_prefix")?,
                            budget_micros: row.try_get("budget_micros")?,
                            spent_micros: row.try_get("spent_micros")?,
                            rpm: row.try_get("rpm")?,
                            tpm: row.try_get("tpm")?,
                            max_parallel: row.try_get("max_parallel")?,
                            model_allowlist: {
                                let s: Option<String> = row.try_get("model_allowlist")?;
                                s.and_then(|s| serde_json::from_str(&s).ok())
                            },
                            expires_at_ms: row.try_get("expires_at_ms")?,
                            revoked: {
                                let v: i64 = row.try_get("revoked")?;
                                v != 0
                            },
                            parent_id: row.try_get("parent_id")?,
                            created_at_ms: row.try_get("created_at_ms")?,
                        })
                    })
                    .collect()
            }
            PoolKind::Postgres(pool) => {
                let rows = sqlx::query("SELECT * FROM keys ORDER BY created_at_ms DESC")
                    .fetch_all(pool)
                    .await?;
                rows.iter()
                    .map(|row| {
                        Ok(StoredKey {
                            id: row.try_get("id")?,
                            name: row.try_get("name")?,
                            token_hash: row.try_get("token_hash")?,
                            token_prefix: row.try_get("token_prefix")?,
                            budget_micros: row.try_get("budget_micros")?,
                            spent_micros: row.try_get("spent_micros")?,
                            rpm: row.try_get("rpm")?,
                            tpm: row.try_get("tpm")?,
                            max_parallel: row.try_get("max_parallel")?,
                            model_allowlist: {
                                let s: Option<String> = row.try_get("model_allowlist")?;
                                s.and_then(|s| serde_json::from_str(&s).ok())
                            },
                            expires_at_ms: row.try_get("expires_at_ms")?,
                            revoked: {
                                let v: i64 = row.try_get("revoked")?;
                                v != 0
                            },
                            parent_id: row.try_get("parent_id")?,
                            created_at_ms: row.try_get("created_at_ms")?,
                        })
                    })
                    .collect()
            }
        }
    }

    pub async fn revoke_key(&self, id: &str) -> Result<(), StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let result = sqlx::query("UPDATE keys SET revoked = 1 WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                if result.rows_affected() == 0 {
                    return Err(StoreError::NotFound);
                }
                Ok(())
            }
            PoolKind::Postgres(pool) => {
                let result = sqlx::query("UPDATE keys SET revoked = 1 WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
                if result.rows_affected() == 0 {
                    return Err(StoreError::NotFound);
                }
                Ok(())
            }
        }
    }

    pub async fn find_by_token_hash(&self, hash: &str) -> Result<StoredKey, StoreError> {
        match &self.pool {
            PoolKind::Sqlite(pool) => {
                let row = sqlx::query("SELECT * FROM keys WHERE token_hash = ?")
                    .bind(hash)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                Ok(StoredKey {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    token_hash: row.try_get("token_hash")?,
                    token_prefix: row.try_get("token_prefix")?,
                    budget_micros: row.try_get("budget_micros")?,
                    spent_micros: row.try_get("spent_micros")?,
                    rpm: row.try_get("rpm")?,
                    tpm: row.try_get("tpm")?,
                    max_parallel: row.try_get("max_parallel")?,
                    model_allowlist: {
                        let s: Option<String> = row.try_get("model_allowlist")?;
                        s.and_then(|s| serde_json::from_str(&s).ok())
                    },
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    revoked: {
                        let v: i64 = row.try_get("revoked")?;
                        v != 0
                    },
                    parent_id: row.try_get("parent_id")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            }
            PoolKind::Postgres(pool) => {
                let row = sqlx::query("SELECT * FROM keys WHERE token_hash = $1")
                    .bind(hash)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                Ok(StoredKey {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    token_hash: row.try_get("token_hash")?,
                    token_prefix: row.try_get("token_prefix")?,
                    budget_micros: row.try_get("budget_micros")?,
                    spent_micros: row.try_get("spent_micros")?,
                    rpm: row.try_get("rpm")?,
                    tpm: row.try_get("tpm")?,
                    max_parallel: row.try_get("max_parallel")?,
                    model_allowlist: {
                        let s: Option<String> = row.try_get("model_allowlist")?;
                        s.and_then(|s| serde_json::from_str(&s).ok())
                    },
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    revoked: {
                        let v: i64 = row.try_get("revoked")?;
                        v != 0
                    },
                    parent_id: row.try_get("parent_id")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            }
        }
    }
}

pub(crate) fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_test_key(id: &str, budget_micros: Option<i64>) -> StoredKey {
        StoredKey {
            id: id.to_string(),
            name: format!("key-{}", id),
            token_hash: format!("hash-{}", id),
            token_prefix: "sk-ox".to_string(),
            budget_micros,
            spent_micros: 0,
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
            expires_at_ms: None,
            revoked: false,
            parent_id: None,
            created_at_ms: now_millis(),
        }
    }

    /// Open an in-memory SQLite store (`:memory:` path).
    async fn mem_store() -> Store {
        Store::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_concurrency_no_overspend() {
        // Use a temp file so all tasks share the same on-disk WAL-mode DB.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let url = format!("sqlite:{}", path.to_str().unwrap());

        let store = Arc::new(Store::connect(&url).await.unwrap());
        let key = make_test_key("key1", Some(1_000_000)); // $1.00 budget
        store.upsert_key(&key).await.unwrap();

        // 200 concurrent tasks each try to reserve $0.10 (100_000 micros)
        // Only 10 should succeed
        let handles: Vec<_> = (0..200)
            .map(|_| {
                let store = store.clone();
                tokio::spawn(async move { store.reserve("key1", Usd::from_micros(100_000)).await })
            })
            .collect();

        let mut successes = 0u32;
        for h in handles {
            if h.await.unwrap().is_ok() {
                successes += 1;
            }
        }

        assert_eq!(
            successes, 10,
            "exactly 10 of 200 tasks should succeed with $1.00 budget at $0.10 each"
        );
    }

    #[tokio::test]
    async fn test_durability_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("durable.db");
        let url = format!("sqlite:{}", path.to_str().unwrap());

        {
            let store = Store::connect(&url).await.unwrap();
            let key = make_test_key("key2", Some(10_000_000));
            store.upsert_key(&key).await.unwrap();
            let res_id = store
                .reserve("key2", Usd::from_micros(500_000))
                .await
                .unwrap();
            store
                .commit(&res_id, Usd::from_micros(450_000))
                .await
                .unwrap();
        }

        // Reopen
        let store2 = Store::connect(&url).await.unwrap();
        let spent = store2.spent("key2").await.unwrap();
        assert_eq!(
            spent.micros(),
            450_000,
            "spent should persist across reopen"
        );
    }

    #[tokio::test]
    async fn test_crash_recovery_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sweep.db");
        let url = format!("sqlite:{}", path.to_str().unwrap());

        let store = Store::connect(&url).await.unwrap();
        let key = make_test_key("key3", Some(10_000_000));
        store.upsert_key(&key).await.unwrap();

        let now = now_millis();
        let stale_created_at = now - 10_000; // 10 seconds old
        let ttl_ms = 5_000; // 5 second TTL

        // Manually insert a stale reservation
        match &store.pool {
            PoolKind::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO reservations \
                     (id, key_id, estimate_micros, created_at_ms) \
                     VALUES ('stale-res', 'key3', 100000, ?)",
                )
                .bind(stale_created_at)
                .execute(pool)
                .await
                .unwrap();
            }
            PoolKind::Postgres(_) => {}
        }

        // Verify reserved = 100000
        let reserved = store.reserved("key3").await.unwrap();
        assert_eq!(reserved.micros(), 100_000);

        // Sweep stale
        let swept = store.sweep_stale_reservations(ttl_ms, now).await.unwrap();
        assert_eq!(swept, 1);

        // Reserved should now be 0
        let reserved = store.reserved("key3").await.unwrap();
        assert_eq!(reserved.micros(), 0);

        // Add a fresh reservation
        let fresh_res = store
            .reserve("key3", Usd::from_micros(50_000))
            .await
            .unwrap();

        // Sweep again — fresh reservation (created just now) should survive
        let swept2 = store.sweep_stale_reservations(ttl_ms, now).await.unwrap();
        assert_eq!(swept2, 0, "fresh reservation should not be swept");

        let reserved_after = store.reserved("key3").await.unwrap();
        assert_eq!(reserved_after.micros(), 50_000);

        store.release(&fresh_res).await.unwrap();
    }

    #[tokio::test]
    async fn test_fail_closed_budget_exceeded() {
        let store = mem_store().await;
        let key = make_test_key("key4", Some(100_000)); // $0.10 budget
        store.upsert_key(&key).await.unwrap();

        // Try to reserve more than budget
        let result = store.reserve("key4", Usd::from_micros(200_000)).await;
        assert!(matches!(result, Err(StoreError::BudgetExceeded { .. })));

        // Verify no reservation was created
        let reserved = store.reserved("key4").await.unwrap();
        assert_eq!(reserved.micros(), 0);
    }

    #[tokio::test]
    async fn test_key_roundtrip() {
        let store = mem_store().await;
        let key = StoredKey {
            id: "k1".into(),
            name: "test key".into(),
            token_hash: "abc123".into(),
            token_prefix: "sk-ox".into(),
            budget_micros: Some(5_000_000),
            spent_micros: 0,
            rpm: Some(100),
            tpm: Some(50_000),
            max_parallel: Some(5),
            model_allowlist: Some(vec!["gpt-4o".into(), "claude-3-5-sonnet".into()]),
            expires_at_ms: None,
            revoked: false,
            parent_id: None,
            created_at_ms: now_millis(),
        };
        store.upsert_key(&key).await.unwrap();

        let loaded = store.get_key("k1").await.unwrap();
        assert_eq!(loaded.id, key.id);
        assert_eq!(loaded.name, key.name);
        assert_eq!(loaded.budget_micros, key.budget_micros);
        assert_eq!(loaded.rpm, key.rpm);
        assert_eq!(
            loaded.model_allowlist,
            Some(vec!["gpt-4o".into(), "claude-3-5-sonnet".into()])
        );
    }

    #[tokio::test]
    async fn test_find_by_token_hash() {
        let store = mem_store().await;
        let key = make_test_key("k5", None);
        store.upsert_key(&key).await.unwrap();

        let found = store.find_by_token_hash("hash-k5").await.unwrap();
        assert_eq!(found.id, "k5");

        let not_found = store.find_by_token_hash("nonexistent").await;
        assert!(matches!(not_found, Err(StoreError::NotFound)));
    }

    #[tokio::test]
    async fn test_revoke_key() {
        let store = mem_store().await;
        store.upsert_key(&make_test_key("k6", None)).await.unwrap();

        store.revoke_key("k6").await.unwrap();
        let loaded = store.get_key("k6").await.unwrap();
        assert!(loaded.revoked);

        let err = store.revoke_key("nonexistent").await;
        assert!(matches!(err, Err(StoreError::NotFound)));
    }
}
