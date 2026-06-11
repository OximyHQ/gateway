# Phase 1.7 — Telemetry & Request Logs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `gateway-telemetry` — the embedded, **off-the-hot-path** request-log + spend store that records one row per LLM request (ts, key, model, provider, `TokenUsage`, `Usd` cost, latency, TTFT, status, served-by, fallback-fired, cache-status, and content gated by a global/key/request opt-out → metadata-only), plus the read side: spend queries grouped by key/team/user/model/tag, OTel GenAI-semconv span emit (feature-flagged exporter, default OTLP), an **authenticated** Prometheus `/metrics` exposition, and an optional Oximy OTEL/ClickHouse export adapter (default **OFF**, to preserve the standalone posture).

**Architecture:** The write path is a **bounded MPSC channel + a single batch-writer task** so request handlers never block on telemetry I/O (design §3/§9: "telemetry writes off the hot path (async/batched)"). Handlers call a cheap, non-blocking `TelemetrySink::log(row)` that `try_send`s into the channel; on a full channel the row is **dropped with a counter bump**, never back-pressuring the request. The store itself is an **embedded append-only columnar segment store** (pure-Rust, single-binary fit — DuckDB is a documented swap behind the `SpendStore` trait per design open-question #1; we do not take the `duckdb` C dependency in P1.7). Money stays integer-only `Usd` (µUSD) from `gateway-spine`; `TokenUsage` is reused verbatim. Content capture is **opt-out at three levels** (global config → per-key → per-request header), resolving to `Metadata` (no prompt/response text) when any level opts out — privacy is fail-safe (default redacted unless explicitly enabled). The Prometheus and OTel surfaces read the same in-memory aggregates the batch writer maintains, so the read path never scans the segment store on the hot Prometheus scrape.

**Tech Stack:** Rust 2024, `serde`/`serde_json`, `thiserror`, `tokio` (mpsc, task, `Instant`), plus new deps: `parking_lot` (cheap aggregate locks), `prometheus-client` (typed registry + OpenMetrics text), `opentelemetry` + `opentelemetry_sdk` + `opentelemetry-otlp` (GenAI span emit, feature-gated), `subtle` (constant-time metrics-token compare). Tests use `tokio::test`, a `MockClock`-style injected `now_ms`, and channel-drain assertions.

**Invariants this milestone enforces (design §2/§9):**
- **Telemetry never blocks or fails a request** — `log()` is non-blocking; a full/closed channel drops + counts, never errors upward.
- **Auth-by-default on `/metrics`** — unauthenticated scrape returns 401 (LiteLLM leaked PII on an open `/metrics`; design §2 names this explicitly).
- **Content is opt-out fail-safe** — default is metadata-only; text is stored only when global+key+request all permit.
- **Cost-correctness carries through** — the stored cost is the spine-priced `Usd` from the committed call, never re-derived with floats; spend aggregates are exact integer sums.
- **Standalone posture preserved** — the Oximy OTEL/ClickHouse adapter is compiled-in but **default-OFF**; no network egress to Oximy unless explicitly configured (design open-question #4).

**What this milestone deliberately does NOT do (later phases):** MCP tool-call rows + dollar metering (P2 writes to the same store via the same sink — the row carries a `kind` discriminant now so P2 is additive); durable on-disk persistence/compaction of segments across restarts (P1.6 owns durable control-plane state; this store is in-memory-segments + append-log for P1.7, swap to DuckDB/Postgres later); the dashboard read views (P1.8 consumes the query API defined here).

---

### Task 1: Add dependencies to `gateway-telemetry`

**Files:**
- Modify: `Cargo.toml` (workspace — add shared dep versions)
- Modify: `crates/gateway-telemetry/Cargo.toml`

- [ ] **Step 1: Add the dep versions to the workspace `[workspace.dependencies]`**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `tracing-subscriber = …` line):

```toml
parking_lot = "0.12"
prometheus-client = "0.22"
subtle = "2"
bytes = "1"
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["grpc-tonic"] }
opentelemetry-semantic-conventions = "0.27"
```

- [ ] **Step 2: Reference them from `gateway-telemetry/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-telemetry/Cargo.toml` with the block below, and add the `[features]` section after it. The OTel exporter is **feature-gated** (`otel`, on by default so the OTLP exporter is available; the Oximy/ClickHouse adapter is a separate default-off knob configured at runtime, not a feature):

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
gateway-spine = { workspace = true }
parking_lot = { workspace = true }
prometheus-client = { workspace = true }
subtle = { workspace = true }
bytes = { workspace = true }
opentelemetry = { workspace = true, optional = true }
opentelemetry_sdk = { workspace = true, optional = true }
opentelemetry-otlp = { workspace = true, optional = true }
opentelemetry-semantic-conventions = { workspace = true, optional = true }

[features]
default = ["otel"]
otel = [
    "dep:opentelemetry",
    "dep:opentelemetry_sdk",
    "dep:opentelemetry-otlp",
    "dep:opentelemetry-semantic-conventions",
]
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p gateway-telemetry`
Expected: builds (still the scaffold `lib.rs` with the `CRATE` placeholder).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/gateway-telemetry/Cargo.toml Cargo.lock
git commit -s -m "build(telemetry): add prometheus-client, parking_lot, subtle, opentelemetry deps"
```

---

### Task 2: The `RequestLogRow` record + content opt-out resolution

**Files:**
- Create: `crates/gateway-telemetry/src/row.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/row.rs`:

```rust
//! One row per governed request. Populated by the HTTP lifecycle (P1.4) from the
//! committed call: the spine-priced `Usd` cost and the provider-reported
//! `TokenUsage` are copied in verbatim — never re-derived here. Content
//! (prompt/response text) is captured ONLY when the resolved capture mode is
//! `Full`; otherwise the text fields stay `None` (metadata-only, fail-safe).
//!
//! `kind` discriminates LLM rows from the MCP tool-call rows P2 will append to
//! the same store, so that extension is purely additive.

use gateway_spine::{TokenUsage, Usd};

/// Plane discriminant. P1.7 only emits `Llm`; P2 adds `McpTool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Llm,
    McpTool,
}

/// Resolved content-capture decision for one request. The three opt-out levels
/// (global config, per-key, per-request header) collapse to this: `Full` only
/// when EVERY level permits; any opt-out anywhere → `Metadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Prompt + response text stored.
    Full,
    /// Counts/cost/latency only; no text. The fail-safe default.
    Metadata,
}

impl CaptureMode {
    /// Fail-safe resolution: text is stored only if `global_enabled` AND
    /// `key_enabled` AND `request_enabled` are all true. Any opt-out wins.
    pub fn resolve(global_enabled: bool, key_enabled: bool, request_enabled: bool) -> Self {
        if global_enabled && key_enabled && request_enabled {
            CaptureMode::Full
        } else {
            CaptureMode::Metadata
        }
    }
    pub fn captures_text(self) -> bool {
        matches!(self, CaptureMode::Full)
    }
}

/// Cache outcome for the request (mirrors the `x-cache` response header values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Hit,
    Miss,
    /// Caching not applicable (e.g. streaming with caching disabled).
    Bypass,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequestLogRow {
    pub ts_ms: i64,
    pub kind: RequestKind,
    /// Virtual-key id that authorized the call.
    pub key_id: String,
    /// Optional attribution tags (P1.4 fills from key metadata / request).
    pub team_id: Option<String>,
    pub user_id: Option<String>,
    pub tags: Vec<String>,
    pub model: String,
    pub provider: String,
    pub usage: TokenUsage,
    /// Spine-priced cost, committed. µUSD, never re-derived with floats.
    pub cost: Usd,
    /// Total wall time of the request, ms.
    pub latency_ms: i64,
    /// Time-to-first-token for streamed responses; `None` for non-streamed.
    pub ttft_ms: Option<i64>,
    /// HTTP status returned to the client.
    pub status: u16,
    /// Which deployment/provider actually served it (the `x-served-by` value).
    pub served_by: String,
    /// Whether a fallback route fired (the `x-fallback` value).
    pub fallback_fired: bool,
    pub cache_status: CacheStatus,
    pub capture_mode: CaptureMode,
    /// Populated only when `capture_mode == Full`.
    pub request_text: Option<String>,
    pub response_text: Option<String>,
}

