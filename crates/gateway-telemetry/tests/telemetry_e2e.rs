//! End-to-end telemetry path as the HTTP lifecycle (P1.4) will drive it:
//! resolve the capture policy → stamp the row → `sink.log` (non-blocking) → the
//! background writer enforces capture, folds metrics, exports (noop here), and
//! appends to the store → spend queries + an authenticated `/metrics` scrape +
//! the response-header formatting all reflect it.

use std::sync::Arc;
use std::time::Duration;

use gateway_spine::{TokenUsage, Usd};
use gateway_telemetry::{
    CacheStatus, CapturePolicy, GatewayMetrics, GlobalCapture, GroupBy, MemorySpendStore,
    MetricsEndpoint, RequestCapturePref, RequestKind, RequestLogRow, SpendStore, TimeRange,
    cost_usd_string, spawn,
};

fn row(
    policy: &CapturePolicy,
    req_pref: RequestCapturePref,
    key: &str,
    cost: i64,
) -> RequestLogRow {
    RequestLogRow {
        ts_ms: 1_000,
        kind: RequestKind::Llm,
        key_id: key.into(),
        team_id: Some("team_a".into()),
        user_id: Some("user_x".into()),
        tags: vec!["prod".into()],
        model: "gpt-4o".into(),
        provider: "openai".into(),
        usage: TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        },
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
    let policy = CapturePolicy {
        global: GlobalCapture::Enabled,
        key_enabled: true,
    };
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
    let opted_out = recent
        .iter()
        .find(|r| r.capture_mode == gateway_telemetry::CaptureMode::Metadata)
        .unwrap();
    assert!(opted_out.request_text.is_none());
    let full = recent
        .iter()
        .find(|r| r.capture_mode == gateway_telemetry::CaptureMode::Full)
        .unwrap();
    assert_eq!(full.request_text.as_deref(), Some("a secret prompt"));

    // Authenticated /metrics reflects the two requests; unauth is 401.
    let endpoint = MetricsEndpoint::new(Arc::clone(&metrics), "scrape-secret");
    assert_eq!(endpoint.handle(None).status, 401);
    let scrape = endpoint.handle(Some("Bearer scrape-secret"));
    assert_eq!(scrape.status, 200);
    assert!(
        scrape
            .body
            .contains("gateway_cost_micros_total{key_id=\"key_1\",model=\"gpt-4o\"} 10000")
    );

    // The response-header cost string is integer-exact.
    assert_eq!(cost_usd_string(buckets[0].cost), "0.010000");
}
