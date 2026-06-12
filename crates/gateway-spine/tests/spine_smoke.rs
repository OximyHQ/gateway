//! End-to-end (in-memory) admission path: a key is created, a request is
//! admitted (usable → allowlist → rate limit → budget reserve), the call's
//! actual cost is priced from the registry and committed, and an audit event is
//! recorded. This is the shape the HTTP lifecycle (P1.4) will wire to real I/O.

use gateway_spine::{
    AuditEvent, AuditSink, BudgetLedger, MemoryAudit, MockClock, ModelEntry, ModelPrice,
    ModelRegistry, RateLimiter, RateLimits, TokenUsage, Usd, VirtualKey,
};

const NOW_MS: i64 = 1_000_000;

fn gpt4o_entry() -> ModelEntry {
    ModelEntry {
        id: "gpt-4o".into(),
        provider: "openai".into(),
        price: ModelPrice {
            input_per_mtok: 2_500_000,
            output_per_mtok: 10_000_000,
            cache_read_per_mtok: 1_250_000,
            cache_write_per_mtok: 0,
        },
        context_window: Some(128_000),
        max_output_tokens: Some(16_384),
        supports_tools: true,
        supports_vision: true,
        supports_streaming: true,
    }
}

#[test]
fn full_admission_and_commit_path() {
    // Setup
    let mut registry = ModelRegistry::new();
    registry.insert(gpt4o_entry());
    let ledger = BudgetLedger::new();
    let limiter = RateLimiter::new(MockClock::new(NOW_MS));
    let audit = MemoryAudit::new();

    let key = VirtualKey {
        id: "key_1".into(),
        token_hash: VirtualKey::hash_secret("sk-test"),
        token_prefix: "sk-test".into(),
        max_budget: Some(Usd::from_dollars_f64(1.0)),
        limits: RateLimits {
            rpm: Some(60),
            tpm: Some(100_000),
            max_parallel: Some(4),
        },
        model_allowlist: Some(vec!["gpt-4o".into()]),
        tool_allowlist: None,
        expires_at: None,
        revoked: false,
        parent_id: None,
    };
    ledger.set_budget(&key.id, key.max_budget, Usd::ZERO);

    let model = "gpt-4o";
    let est_tokens = 1500;

    // 1. key usable (no expiry, so NOW_MS is fine)
    key.ensure_usable(NOW_MS).unwrap();
    // 2. model allowed
    assert!(key.allows_model(model));
    // 3. rate limit
    limiter.acquire(&key.id, &key.limits, est_tokens).unwrap();
    // 4. budget reserve (estimate $0.05)
    let res = ledger
        .reserve(&key.id, Usd::from_dollars_f64(0.05))
        .unwrap();

    // ... upstream call happens here in P1.4; we simulate the returned usage ...
    let usage = TokenUsage {
        input_tokens: 1000,
        output_tokens: 500,
        ..Default::default()
    };
    let actual = registry
        .cost(model, &usage)
        .expect("known model has a price");
    assert_eq!(actual, Usd::from_micros(7_500)); // $0.0075

    // 5. commit actual, release parallel slot
    ledger.commit(res, actual).unwrap();
    limiter.release_parallel(&key.id);

    // 6. audit
    audit.record(AuditEvent {
        ts_ms: NOW_MS,
        actor: key.id.clone(),
        action: "request.complete".into(),
        target: model.into(),
        outcome: "ok".into(),
        detail: Some(format!("{} µUSD", actual.micros())),
    });

    // Final state
    assert_eq!(ledger.spent(&key.id), Usd::from_micros(7_500));
    assert_eq!(ledger.reserved(&key.id), Usd::ZERO);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit.events()[0].outcome, "ok");
}