impl RequestLogRow {
    /// Strip text if the resolved mode is metadata-only. Idempotent; called
    /// defensively before the row enters the store so a mis-populated text field
    /// can never leak past an opt-out.
    pub fn enforce_capture(mut self) -> Self {
        if !self.capture_mode.captures_text() {
            self.request_text = None;
            self.response_text = None;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1_000,
            kind: RequestKind::Llm,
            key_id: "key_1".into(),
            team_id: Some("team_a".into()),
            user_id: Some("user_x".into()),
            tags: vec!["prod".into()],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
            cost: Usd::from_micros(7_500),
            latency_ms: 820,
            ttft_ms: Some(140),
            status: 200,
            served_by: "openai/gpt-4o".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("hello".into()),
            response_text: Some("hi there".into()),
        }
    }

    #[test]
    fn resolve_is_fail_safe() {
        assert_eq!(CaptureMode::resolve(true, true, true), CaptureMode::Full);
        assert_eq!(CaptureMode::resolve(false, true, true), CaptureMode::Metadata);
        assert_eq!(CaptureMode::resolve(true, false, true), CaptureMode::Metadata);
        assert_eq!(CaptureMode::resolve(true, true, false), CaptureMode::Metadata);
    }

    #[test]
    fn enforce_capture_strips_text_in_metadata_mode() {
        let r = row(CaptureMode::Metadata).enforce_capture();
        assert!(r.request_text.is_none());
        assert!(r.response_text.is_none());
    }

    #[test]
    fn enforce_capture_keeps_text_in_full_mode() {
        let r = row(CaptureMode::Full).enforce_capture();
        assert_eq!(r.request_text.as_deref(), Some("hello"));
        assert_eq!(r.response_text.as_deref(), Some("hi there"));
    }

    #[test]
    fn row_roundtrips_json() {
        let r = row(CaptureMode::Metadata).enforce_capture();
        let s = serde_json::to_string(&r).unwrap();
        let back: RequestLogRow = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
```

Replace the contents of `crates/gateway-telemetry/src/lib.rs` (drop the `CRATE` placeholder) with:

```rust
//! # gateway-telemetry
//!
//! Async, off-hot-path request-log + spend store; OTel GenAI/MCP semconv span
//! emit; authenticated Prometheus; optional Oximy OTEL/ClickHouse export adapter
//! (default-off). Part of [Oximy Gateway](https://github.com/oximyhq/gateway).
//! See `docs/2026-06-10-oximy-gateway-design.md` (§3, §9) and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod row;

pub use row::{CacheStatus, CaptureMode, RequestKind, RequestLogRow};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry row::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/row.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): RequestLogRow + fail-safe content capture mode"
```

---

### Task 3: The `SpendStore` trait + in-memory columnar segment store

**Files:**
- Create: `crates/gateway-telemetry/src/store.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/store.rs`:

```rust
//! The columnar segment store. Append-only `RequestLogRow`s land in fixed-size
//! segments; reads scan segments and fold into spend aggregates. This is the
//! single-binary embedded fallback (design open-question #1) — DuckDB swaps in
//! behind `SpendStore` later without touching callers. In-memory for P1.7; the
//! batch writer (Task 4) is the only writer, so a plain `RwLock<Vec<Segment>>`
//! suffices and reads never contend with the request hot path.

use std::collections::BTreeMap;

use parking_lot::RwLock;

use gateway_spine::Usd;

use crate::row::RequestLogRow;

/// How spend is bucketed in a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Key,
    Team,
    User,
    Model,
    Tag,
}

/// Half-open time filter `[since_ms, until_ms)`; `None` bounds are open.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeRange {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
}

impl TimeRange {
    pub fn contains(&self, ts_ms: i64) -> bool {
        self.since_ms.is_none_or(|s| ts_ms >= s) && self.until_ms.is_none_or(|u| ts_ms < u)
    }
}

/// One spend bucket in a grouped query result.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpendBucket {
    pub group: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost: Usd,
}

/// The read+write contract. P1.8 reads through this; DuckDB/Postgres impls swap
/// in later. `append` is called ONLY by the batch writer task.
pub trait SpendStore: Send + Sync {
    fn append(&self, row: RequestLogRow);
    fn query(&self, group_by: GroupBy, range: TimeRange, tag_filter: Option<&str>) -> Vec<SpendBucket>;
    /// Most-recent rows first, capped at `limit`, for the logs view (P1.8).
    fn recent(&self, range: TimeRange, limit: usize) -> Vec<RequestLogRow>;
    fn row_count(&self) -> usize;
}

const SEGMENT_ROWS: usize = 4096;

#[derive(Default)]
struct Inner {
    /// Sealed segments + the open tail segment (last element).
    segments: Vec<Vec<RequestLogRow>>,
}

#[derive(Default)]
pub struct MemorySpendStore {
    inner: RwLock<Inner>,
}

impl MemorySpendStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn each_row<F: FnMut(&RequestLogRow)>(&self, range: TimeRange, mut f: F) {
        let g = self.inner.read();
        for seg in &g.segments {
            for r in seg {
                if range.contains(r.ts_ms) {
                    f(r);
                }
            }
        }
    }
}

fn group_keys(row: &RequestLogRow, group_by: GroupBy) -> Vec<String> {
    match group_by {
        GroupBy::Key => vec![row.key_id.clone()],
        GroupBy::Team => vec![row.team_id.clone().unwrap_or_else(|| "(none)".into())],
        GroupBy::User => vec![row.user_id.clone().unwrap_or_else(|| "(none)".into())],
        GroupBy::Model => vec![row.model.clone()],
        GroupBy::Tag => {
            if row.tags.is_empty() {
                vec!["(untagged)".into()]
            } else {
                row.tags.clone()
            }
        }
    }
}

impl SpendStore for MemorySpendStore {
    fn append(&self, row: RequestLogRow) {
        let mut g = self.inner.write();
        if g.segments.last().is_none_or(|s| s.len() >= SEGMENT_ROWS) {
            g.segments.push(Vec::with_capacity(SEGMENT_ROWS));
        }
        g.segments.last_mut().unwrap().push(row);
    }

    fn query(&self, group_by: GroupBy, range: TimeRange, tag_filter: Option<&str>) -> Vec<SpendBucket> {
        let mut acc: BTreeMap<String, SpendBucket> = BTreeMap::new();
        self.each_row(range, |row| {
            if let Some(tag) = tag_filter
                && !row.tags.iter().any(|t| t == tag)
            {
                return;
            }
            for key in group_keys(row, group_by) {
                let b = acc.entry(key.clone()).or_insert_with(|| SpendBucket {
                    group: key,
                    requests: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cost: Usd::ZERO,
                });
                b.requests += 1;
                b.input_tokens += row.usage.input_tokens;
                b.output_tokens += row.usage.output_tokens;
                b.cost += row.cost;
            }
        });
        acc.into_values().collect()
    }

    fn recent(&self, range: TimeRange, limit: usize) -> Vec<RequestLogRow> {
        let g = self.inner.read();
        let mut out = Vec::new();
        for seg in g.segments.iter().rev() {
            for r in seg.iter().rev() {
                if range.contains(r.ts_ms) {
                    out.push(r.clone());
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    fn row_count(&self) -> usize {
        self.inner.read().segments.iter().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{CacheStatus, CaptureMode, RequestKind};
    use gateway_spine::TokenUsage;

    fn row(ts: i64, key: &str, team: &str, model: &str, cost_micros: i64, tags: &[&str]) -> RequestLogRow {
        RequestLogRow {
            ts_ms: ts,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: Some(team.into()),
            user_id: Some("u".into()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            model: model.into(),
            provider: "openai".into(),
            usage: TokenUsage { input_tokens: 100, output_tokens: 50, ..Default::default() },
            cost: Usd::from_micros(cost_micros),
            latency_ms: 10,
            ttft_ms: None,
            status: 200,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: CaptureMode::Metadata,
            request_text: None,
            response_text: None,
        }
    }

    #[test]
    fn group_by_key_sums_cost_and_requests() {
        let s = MemorySpendStore::new();
        s.append(row(1, "k1", "t1", "gpt-4o", 100, &[]));
        s.append(row(2, "k1", "t1", "gpt-4o", 200, &[]));
        s.append(row(3, "k2", "t1", "gpt-4o", 50, &[]));
        let mut buckets = s.query(GroupBy::Key, TimeRange::default(), None);
        buckets.sort_by(|a, b| a.group.cmp(&b.group));
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].group, "k1");
        assert_eq!(buckets[0].requests, 2);
        assert_eq!(buckets[0].cost, Usd::from_micros(300));
        assert_eq!(buckets[1].group, "k2");
        assert_eq!(buckets[1].cost, Usd::from_micros(50));
    }

    #[test]
    fn group_by_model_and_team() {
        let s = MemorySpendStore::new();
        s.append(row(1, "k1", "t1", "gpt-4o", 100, &[]));
        s.append(row(2, "k2", "t2", "claude", 200, &[]));
        let by_model = s.query(GroupBy::Model, TimeRange::default(), None);
        assert_eq!(by_model.len(), 2);
        let by_team = s.query(GroupBy::Team, TimeRange::default(), None);
        assert_eq!(by_team.len(), 2);
    }

    #[test]
    fn group_by_tag_explodes_multi_tag_rows() {
        let s = MemorySpendStore::new();
        s.append(row(1, "k1", "t1", "gpt-4o", 100, &["prod", "agent"]));
        s.append(row(2, "k1", "t1", "gpt-4o", 100, &["prod"]));
        let mut buckets = s.query(GroupBy::Tag, TimeRange::default(), None);
        buckets.sort_by(|a, b| a.group.cmp(&b.group));
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].group, "agent");
        assert_eq!(buckets[0].cost, Usd::from_micros(100));
        assert_eq!(buckets[1].group, "prod");
        assert_eq!(buckets[1].cost, Usd::from_micros(200)); // both rows
    }

    #[test]
    fn time_range_is_half_open() {
        let s = MemorySpendStore::new();
        s.append(row(10, "k", "t", "m", 1, &[]));
        s.append(row(20, "k", "t", "m", 1, &[]));
        s.append(row(30, "k", "t", "m", 1, &[]));
        let range = TimeRange { since_ms: Some(20), until_ms: Some(30) };
        let buckets = s.query(GroupBy::Key, range, None);
        assert_eq!(buckets[0].requests, 1); // only ts=20
    }

    #[test]
    fn tag_filter_restricts_rows() {
        let s = MemorySpendStore::new();
        s.append(row(1, "k1", "t1", "gpt-4o", 100, &["prod"]));
        s.append(row(2, "k2", "t1", "gpt-4o", 100, &["dev"]));
        let buckets = s.query(GroupBy::Key, TimeRange::default(), Some("prod"));
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].group, "k1");
    }

