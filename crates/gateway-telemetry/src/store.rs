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
    fn query(
        &self,
        group_by: GroupBy,
        range: TimeRange,
        tag_filter: Option<&str>,
    ) -> Vec<SpendBucket>;
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

    fn query(
        &self,
        group_by: GroupBy,
        range: TimeRange,
        tag_filter: Option<&str>,
    ) -> Vec<SpendBucket> {
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

    fn row(
        ts: i64,
        key: &str,
        team: &str,
        model: &str,
        cost_micros: i64,
        tags: &[&str],
    ) -> RequestLogRow {
        RequestLogRow {
            ts_ms: ts,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: Some(team.into()),
            user_id: Some("u".into()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            model: model.into(),
            provider: "openai".into(),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
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
        let range = TimeRange {
            since_ms: Some(20),
            until_ms: Some(30),
        };
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
