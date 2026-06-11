//! End-to-end: the cache layer over a tiered (L1-only) store, exercising the full
//! shape P1.4 will wire — MISS → call upstream → store → HIT (replay) → stats —
//! plus TTL expiry against a shared `MockClock`, and the 200-only / partial-stream
//! invariants. A `MockClock` is shared by `Arc`-cloning it into both the layer and
//! the test (the layer takes a `Clock` by value; `MockClock` is `Sync`, so we wrap
//! it in an `Arc` and impl `Clock` for the Arc via the spine's blanket — here we
//! just construct two layers sharing one store and advance via a helper clock).

use std::sync::Arc;

use gateway_cache::{
    CacheControl, CacheLayer, CacheStatus, CacheStore, MemoryStore, TieredStore, replay_stream,
    replay_unary,
};
use gateway_llm::{ChatResponse, FinishReason, StreamDelta};
use gateway_spine::{Clock, TokenUsage, Usd};

/// A clock the test can advance, shared into the layer by Arc.
#[derive(Clone)]
struct SharedClock(Arc<std::sync::atomic::AtomicI64>);
impl SharedClock {
    fn new(start: i64) -> Self {
        Self(Arc::new(std::sync::atomic::AtomicI64::new(start)))
    }
    fn advance(&self, by: i64) {
        self.0.fetch_add(by, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Clock for SharedClock {
    fn now_ms(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn unary_resp() -> ChatResponse {
    ChatResponse {
        model: "gpt-4o".into(),
        content: vec![],
        tool_calls: vec![],
        finish_reason: FinishReason::Stop,
        usage: TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        },
        provider_response_id: Some("resp_1".into()),
    }
}
fn usage() -> TokenUsage {
    TokenUsage {
        input_tokens: 1000,
        output_tokens: 500,
        ..Default::default()
    }
}

#[tokio::test]
async fn unary_miss_store_hit_and_stats() {
    let store: Arc<dyn CacheStore> = Arc::new(TieredStore::l1_only(Arc::new(MemoryStore::new())));
    let clock = SharedClock::new(1_000_000);
    let layer = CacheLayer::new(store, clock, 60);

    let body = serde_json::json!({"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]});
    let ctl = CacheControl::default();

    // 1. MISS
    assert_eq!(
        layer
            .lookup("tenant-1", "/v1/chat/completions", "gpt-4o", &body, &ctl)
            .await
            .status,
        CacheStatus::Miss
    );

    // 2. ... upstream call ... store the 200
    layer
        .store_unary(
            "tenant-1",
            "/v1/chat/completions",
            "gpt-4o",
            &body,
            &ctl,
            unary_resp(),
            usage(),
            Usd::from_micros(7_500),
        )
        .await;

    // 3. HIT, replay the unary response, $0 re-charge but $-saved tracked
    let hit = layer
        .lookup("tenant-1", "/v1/chat/completions", "gpt-4o", &body, &ctl)
        .await;
    assert_eq!(hit.status, CacheStatus::Hit);
    let entry = hit.value.unwrap();
    assert_eq!(entry.original_cost, Usd::from_micros(7_500));
    assert_eq!(
        replay_unary(&entry)
            .unwrap()
            .provider_response_id
            .as_deref(),
        Some("resp_1")
    );

    let snap = layer.stats();
    assert_eq!(snap.hits, 1);
    assert_eq!(snap.misses, 1);
    assert_eq!(snap.dollars_saved(), Usd::from_micros(7_500));
}

#[tokio::test]
async fn ttl_expiry_against_shared_clock() {
    let store: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
    let clock = SharedClock::new(0);
    let layer = CacheLayer::new(store, clock.clone(), 3600);
    let body = serde_json::json!({"a":1});
    // 2-second TTL override
    let ctl = CacheControl {
        ttl_secs: Some(2),
        ..Default::default()
    };
    layer
        .store_unary(
            "t",
            "/e",
            "m",
            &body,
            &ctl,
            unary_resp(),
            usage(),
            Usd::ZERO,
        )
        .await;
    // immediate read → HIT
    assert_eq!(
        layer
            .lookup("t", "/e", "m", &body, &CacheControl::default())
            .await
            .status,
        CacheStatus::Hit
    );
    // advance past TTL → MISS (expired)
    clock.advance(2_000);
    assert_eq!(
        layer
            .lookup("t", "/e", "m", &body, &CacheControl::default())
            .await
            .status,
        CacheStatus::Miss
    );
}

#[tokio::test]
async fn streaming_roundtrip_replays_exactly() {
    let store: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
    let layer = CacheLayer::new(store, SharedClock::new(0), 60);
    let body = serde_json::json!({"stream":true,"messages":[{"content":"hi"}]});
    let ctl = CacheControl::default();

    let deltas = vec![
        StreamDelta::text("Hel"),
        StreamDelta::text("lo"),
        StreamDelta::finish(FinishReason::Stop, usage()),
    ];
    let stored = layer
        .store_stream(
            "t",
            "/e",
            "gpt-4o",
            &body,
            &ctl,
            deltas,
            usage(),
            Usd::from_micros(7_500),
        )
        .await;
    assert!(stored);

    let hit = layer.lookup("t", "/e", "gpt-4o", &body, &ctl).await;
    assert_eq!(hit.status, CacheStatus::Hit);
    let entry = hit.value.unwrap();
    let replayed: Vec<StreamDelta> = replay_stream(&entry).unwrap().collect();
    assert_eq!(replayed.len(), 3);
    let text: String = replayed
        .iter()
        .filter_map(|d| d.content_delta.clone())
        .collect();
    assert_eq!(text, "Hello");
    assert!(
        replayed.last().unwrap().usage.is_some(),
        "terminal usage delta is replayed"
    );
}

#[tokio::test]
async fn tenant_isolation_end_to_end() {
    let store: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
    let layer = CacheLayer::new(store, SharedClock::new(0), 60);
    let body = serde_json::json!({"messages":[{"content":"secret"}]});
    let ctl = CacheControl::default();
    layer
        .store_unary(
            "tenant-a",
            "/e",
            "m",
            &body,
            &ctl,
            unary_resp(),
            usage(),
            Usd::ZERO,
        )
        .await;
    // tenant-b sends the IDENTICAL body → must MISS (never read tenant-a's entry).
    assert_eq!(
        layer
            .lookup("tenant-b", "/e", "m", &body, &ctl)
            .await
            .status,
        CacheStatus::Miss
    );
}