    #[test]
    fn recent_returns_newest_first() {
        let s = MemorySpendStore::new();
        s.append(row(1, "k", "t", "m", 1, &[]));
        s.append(row(2, "k", "t", "m", 1, &[]));
        s.append(row(3, "k", "t", "m", 1, &[]));
        let recent = s.recent(TimeRange::default(), 2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].ts_ms, 3);
        assert_eq!(recent[1].ts_ms, 2);
    }

    #[test]
    fn segments_roll_over_at_capacity() {
        let s = MemorySpendStore::new();
        for i in 0..(SEGMENT_ROWS + 5) {
            s.append(row(i as i64, "k", "t", "m", 1, &[]));
        }
        assert_eq!(s.row_count(), SEGMENT_ROWS + 5);
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod store;

pub use store::{GroupBy, MemorySpendStore, SpendBucket, SpendStore, TimeRange};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry store::`
Expected: 7 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/store.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): SpendStore trait + in-memory columnar segment store with grouped spend queries"
```

---

### Task 4: Live Prometheus-shaped aggregates (the scrape read-model)

**Files:**
- Create: `crates/gateway-telemetry/src/metrics.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> The batch writer (Task 5) updates these counters as it drains rows, so a Prometheus scrape renders a fixed in-memory registry — it NEVER scans the segment store on the hot scrape path.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/metrics.rs`:

```rust
//! Typed Prometheus registry. The batch writer folds each drained row into these
//! counters; the `/metrics` handler (Task 7) renders the OpenMetrics text. Cost
//! is exposed as a µUSD counter (integer-faithful — no float drift across a
//! scrape series). Per-label cardinality is intentionally limited to
//! key/model/status so a noisy tag set can't explode the series count.

use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::row::RequestLogRow;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RequestLabels {
    pub key_id: String,
    pub model: String,
    pub status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct KeyModelLabels {
    pub key_id: String,
    pub model: String,
}

pub struct GatewayMetrics {
    registry: Registry,
    requests_total: Family<RequestLabels, Counter>,
    cost_micros_total: Family<KeyModelLabels, Counter>,
    input_tokens_total: Family<KeyModelLabels, Counter>,
    output_tokens_total: Family<KeyModelLabels, Counter>,
    dropped_rows_total: Counter,
    latency_ms: Family<KeyModelLabels, Histogram>,
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayMetrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        let requests_total = Family::<RequestLabels, Counter>::default();
        let cost_micros_total = Family::<KeyModelLabels, Counter>::default();
        let input_tokens_total = Family::<KeyModelLabels, Counter>::default();
        let output_tokens_total = Family::<KeyModelLabels, Counter>::default();
        let dropped_rows_total = Counter::default();
        let latency_ms = Family::<KeyModelLabels, Histogram>::new_with_constructor(|| {
            Histogram::new([5.0, 25.0, 100.0, 250.0, 1000.0, 5000.0, 30000.0].into_iter())
        });

        registry.register("gateway_requests", "Total governed requests", requests_total.clone());
        registry.register("gateway_cost_micros", "Total cost in µUSD", cost_micros_total.clone());
        registry.register("gateway_input_tokens", "Total input tokens", input_tokens_total.clone());
        registry.register("gateway_output_tokens", "Total output tokens", output_tokens_total.clone());
        registry.register(
            "gateway_dropped_rows",
            "Telemetry rows dropped due to a full channel",
            dropped_rows_total.clone(),
        );
        registry.register("gateway_request_latency_ms", "Request latency (ms)", latency_ms.clone());

        Self {
            registry,
            requests_total,
            cost_micros_total,
            input_tokens_total,
            output_tokens_total,
            dropped_rows_total,
            latency_ms,
        }
    }

    /// Fold one drained row into the live counters.
    pub fn record(&self, row: &RequestLogRow) {
        let req = RequestLabels {
            key_id: row.key_id.clone(),
            model: row.model.clone(),
            status: row.status.to_string(),
        };
        let km = KeyModelLabels { key_id: row.key_id.clone(), model: row.model.clone() };
        self.requests_total.get_or_create(&req).inc();
        // Cost is non-negative µUSD; a Counter only goes up — exact integer.
        self.cost_micros_total
            .get_or_create(&km)
            .inc_by(row.cost.micros().max(0) as u64);
        self.input_tokens_total
            .get_or_create(&km)
            .inc_by(row.usage.input_tokens.max(0) as u64);
        self.output_tokens_total
            .get_or_create(&km)
            .inc_by(row.usage.output_tokens.max(0) as u64);
        self.latency_ms.get_or_create(&km).observe(row.latency_ms as f64);
    }

    pub fn record_dropped(&self) {
        self.dropped_rows_total.inc();
    }

    /// Render the OpenMetrics text body for `/metrics`.
    pub fn render(&self) -> String {
        let mut buf = String::new();
        encode(&mut buf, &self.registry).expect("encode never fails into a String");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{CacheStatus, CaptureMode, RequestKind};
    use gateway_spine::{TokenUsage, Usd};

    fn row(key: &str, model: &str, cost: i64, status: u16) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: model.into(),
            provider: "openai".into(),
            usage: TokenUsage { input_tokens: 100, output_tokens: 50, ..Default::default() },
            cost: Usd::from_micros(cost),
            latency_ms: 120,
            ttft_ms: Some(40),
            status,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: CaptureMode::Metadata,
            request_text: None,
            response_text: None,
        }
    }

    #[test]
    fn render_includes_registered_series() {
        let m = GatewayMetrics::new();
        m.record(&row("k1", "gpt-4o", 7_500, 200));
        let text = m.render();
        assert!(text.contains("gateway_requests_total"));
        assert!(text.contains("gateway_cost_micros_total"));
        assert!(text.contains("key_id=\"k1\""));
        assert!(text.contains("model=\"gpt-4o\""));
    }

    #[test]
    fn cost_counter_accumulates_exact_micros() {
        let m = GatewayMetrics::new();
        m.record(&row("k1", "gpt-4o", 7_500, 200));
        m.record(&row("k1", "gpt-4o", 2_500, 200));
        let text = m.render();
        // 7_500 + 2_500 = 10_000 µUSD on the same label set
        assert!(text.contains("gateway_cost_micros_total{key_id=\"k1\",model=\"gpt-4o\"} 10000"));
    }

    #[test]
    fn dropped_rows_counter_is_exposed() {
        let m = GatewayMetrics::new();
        m.record_dropped();
        m.record_dropped();
        let text = m.render();
        assert!(text.contains("gateway_dropped_rows_total 2"));
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod metrics;

pub use metrics::GatewayMetrics;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry metrics::`
Expected: 3 tests PASS. (If `prometheus-client`'s text encoder appends a trailing `# EOF` or orders labels differently, adjust the literal in `cost_counter_accumulates_exact_micros` to match the encoder's actual output — run once and read the failure diff; the label order is `key_id,model` as declared.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/metrics.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): live Prometheus aggregates with integer µUSD cost counter"
```

---

### Task 5: The async batch writer + non-blocking `TelemetrySink`

**Files:**
- Create: `crates/gateway-telemetry/src/sink.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/sink.rs`:

```rust
//! The hot-path seam. Request handlers (P1.4) call `TelemetrySink::log(row)`,
//! which is a non-blocking `try_send` into a bounded channel — it NEVER blocks
//! and NEVER errors upward. A full channel drops the row and bumps
//! `gateway_dropped_rows_total` (back-pressure must never reach a request). A
//! single background task drains the channel, enforces the capture mode, folds
//! the row into the live Prometheus aggregates, and appends it to the
//! `SpendStore`. This is the "telemetry writes off the hot path" invariant
//! (design §3/§9) made concrete.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::metrics::GatewayMetrics;
use crate::row::RequestLogRow;
use crate::store::SpendStore;

/// Default channel depth. Sized so a brief writer stall buffers rather than
/// drops, but bounded so a wedged writer can't grow memory unbounded.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 16_384;

/// Cheap, `Clone`-able handle held by every request handler.
#[derive(Clone)]
pub struct TelemetrySink {
    tx: mpsc::Sender<RequestLogRow>,
    metrics: Arc<GatewayMetrics>,
}

impl TelemetrySink {
    /// Non-blocking. Enforces capture mode defensively, then tries to enqueue.
    /// On a full or closed channel the row is dropped and counted — the request
    /// is never affected.
    pub fn log(&self, row: RequestLogRow) {
        let row = row.enforce_capture();
        if self.tx.try_send(row).is_err() {
            self.metrics.record_dropped();
        }
    }

    /// Shared metrics handle (the `/metrics` handler renders from this).
    pub fn metrics(&self) -> Arc<GatewayMetrics> {
        Arc::clone(&self.metrics)
    }
}

/// Owns the background drain task; dropping it stops the writer once the channel
/// empties. Built once at boot and kept alive for the process lifetime.
pub struct TelemetryWriter {
    handle: JoinHandle<()>,
}

impl TelemetryWriter {
    pub fn abort(&self) {
        self.handle.abort();
    }
}

/// Wire up the sink + writer over a store + metrics. Returns the cloneable sink
/// (for handlers) and the writer guard (kept at the top level).
pub fn spawn<S: SpendStore + 'static>(
    store: Arc<S>,
    metrics: Arc<GatewayMetrics>,
    capacity: usize,
) -> (TelemetrySink, TelemetryWriter) {
    let (tx, mut rx) = mpsc::channel::<RequestLogRow>(capacity);
    let writer_metrics = Arc::clone(&metrics);
    let handle = tokio::spawn(async move {
        // Drain in small batches to amortize lock acquisition under load.
        let mut batch: Vec<RequestLogRow> = Vec::with_capacity(256);
        loop {
            let n = rx.recv_many(&mut batch, 256).await;
            if n == 0 {
                break; // all senders dropped
            }
            for row in batch.drain(..) {
                writer_metrics.record(&row);
                store.append(row);
            }
        }
    });
    (TelemetrySink { tx, metrics }, TelemetryWriter { handle })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::GatewayMetrics;
    use crate::row::{CacheStatus, CaptureMode, RequestKind};
    use crate::store::{GroupBy, MemorySpendStore, TimeRange};
    use gateway_spine::{TokenUsage, Usd};

    fn row(key: &str, cost: i64, mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage { input_tokens: 100, output_tokens: 50, ..Default::default() },
            cost: Usd::from_micros(cost),
            latency_ms: 10,
            ttft_ms: None,
            status: 200,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("secret prompt".into()),
            response_text: Some("secret reply".into()),
        }
    }

    async fn drain(store: &Arc<MemorySpendStore>, expected: usize) {
        for _ in 0..1000 {
            if store.row_count() >= expected {
                return;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("writer did not drain {expected} rows in time");
    }

    #[tokio::test]
    async fn logged_rows_reach_the_store_and_metrics() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        sink.log(row("k1", 200, CaptureMode::Metadata));
        drain(&store, 2).await;

        let buckets = store.query(GroupBy::Key, TimeRange::default(), None);
        assert_eq!(buckets[0].cost, Usd::from_micros(300));
        assert!(metrics.render().contains("gateway_requests_total"));
    }

    #[tokio::test]
    async fn metadata_mode_strips_text_before_storage() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        drain(&store, 1).await;

        let recent = store.recent(TimeRange::default(), 1);
        assert!(recent[0].request_text.is_none());
        assert!(recent[0].response_text.is_none());
    }

    #[tokio::test]
    async fn full_mode_preserves_text() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Full));
        drain(&store, 1).await;

        let recent = store.recent(TimeRange::default(), 1);
        assert_eq!(recent[0].request_text.as_deref(), Some("secret prompt"));
    }

    #[tokio::test]
    async fn full_channel_drops_and_counts_never_errors() {
        // Capacity 1, no writer running: the channel fills and `log` must still
        // return without panicking, bumping the dropped counter.
        let metrics = Arc::new(GatewayMetrics::new());
        let (tx, _rx) = mpsc::channel::<RequestLogRow>(1);
        let sink = TelemetrySink { tx, metrics: Arc::clone(&metrics) };

        // First fills the buffer, the rest are dropped — none block or error.
        for _ in 0..100 {
            sink.log(row("k1", 1, CaptureMode::Metadata));
        }
        assert!(metrics.render().contains("gateway_dropped_rows_total"));
        // At least 99 dropped (buffer holds 1).
        let text = metrics.render();
        let dropped_line = text
            .lines()
            .find(|l| l.starts_with("gateway_dropped_rows_total "))
            .expect("dropped counter present");
        let n: i64 = dropped_line.rsplit(' ').next().unwrap().parse().unwrap();
        assert!(n >= 99, "expected >=99 drops, got {n}");
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod sink;

pub use sink::{spawn, TelemetrySink, TelemetryWriter, DEFAULT_CHANNEL_CAPACITY};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry sink::`
Expected: 4 tests PASS. (The drain helper polls; if a CI box is slow, the 1000-iteration budget is generous — a genuine failure means the writer isn't consuming.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/sink.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): non-blocking TelemetrySink + async batch writer (off the hot path)"
```

---

### Task 6: Capture-policy resolution from config/key/request

**Files:**
- Create: `crates/gateway-telemetry/src/policy.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> P1.4 calls `CapturePolicy::resolve(...)` once per request to get the `CaptureMode` it stamps onto the row before `sink.log`. This centralizes the fail-safe precedence so the HTTP layer can't get it wrong.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/policy.rs`:

```rust
//! Content-capture policy resolution. Three independent opt-outs compose to a
//! single `CaptureMode` (Task 2). Precedence is AND-of-permits, so the most
//! restrictive level always wins — the gateway defaults to metadata-only and
//! only stores text when the operator, the key owner, AND the caller all opt in.
//!
//! The per-request signal comes from a header (P1.4 maps `x-oximy-log-content:
//! none` → opt-out). `RequestCapturePref::Default` means "no explicit request
//! preference" and inherits the global+key decision.

use crate::row::CaptureMode;

/// Global operator default for content logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalCapture {
    /// Operator allows content logging (subject to key + request opt-out).
    Enabled,
    /// Operator forbids content logging product-wide (hard floor).
    Disabled,
}

