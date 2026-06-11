# Oximy Gateway — Persistence & Durable Spend Design

**Date:** 2026-06-11
**Status:** Design / approved-direction
**Motivation:** Budgets and usage are currently in-memory and lost on restart. For a gateway that *enforces money*, the store must be the durable source of truth. This is the foundational infra layer the design doc (`2026-06-10-oximy-gateway-design.md` §3) always pointed at: SQLite-default → Postgres-upgrade, embedded analytics.

---

## 1. Principle

**The database is the source of truth for money, written synchronously.** A SQLite WAL write is sub-millisecond; the LLM call it guards takes 100ms–10s. So full correctness costs ~0.01% latency — there is no reason to be eventually-consistent about budgets. The in-memory `BudgetLedger` becomes at most a read cache; the DB is authoritative.

Invariants preserved from the spine (now *durably*): fail-closed budgets, no overspend under concurrency, no double-billing, cost-correctness — and now **no spend loss across restarts/crashes**.

## 2. Three storage concerns, one trait

A single `StorageBackend` (async, `sqlx`) with two impls — **SQLite** (default, embedded, one `gateway.db`) and **Postgres** (`DATABASE_URL`, HA/multi-replica). The SQL below is written to run on both.

| Concern | Tables | Notes |
|---|---|---|
| Control plane | `keys`, `providers`, `routes`, `guardrail_attachments`, `teams`, `meta` | relational, low-volume; provider secrets AEAD-encrypted at rest |
| Spend ledger | `keys.spent_micros`, `keys.budget_micros`, `reservations` | atomic SQL; the heart of correctness |
| Telemetry | `request_log`, `usage_rollup_*` | append-only + rollups + retention; async off hot path |

## 3. The durable spend ledger (the core)

### Schema (money in integer µUSD)
```sql
CREATE TABLE keys (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  token_hash   TEXT NOT NULL,
  token_prefix TEXT NOT NULL,
  budget_micros INTEGER,            -- NULL = unlimited
  spent_micros  INTEGER NOT NULL DEFAULT 0,
  rpm INTEGER, tpm INTEGER, max_parallel INTEGER,
  model_allowlist TEXT,             -- JSON array or NULL=all
  expires_at_ms INTEGER, revoked INTEGER NOT NULL DEFAULT 0,
  parent_id TEXT, created_at_ms INTEGER NOT NULL
);
CREATE TABLE reservations (
  id TEXT PRIMARY KEY,              -- uuid
  key_id TEXT NOT NULL,
  estimate_micros INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX idx_res_key ON reservations(key_id);
```

`reserved` for a key is **derived**: `SELECT COALESCE(SUM(estimate_micros),0) FROM reservations WHERE key_id=?`. This makes crash recovery trivial (no `reserved` counter to drift).

### Reserve (fail-closed, atomic, before the upstream call)
One transaction:
```sql
BEGIN IMMEDIATE;                                  -- SQLite: write lock; PG: default
-- budget check INSIDE the txn so concurrent reserves serialize
SELECT budget_micros, spent_micros,
       (SELECT COALESCE(SUM(estimate_micros),0) FROM reservations WHERE key_id=:id) AS reserved
  FROM keys WHERE id=:id;
-- in app: if budget IS NOT NULL AND spent+reserved+:est > budget  → ROLLBACK, return BudgetExceeded
INSERT INTO reservations(id,key_id,estimate_micros,created_at_ms) VALUES(:uuid,:id,:est,:now);
COMMIT;
```
`BEGIN IMMEDIATE` (SQLite) / row-lock (Postgres `SELECT … FOR UPDATE`) makes the read-check-insert atomic, so two concurrent requests can't both pass a budget they jointly exceed. Returns the reservation id.

### Commit (true-up from real usage, durable)
```sql
BEGIN IMMEDIATE;
UPDATE keys SET spent_micros = spent_micros + :actual WHERE id=:id;
DELETE FROM reservations WHERE id=:res_id;
COMMIT;
```

### Release (request failed before billing)
```sql
DELETE FROM reservations WHERE id=:res_id;
```

### Crash recovery — the robustness piece
A request that dies mid-flight (process crash, killed connection) leaves a `reservations` row. A **sweeper** (every 60s) deletes reservations older than a TTL (default 5 min — longer than any real request): `DELETE FROM reservations WHERE created_at_ms < :now - :ttl`. Because `reserved` is derived from this table, stale holds self-heal. No counter to reconcile, no double-spend. On boot there is nothing to "restore" — `spent_micros` is already durable in `keys`, and the sweeper clears any orphaned reservations.

**Multi-replica:** the exact same SQL on Postgres is correct across N gateway nodes — the DB row is the single point of serialization. No gossip, no distributed counter needed for budgets. (Rate limits — RPM/TPM — are softer and may stay per-node or use Redis; budgets, which are money, go through the DB.)

## 4. Control plane

`keys` is the same table; providers/routes/config persist alongside. Provider API keys are **AEAD-encrypted** (`XChaCha20-Poly1305`, key from `OXIMY_MASTER_KEY` env or a KMS ref) — never plaintext at rest. Schema migrations via `sqlx::migrate!` (embedded, versioned). The `keys` CLI and the dashboard admin API both operate on this store (one source of truth, replacing the JSON `state_file`).

## 5. Telemetry persistence

`request_log` (ts, key_id, model, provider, tokens, cost_micros, latency, status, cache_status, …) written **async, batched, off the hot path** (bounded channel → batch INSERT). Periodic **rollup** into `usage_rollup_by_model_day` / `_by_key_day` for the dashboard Usage/Overview queries, plus **retention** pruning of raw rows (default keep 30 days raw, rollups forever). Scale path: an export adapter streams the same rows to **ClickHouse** (or Oximy's OTEL substrate) — default off.

## 6. Deployment & degraded mode

- **Default:** embedded SQLite at `<data_dir>/gateway.db`, WAL mode, zero setup. `oximy-gateway up` just works and is now durable.
- **Scale:** `DATABASE_URL=postgres://…` → control plane + ledger on Postgres (HA). Optional `REDIS_URL` (rate-limit counters), `CLICKHOUSE_URL` (analytics export).
- **DB unreachable (degraded):** budgeted requests **fail-closed** (can't verify budget → reject, never risk overspend); unlimited keys may serve from the cached key set. Surface DB health on `/health` + a dashboard banner.
- **Backups:** SQLite is one file (`cp`/litestream-friendly); Postgres via standard tooling.

## 7. Build stages (correctness-critical, sequenced not fanned-out)

1. **`gateway-store` crate** (or extend `gateway-config::store`): `StorageBackend` trait + SQLite impl, the atomic reserve/commit/release SQL, `reservations` + sweeper, migrations. Tests: concurrency (no overspend) + a *durability* test (write spend, drop the pool, reopen, assert spend persisted) + crash-recovery (orphan reservation swept).
2. **Rework the spine ledger seam** so `gateway-control` uses the durable store as the budget authority (the in-memory `BudgetLedger` is kept only as an optional read cache or removed from the hot path). Update the request lifecycle to reserve/commit against the store.
3. **Wire the binary**: open `gateway.db` (or `DATABASE_URL`) on boot; keys/providers/spend all from the store; remove the JSON `state_file` as source of truth (migrate any existing file once). `keys` CLI + admin API → the store.
4. **Durable telemetry** + rollups + retention; admin `/usage` `/logs` read from the store. AEAD secrets. Postgres impl + `DATABASE_URL`. Degraded mode + `/health` DB status.

Each stage gates on `cargo test --workspace` + clippy + fmt and a live boot proving spend **survives a restart**.