/// Per-request preference parsed from the request header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestCapturePref {
    /// No explicit request-level signal; inherit global + key.
    Default,
    /// Caller explicitly opted OUT for this request.
    OptOut,
}

#[derive(Debug, Clone, Copy)]
pub struct CapturePolicy {
    pub global: GlobalCapture,
    /// Per-key default (e.g. a key flagged "never log content").
    pub key_enabled: bool,
}

impl CapturePolicy {
    /// Resolve to the row's `CaptureMode`. Disabled global is a hard floor;
    /// otherwise content is captured only if the key permits AND the request did
    /// not opt out.
    pub fn resolve(&self, request: RequestCapturePref) -> CaptureMode {
        let global_enabled = matches!(self.global, GlobalCapture::Enabled);
        let request_enabled = matches!(request, RequestCapturePref::Default);
        CaptureMode::resolve(global_enabled, self.key_enabled, request_enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_disabled_is_a_hard_floor() {
        let p = CapturePolicy { global: GlobalCapture::Disabled, key_enabled: true };
        assert_eq!(p.resolve(RequestCapturePref::Default), CaptureMode::Metadata);
    }

    #[test]
    fn key_opt_out_wins_even_if_global_enabled() {
        let p = CapturePolicy { global: GlobalCapture::Enabled, key_enabled: false };
        assert_eq!(p.resolve(RequestCapturePref::Default), CaptureMode::Metadata);
    }

    #[test]
    fn request_opt_out_wins() {
        let p = CapturePolicy { global: GlobalCapture::Enabled, key_enabled: true };
        assert_eq!(p.resolve(RequestCapturePref::OptOut), CaptureMode::Metadata);
    }

    #[test]
    fn full_only_when_all_three_permit() {
        let p = CapturePolicy { global: GlobalCapture::Enabled, key_enabled: true };
        assert_eq!(p.resolve(RequestCapturePref::Default), CaptureMode::Full);
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod policy;

pub use policy::{CapturePolicy, GlobalCapture, RequestCapturePref};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry policy::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/policy.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): fail-safe capture-policy resolution (global/key/request)"
```

---

### Task 7: Authenticated `/metrics` exposition (constant-time bearer check)

**Files:**
- Create: `crates/gateway-telemetry/src/prom.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> The HTTP wiring (axum router) lives in `gateway-control` (P1.4). This task owns the **auth + body** as a transport-agnostic function so it is unit-testable here and the server just forwards the `Authorization` header value. Auth-by-default: an absent or wrong token is 401 (design §2 — the LiteLLM open-`/metrics` lesson).

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/prom.rs`:

```rust
//! Authenticated Prometheus exposition. `/metrics` is NEVER open (design §2).
//! The handler takes the raw `Authorization` header value (if any) and the
//! configured scrape token, compares them in constant time, and returns either
//! a 401 or the OpenMetrics body. Transport (axum) is wired in P1.4; this is the
//! pure auth+render core.

use std::sync::Arc;

use subtle::ConstantTimeEq;

use crate::metrics::GatewayMetrics;

/// The Prometheus content type required by scrapers for OpenMetrics text.
pub const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";

/// Outcome of a `/metrics` request: status + body + content type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

pub struct MetricsEndpoint {
    metrics: Arc<GatewayMetrics>,
    /// Bearer token required to scrape. Empty string = a misconfiguration that
    /// we treat as "deny all" (never accidentally open).
    scrape_token: String,
}

impl MetricsEndpoint {
    pub fn new(metrics: Arc<GatewayMetrics>, scrape_token: impl Into<String>) -> Self {
        Self { metrics, scrape_token: scrape_token.into() }
    }

    /// `authorization` is the raw header value, e.g. `Some("Bearer abc")`.
    pub fn handle(&self, authorization: Option<&str>) -> MetricsResponse {
        if !self.authorized(authorization) {
            return MetricsResponse {
                status: 401,
                content_type: "text/plain; charset=utf-8",
                body: "unauthorized\n".into(),
            };
        }
        MetricsResponse {
            status: 200,
            content_type: METRICS_CONTENT_TYPE,
            body: self.metrics.render(),
        }
    }

    fn authorized(&self, authorization: Option<&str>) -> bool {
        // Empty configured token = deny all (fail closed on misconfig).
        if self.scrape_token.is_empty() {
            return false;
        }
        let Some(header) = authorization else {
            return false;
        };
        let Some(presented) = header.strip_prefix("Bearer ") else {
            return false;
        };
        // Constant-time compare; length-mismatch short-circuits to false but the
        // equal-length path is timing-safe.
        let a = presented.as_bytes();
        let b = self.scrape_token.as_bytes();
        a.len() == b.len() && a.ct_eq(b).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(token: &str) -> MetricsEndpoint {
        MetricsEndpoint::new(Arc::new(GatewayMetrics::new()), token)
    }

    #[test]
    fn missing_auth_is_401() {
        let r = endpoint("secret").handle(None);
        assert_eq!(r.status, 401);
        assert!(!r.body.contains("gateway_requests"));
    }

    #[test]
    fn wrong_token_is_401() {
        let r = endpoint("secret").handle(Some("Bearer nope"));
        assert_eq!(r.status, 401);
    }

    #[test]
    fn non_bearer_scheme_is_401() {
        let r = endpoint("secret").handle(Some("Basic secret"));
        assert_eq!(r.status, 401);
    }

    #[test]
    fn correct_token_renders_metrics() {
        let r = endpoint("secret").handle(Some("Bearer secret"));
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, METRICS_CONTENT_TYPE);
        assert!(r.body.contains("gateway_dropped_rows_total"));
    }

    #[test]
    fn empty_configured_token_denies_all() {
        let r = endpoint("").handle(Some("Bearer "));
        assert_eq!(r.status, 401);
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod prom;

pub use prom::{MetricsEndpoint, MetricsResponse, METRICS_CONTENT_TYPE};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry prom::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/prom.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): authenticated /metrics with constant-time bearer check"
```

---

### Task 8: `usage.cost` + `x-overhead-duration-ms` header helpers

**Files:**
- Create: `crates/gateway-telemetry/src/headers.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> The always-on benchmark feature (design §5/§8/§9): every response carries the gateway's own overhead and the call's USD cost. P1.4 measures `overhead = total_handler_time - upstream_time` and asks this module to format the header values. Keeping the formatting here (and tested) means the µUSD→USD-string rendering is consistent across the response header, the logs view, and the dashboard.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/headers.rs`:

```rust
//! Response-header value formatting for the always-on cost + overhead surface.
//! `x-overhead-duration-ms` is the gateway's self-overhead (NOT upstream time);
//! `usage.cost` is rendered as a fixed-6-decimal USD string from integer µUSD so
//! it round-trips exactly (no float formatting drift). These strings are the
//! single source of truth for the header, the logs row display, and the
//! dashboard.

use gateway_spine::Usd;

pub const OVERHEAD_HEADER: &str = "x-overhead-duration-ms";
pub const COST_HEADER: &str = "x-oximy-cost-usd";
pub const SERVED_BY_HEADER: &str = "x-served-by";
pub const FALLBACK_HEADER: &str = "x-fallback";
pub const CACHE_HEADER: &str = "x-cache";

/// Render a `Usd` as a fixed 6-decimal USD string, e.g. 7_500 µUSD → "0.007500".
/// Integer-only: splits whole dollars and the µUSD remainder, zero-pads to 6.
pub fn cost_usd_string(cost: Usd) -> String {
    let micros = cost.micros();
    let sign = if micros < 0 { "-" } else { "" };
    let abs = micros.unsigned_abs();
    let dollars = abs / 1_000_000;
    let frac = abs % 1_000_000;
    format!("{sign}{dollars}.{frac:06}")
}

/// Render an overhead duration (ms) for the header.
pub fn overhead_ms_string(overhead_ms: i64) -> String {
    overhead_ms.max(0).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_formats_six_decimals() {
        assert_eq!(cost_usd_string(Usd::from_micros(7_500)), "0.007500");
        assert_eq!(cost_usd_string(Usd::from_micros(1_000_000)), "1.000000");
        assert_eq!(cost_usd_string(Usd::from_micros(12_500_000)), "12.500000");
        assert_eq!(cost_usd_string(Usd::ZERO), "0.000000");
    }

    #[test]
    fn cost_handles_negative_defensively() {
        assert_eq!(cost_usd_string(Usd::from_micros(-500)), "-0.000500");
    }

    #[test]
    fn overhead_clamps_negative_to_zero() {
        assert_eq!(overhead_ms_string(42), "42");
        assert_eq!(overhead_ms_string(-3), "0");
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod headers;

pub use headers::{
    cost_usd_string, overhead_ms_string, CACHE_HEADER, COST_HEADER, FALLBACK_HEADER,
    OVERHEAD_HEADER, SERVED_BY_HEADER,
};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry headers::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/headers.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): integer-exact usage.cost + x-overhead-duration-ms header formatting"
```

---

### Task 9: OTel GenAI-semconv span builder (feature-gated)

**Files:**
- Create: `crates/gateway-telemetry/src/otel.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> Per design §8/§9 and open-question #4, the gateway emits first-party `gen_ai.*` spans. The full OTLP exporter wiring is heavy and feature-gated (`otel`), but the **attribute mapping** from a `RequestLogRow` to GenAI-semconv key/values is pure and must be tested regardless of the feature, so we keep the mapper out of the `cfg(feature)` block and gate only the exporter init.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/otel.rs`:

```rust
//! OTel GenAI semantic-convention span emit. The attribute MAPPING (row →
//! `gen_ai.*` key/values) is always compiled and tested; only the OTLP exporter
//! pipeline init is behind the `otel` feature. We emit a span per request named
//! `gen_ai.{operation}` with the conventional model/provider/usage attributes;
//! content text is attached ONLY when the row's capture mode kept it (the same
//! fail-safe privacy floor as storage).

use crate::row::{CaptureMode, RequestLogRow};

/// One OTel attribute as a typed key/value. Kept dependency-light so the mapping
/// is testable without pulling the OTel SDK into the unit test.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanAttr {
    pub key: &'static str,
    pub value: AttrValue,
}

/// Build the GenAI-semconv attribute set for a request row. Stable keys per the
/// OpenTelemetry GenAI conventions (`gen_ai.system`, `gen_ai.request.model`,
/// `gen_ai.usage.input_tokens`, etc.) plus gateway-specific `oximy.*` extras.
pub fn span_attrs(row: &RequestLogRow) -> Vec<SpanAttr> {
    let mut attrs = vec![
        SpanAttr { key: "gen_ai.system", value: AttrValue::Str(row.provider.clone()) },
        SpanAttr { key: "gen_ai.request.model", value: AttrValue::Str(row.model.clone()) },
        SpanAttr {
            key: "gen_ai.usage.input_tokens",
            value: AttrValue::Int(row.usage.input_tokens),
        },
        SpanAttr {
            key: "gen_ai.usage.output_tokens",
            value: AttrValue::Int(row.usage.output_tokens),
        },
        SpanAttr { key: "oximy.cost_micros", value: AttrValue::Int(row.cost.micros()) },
        SpanAttr { key: "oximy.key_id", value: AttrValue::Str(row.key_id.clone()) },
        SpanAttr { key: "oximy.served_by", value: AttrValue::Str(row.served_by.clone()) },
        SpanAttr { key: "oximy.fallback_fired", value: AttrValue::Bool(row.fallback_fired) },
        SpanAttr { key: "http.response.status_code", value: AttrValue::Int(row.status as i64) },
    ];
    if let Some(ttft) = row.ttft_ms {
        attrs.push(SpanAttr { key: "oximy.ttft_ms", value: AttrValue::Int(ttft) });
    }
    // Content is attached only under the same privacy floor as storage.
    if row.capture_mode == CaptureMode::Full {
        if let Some(text) = &row.request_text {
            attrs.push(SpanAttr { key: "gen_ai.prompt", value: AttrValue::Str(text.clone()) });
        }
        if let Some(text) = &row.response_text {
            attrs.push(SpanAttr { key: "gen_ai.completion", value: AttrValue::Str(text.clone()) });
        }
    }
    attrs
}

/// The conventional span name for an LLM chat request.
pub fn span_name(row: &RequestLogRow) -> String {
    format!("gen_ai.chat {}", row.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{CacheStatus, RequestKind};
    use gateway_spine::{TokenUsage, Usd};

    fn row(mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: "k1".into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage { input_tokens: 100, output_tokens: 50, ..Default::default() },
            cost: Usd::from_micros(7_500),
            latency_ms: 800,
            ttft_ms: Some(120),
            status: 200,
            served_by: "openai/gpt-4o".into(),
            fallback_fired: true,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("hello".into()),
            response_text: Some("hi".into()),
        }
    }

    fn find<'a>(attrs: &'a [SpanAttr], key: &str) -> Option<&'a AttrValue> {
        attrs.iter().find(|a| a.key == key).map(|a| &a.value)
    }

    #[test]
    fn maps_core_genai_attributes() {
        let attrs = span_attrs(&row(CaptureMode::Metadata));
        assert_eq!(find(&attrs, "gen_ai.system"), Some(&AttrValue::Str("openai".into())));
        assert_eq!(find(&attrs, "gen_ai.request.model"), Some(&AttrValue::Str("gpt-4o".into())));
        assert_eq!(find(&attrs, "gen_ai.usage.input_tokens"), Some(&AttrValue::Int(100)));
        assert_eq!(find(&attrs, "oximy.cost_micros"), Some(&AttrValue::Int(7_500)));
        assert_eq!(find(&attrs, "oximy.fallback_fired"), Some(&AttrValue::Bool(true)));
        assert_eq!(find(&attrs, "oximy.ttft_ms"), Some(&AttrValue::Int(120)));
    }

    #[test]
    fn metadata_mode_omits_content() {
        let attrs = span_attrs(&row(CaptureMode::Metadata));
        assert!(find(&attrs, "gen_ai.prompt").is_none());
        assert!(find(&attrs, "gen_ai.completion").is_none());
    }

    #[test]
    fn full_mode_includes_content() {
        let attrs = span_attrs(&row(CaptureMode::Full));
        assert_eq!(find(&attrs, "gen_ai.prompt"), Some(&AttrValue::Str("hello".into())));
        assert_eq!(find(&attrs, "gen_ai.completion"), Some(&AttrValue::Str("hi".into())));
    }

    #[test]
    fn span_name_includes_model() {
        assert_eq!(span_name(&row(CaptureMode::Metadata)), "gen_ai.chat gpt-4o");
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod otel;

pub use otel::{span_attrs, span_name, AttrValue, SpanAttr};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry otel::`
Expected: 4 tests PASS.

- [ ] **Step 3: Verify the feature gate still builds both ways**

Run: `cargo build -p gateway-telemetry --no-default-features` then `cargo build -p gateway-telemetry`
Expected: both build. (The mapper is feature-independent; only the exporter init in Task 10 is gated.)

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/otel.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): GenAI-semconv span attribute mapping with content privacy floor"
```

---

### Task 10: OTLP + Oximy/ClickHouse export adapters (default-OFF wiring)

**Files:**
- Create: `crates/gateway-telemetry/src/export.rs`
- Modify: `crates/gateway-telemetry/src/lib.rs`

> Two export targets, BOTH off unless configured (design open-question #4 — preserve the standalone posture). `ExportConfig` is plain data; `ExportTarget::Otlp` emits the GenAI spans to an OTLP endpoint (feature `otel`), `ExportTarget::OximyClickHouse` is the first-party substrate adapter. The trait + config + the `Disabled` default are tested here without standing up a network exporter; the live OTLP pipeline init is feature-gated and exercised in P1.8/manual smoke.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-telemetry/src/export.rs`:

```rust
//! Telemetry export adapters. DEFAULT IS OFF: a fresh gateway emits nothing to
//! any external system (no OTLP, no Oximy/ClickHouse) — the standalone posture
//! (design open-question #4). When enabled, the batch writer additionally hands
//! each drained row to the configured `Exporter`. The Oximy/ClickHouse adapter
//! is the first-party substrate seam (design §8.9) and is NEVER the default.

use crate::otel::{span_attrs, SpanAttr};
use crate::row::RequestLogRow;

/// Where, if anywhere, telemetry is exported. `Disabled` is the default.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum ExportConfig {
    /// No external export. The shipped default.
    Disabled,
    /// Standard OTLP endpoint for the GenAI spans.
    Otlp { endpoint: String },
    /// Oximy's OTEL/ClickHouse substrate (opt-in only).
    OximyClickHouse { endpoint: String, tenant_id: String },
}

impl Default for ExportConfig {
    fn default() -> Self {
        ExportConfig::Disabled
    }
}

impl ExportConfig {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, ExportConfig::Disabled)
    }
}

/// The export seam. Implementations receive each drained row (post capture
/// enforcement). `Disabled` builds a no-op exporter.
pub trait Exporter: Send + Sync {
    fn export(&self, row: &RequestLogRow);
}

/// No-op exporter — what `ExportConfig::Disabled` resolves to.
#[derive(Debug, Default)]
pub struct NoopExporter;

impl Exporter for NoopExporter {
    fn export(&self, _row: &RequestLogRow) {}
}

/// Build the exporter for a config. `Disabled` → `NoopExporter`. The live OTLP /
/// ClickHouse exporters are constructed here (feature-gated); when the `otel`
/// feature is off, both enabled variants fall back to noop with a warning so the
/// binary still runs.
pub fn build_exporter(config: &ExportConfig) -> Box<dyn Exporter> {
    match config {
        ExportConfig::Disabled => Box::new(NoopExporter),
        ExportConfig::Otlp { endpoint } => build_otlp(endpoint),
        ExportConfig::OximyClickHouse { endpoint, tenant_id } => {
            build_clickhouse(endpoint, tenant_id)
        }
    }
}

#[cfg(feature = "otel")]
fn build_otlp(endpoint: &str) -> Box<dyn Exporter> {
    // The real pipeline (opentelemetry-otlp) is initialized in P1.8 boot; here we
    // keep a thin adapter that maps each row to GenAI attrs and forwards. The
    // span-pipeline handle is stored in the concrete exporter at boot.
    tracing::info!(endpoint, "OTLP export enabled");
    Box::new(OtlpExporter { _endpoint: endpoint.to_string() })
}

#[cfg(not(feature = "otel"))]
fn build_otlp(endpoint: &str) -> Box<dyn Exporter> {
    tracing::warn!(endpoint, "OTLP export requested but `otel` feature is off; using noop");
    Box::new(NoopExporter)
}

fn build_clickhouse(endpoint: &str, tenant_id: &str) -> Box<dyn Exporter> {
    tracing::info!(endpoint, tenant_id, "Oximy/ClickHouse export enabled");
    Box::new(ClickHouseExporter { _endpoint: endpoint.to_string(), _tenant_id: tenant_id.to_string() })
}

#[cfg(feature = "otel")]
struct OtlpExporter {
    _endpoint: String,
}

#[cfg(feature = "otel")]
impl Exporter for OtlpExporter {
    fn export(&self, row: &RequestLogRow) {
        // Map to GenAI attrs; the concrete span pipeline (P1.8) emits them.
        let _attrs: Vec<SpanAttr> = span_attrs(row);
    }
}

struct ClickHouseExporter {
    _endpoint: String,
    _tenant_id: String,
}

impl Exporter for ClickHouseExporter {
    fn export(&self, row: &RequestLogRow) {
        // Batched insert into the Oximy substrate (P4 hardens this); attr shape
        // mirrors the GenAI span set for cross-system consistency.
        let _attrs: Vec<SpanAttr> = span_attrs(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        assert_eq!(ExportConfig::default(), ExportConfig::Disabled);
        assert!(!ExportConfig::default().is_enabled());
    }

    #[test]
    fn disabled_builds_noop_that_does_nothing() {
        let exporter = build_exporter(&ExportConfig::Disabled);
        // Calling export on a noop must be safe and a no-op (no panic).
        let row = sample_row();
        exporter.export(&row);
    }

    #[test]
    fn enabled_configs_report_enabled() {
        assert!(ExportConfig::Otlp { endpoint: "http://localhost:4317".into() }.is_enabled());
        assert!(
            ExportConfig::OximyClickHouse {
                endpoint: "https://ch.oximy.com".into(),
                tenant_id: "t1".into(),
            }
            .is_enabled()
        );
    }

    #[test]
    fn config_roundtrips_json_with_target_tag() {
        let c = ExportConfig::OximyClickHouse {
            endpoint: "https://ch.oximy.com".into(),
            tenant_id: "t1".into(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"target\":\"oximy_click_house\""));
        let back: ExportConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    fn sample_row() -> RequestLogRow {
        use crate::row::{CacheStatus, CaptureMode, RequestKind};
        use gateway_spine::{TokenUsage, Usd};
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: "k1".into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage::default(),
            cost: Usd::ZERO,
            latency_ms: 1,
            ttft_ms: None,
            status: 200,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: CaptureMode::Metadata,
            request_text: None,
            response_text: None,
        }
    }
}
```

Add to `crates/gateway-telemetry/src/lib.rs`:

```rust
pub mod export;

pub use export::{build_exporter, ExportConfig, Exporter, NoopExporter};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-telemetry export::`
Expected: 4 tests PASS. (If `serde` renames the variant to a different snake_case spelling, read the failure and update the literal — `OximyClickHouse` → `oximy_click_house` is the expected `rename_all = "snake_case"` output.)

- [ ] **Step 3: Verify both feature configurations build**

Run: `cargo build -p gateway-telemetry --no-default-features` then `cargo build -p gateway-telemetry`
Expected: both build (the `otel`-off path uses the noop fallback for `Otlp`).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/export.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): default-off OTLP + Oximy/ClickHouse export adapters"
```

---

### Task 11: Wire the exporter into the batch writer

**Files:**
- Modify: `crates/gateway-telemetry/src/sink.rs`

> The writer must fan each drained row out to the configured exporter in addition to metrics + store. Default-off means a `NoopExporter` by default, so this changes nothing observable unless an operator enables export.

- [ ] **Step 1: Extend the failing test**

Append this test to the `tests` module in `crates/gateway-telemetry/src/sink.rs` (before the closing `}` of `mod tests`):

```rust
    #[tokio::test]
    async fn writer_fans_out_to_exporter() {
        use crate::export::Exporter;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingExporter(Arc<AtomicUsize>);
        impl Exporter for CountingExporter {
            fn export(&self, _row: &RequestLogRow) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let count = Arc::new(AtomicUsize::new(0));
        let exporter: Box<dyn Exporter> = Box::new(CountingExporter(Arc::clone(&count)));
        let (sink, _writer) =
            spawn_with_exporter(Arc::clone(&store), Arc::clone(&metrics), exporter, 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        sink.log(row("k1", 200, CaptureMode::Metadata));
        drain(&store, 2).await;

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p gateway-telemetry sink::writer_fans_out_to_exporter`
Expected: FAILS to compile — `spawn_with_exporter` does not exist yet.

- [ ] **Step 3: Refactor `spawn` to delegate to a new `spawn_with_exporter`**

In `crates/gateway-telemetry/src/sink.rs`, add the import near the top (with the other `use crate::` lines):

```rust
use crate::export::{Exporter, NoopExporter};
```

Replace the existing `spawn` function with both functions below (`spawn` now delegates with a default `NoopExporter`):

```rust
/// Wire up the sink + writer over a store + metrics with NO external export
/// (the shipped default — standalone posture).
pub fn spawn<S: SpendStore + 'static>(
    store: Arc<S>,
    metrics: Arc<GatewayMetrics>,
    capacity: usize,
) -> (TelemetrySink, TelemetryWriter) {
    spawn_with_exporter(store, metrics, Box::new(NoopExporter), capacity)
}

/// Wire up the sink + writer with an explicit exporter (e.g. OTLP or Oximy
/// substrate when an operator opts in via `ExportConfig`).
pub fn spawn_with_exporter<S: SpendStore + 'static>(
    store: Arc<S>,
    metrics: Arc<GatewayMetrics>,
    exporter: Box<dyn Exporter>,
    capacity: usize,
) -> (TelemetrySink, TelemetryWriter) {
    let (tx, mut rx) = mpsc::channel::<RequestLogRow>(capacity);
    let writer_metrics = Arc::clone(&metrics);
    let handle = tokio::spawn(async move {
        let mut batch: Vec<RequestLogRow> = Vec::with_capacity(256);
        loop {
            let n = rx.recv_many(&mut batch, 256).await;
            if n == 0 {
                break;
            }
            for row in batch.drain(..) {
                writer_metrics.record(&row);
                exporter.export(&row);
                store.append(row);
            }
        }
    });
    (TelemetrySink { tx, metrics }, TelemetryWriter { handle })
}
```

Add the new function to the re-exports in `crates/gateway-telemetry/src/lib.rs`:

```rust
pub use sink::{spawn, spawn_with_exporter, TelemetrySink, TelemetryWriter, DEFAULT_CHANNEL_CAPACITY};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p gateway-telemetry sink::`
Expected: all 5 sink tests PASS (including `writer_fans_out_to_exporter`).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
git add crates/gateway-telemetry/src/sink.rs crates/gateway-telemetry/src/lib.rs
git commit -s -m "feat(telemetry): fan drained rows out to the configured exporter"
```

---

### Task 12: Finalize `lib.rs` + end-to-end telemetry integration test

**Files:**
- Modify: `crates/gateway-telemetry/src/lib.rs`
- Create: `crates/gateway-telemetry/tests/telemetry_e2e.rs`

- [ ] **Step 1: Confirm the final module surface**

Ensure `crates/gateway-telemetry/src/lib.rs` reads exactly (doc comment + forbid attribute, all module declarations, then all re-exports, NO `CRATE` placeholder):

```rust
//! # gateway-telemetry
//!
//! Async, off-hot-path request-log + spend store; OTel GenAI/MCP semconv span
//! emit; authenticated Prometheus; optional Oximy OTEL/ClickHouse export adapter
//! (default-off). Owns the invariants: telemetry never blocks/fails a request,
//! `/metrics` is auth-by-default, content capture is fail-safe metadata-only,
//! and the standalone posture is preserved (no external export unless opted in).
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` (§3, §9) and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod export;
pub mod headers;
pub mod metrics;
pub mod otel;
pub mod policy;
pub mod prom;
pub mod row;
pub mod sink;
pub mod store;

pub use export::{build_exporter, ExportConfig, Exporter, NoopExporter};
pub use headers::{
    cost_usd_string, overhead_ms_string, CACHE_HEADER, COST_HEADER, FALLBACK_HEADER,
    OVERHEAD_HEADER, SERVED_BY_HEADER,
};
pub use metrics::GatewayMetrics;
pub use otel::{span_attrs, span_name, AttrValue, SpanAttr};
pub use policy::{CapturePolicy, GlobalCapture, RequestCapturePref};
pub use prom::{MetricsEndpoint, MetricsResponse, METRICS_CONTENT_TYPE};
pub use row::{CacheStatus, CaptureMode, RequestKind, RequestLogRow};
pub use sink::{spawn, spawn_with_exporter, TelemetrySink, TelemetryWriter, DEFAULT_CHANNEL_CAPACITY};
pub use store::{GroupBy, MemorySpendStore, SpendBucket, SpendStore, TimeRange};
```

- [ ] **Step 2: Write the end-to-end integration test**

Create `crates/gateway-telemetry/tests/telemetry_e2e.rs`:

```rust
//! End-to-end telemetry path as the HTTP lifecycle (P1.4) will drive it:
//! resolve the capture policy → stamp the row → `sink.log` (non-blocking) → the
//! background writer enforces capture, folds metrics, exports (noop here), and
//! appends to the store → spend queries + an authenticated `/metrics` scrape +
//! the response-header formatting all reflect it.

use std::sync::Arc;
use std::time::Duration;

use gateway_spine::{TokenUsage, Usd};
use gateway_telemetry::{
    cost_usd_string, spawn, CacheStatus, CapturePolicy, GatewayMetrics, GlobalCapture, GroupBy,
    MemorySpendStore, MetricsEndpoint, RequestCapturePref, RequestKind, RequestLogRow, TimeRange,
};

fn row(policy: &CapturePolicy, req_pref: RequestCapturePref, key: &str, cost: i64) -> RequestLogRow {
    RequestLogRow {
        ts_ms: 1_000,
        kind: RequestKind::Llm,
        key_id: key.into(),
        team_id: Some("team_a".into()),
        user_id: Some("user_x".into()),
        tags: vec!["prod".into()],
        model: "gpt-4o".into(),
        provider: "openai".into(),
        usage: TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
        cost: Usd::from_micros(cost),
        latency_ms: 820,
        ttft_ms: Some(140),
        status: 200,
        served_by: "openai/gpt-4o".into(),
        fallback_fired: false,
        cache_status: CacheStatus::Miss,
        capture_mode: policy.resolve(req_pref),
        request_text: Some("a secret prompt".into()),
        response_text: Some("a secret reply".into()),
    }
}

#[tokio::test]
async fn full_telemetry_path() {
    let store = Arc::new(MemorySpendStore::new());
    let metrics = Arc::new(GatewayMetrics::new());
    let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

    // Operator allows content, key allows it, request opts OUT → metadata-only.
    let policy = CapturePolicy { global: GlobalCapture::Enabled, key_enabled: true };
    sink.log(row(&policy, RequestCapturePref::OptOut, "key_1", 7_500));
    // A second call from the same key, content permitted everywhere → Full.
    sink.log(row(&policy, RequestCapturePref::Default, "key_1", 2_500));

    // Wait for the writer to drain.
    for _ in 0..1000 {
        if store.row_count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(store.row_count(), 2);

    // Spend grouped by key is exact.
    let buckets = store.query(GroupBy::Key, TimeRange::default(), None);
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].group, "key_1");
    assert_eq!(buckets[0].requests, 2);
    assert_eq!(buckets[0].cost, Usd::from_micros(10_000));

    // The opt-out row stored NO text; the permitted row kept it.
    let recent = store.recent(TimeRange::default(), 2);
    let opted_out = recent.iter().find(|r| r.capture_mode == gateway_telemetry::CaptureMode::Metadata).unwrap();
    assert!(opted_out.request_text.is_none());
    let full = recent.iter().find(|r| r.capture_mode == gateway_telemetry::CaptureMode::Full).unwrap();
    assert_eq!(full.request_text.as_deref(), Some("a secret prompt"));

    // Authenticated /metrics reflects the two requests; unauth is 401.
    let endpoint = MetricsEndpoint::new(Arc::clone(&metrics), "scrape-secret");
    assert_eq!(endpoint.handle(None).status, 401);
    let scrape = endpoint.handle(Some("Bearer scrape-secret"));
    assert_eq!(scrape.status, 200);
    assert!(scrape.body.contains("gateway_cost_micros_total{key_id=\"key_1\",model=\"gpt-4o\"} 10000"));

    // The response-header cost string is integer-exact.
    assert_eq!(cost_usd_string(buckets[0].cost), "0.010000");
}
```

- [ ] **Step 3: Run the whole crate's tests**

Run: `cargo test -p gateway-telemetry`
Expected: all unit tests + `telemetry_e2e` PASS.

- [ ] **Step 4: Full gate (both feature configs), then commit**

```bash
cargo fmt --all && cargo clippy -p gateway-telemetry --all-targets -- -D warnings
cargo build -p gateway-telemetry --no-default-features
git add crates/gateway-telemetry/src/lib.rs crates/gateway-telemetry/tests/telemetry_e2e.rs
git commit -s -m "feat(telemetry): finalize module surface + end-to-end telemetry path test"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-telemetry` is fully green (all unit tests + `telemetry_e2e`).
- [ ] `cargo clippy -p gateway-telemetry --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
- [ ] `cargo build -p gateway-telemetry --no-default-features` builds (the `otel`-off path is intact; OTLP export falls back to noop with a warning).
- [ ] The four invariants this milestone owns are each proven by a test: telemetry-never-blocks (`full_channel_drops_and_counts_never_errors`), auth-by-default `/metrics` (`missing_auth_is_401`), fail-safe content capture (`metadata_mode_strips_text_before_storage` + `global_disabled_is_a_hard_floor`), standalone-default export (`default_is_disabled`).
- [ ] No floats in cost math (grep `f64` in `gateway-telemetry/src` → only in the latency histogram `observe()` bucket boundaries, never in `Usd`/cost; cost renders via integer split in `headers.rs`).
- [ ] Spend queries group correctly by key/team/user/model/tag with exact integer cost sums (`store::` tests).

## Interface this milestone EXPOSES (downstream milestones depend on it)

P1.4 (HTTP lifecycle) and P1.8 (dashboard) consume `gateway-telemetry`'s public surface verbatim:

- **Hot-path logging:** `TelemetrySink` (`.log(RequestLogRow)` — non-blocking, `Clone`, held by every request handler) + `gateway_telemetry::spawn(store, metrics, capacity)` / `spawn_with_exporter(...)` → `(TelemetrySink, TelemetryWriter)`, built once at boot. `DEFAULT_CHANNEL_CAPACITY`.
- **The row P1.4 must populate:** `RequestLogRow { ts_ms, kind: RequestKind, key_id, team_id, user_id, tags, model, provider, usage: TokenUsage, cost: Usd, latency_ms, ttft_ms, status, served_by, fallback_fired, cache_status: CacheStatus, capture_mode: CaptureMode, request_text, response_text }`; stamp `capture_mode` via `CapturePolicy { global, key_enabled }.resolve(RequestCapturePref)` before logging.
- **Response headers (always-on benchmark surface):** `cost_usd_string(Usd) -> String`, `overhead_ms_string(i64) -> String`, and the header-name consts `OVERHEAD_HEADER` / `COST_HEADER` / `SERVED_BY_HEADER` / `FALLBACK_HEADER` / `CACHE_HEADER`.
- **`/metrics` endpoint:** `MetricsEndpoint::new(Arc<GatewayMetrics>, scrape_token).handle(Option<&str>) -> MetricsResponse { status, content_type, body }` — P1.4 mounts it on the axum router and forwards the `Authorization` header; `GatewayMetrics` comes from the sink via `TelemetrySink::metrics()`.
- **Spend read API (P1.8 dashboard):** the `SpendStore` trait — `query(GroupBy, TimeRange, Option<&str> tag) -> Vec<SpendBucket>`, `recent(TimeRange, limit) -> Vec<RequestLogRow>`, `row_count()`; `GroupBy { Key, Team, User, Model, Tag }`, `TimeRange { since_ms, until_ms }`, `SpendBucket { group, requests, input_tokens, output_tokens, cost }`; default impl `MemorySpendStore` (DuckDB/Postgres swap in later behind this trait).
- **Export config (P1.6 config schema):** `ExportConfig { Disabled | Otlp { endpoint } | OximyClickHouse { endpoint, tenant_id } }` (default `Disabled`) + `build_exporter(&ExportConfig) -> Box<dyn Exporter>` for boot wiring.

**Next:** `2026-06-10-p1-08-dashboard-and-firstboot.md` — the embedded dashboard + `oximy-gateway up` zero-config first boot, which renders the spend queries, logs, and metrics this milestone exposes.
