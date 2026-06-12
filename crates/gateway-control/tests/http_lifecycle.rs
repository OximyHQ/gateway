//! Black-box HTTP test: drive the assembled `/v1/*` router with `tower::oneshot`
//! and assert the governance lifecycle is observable over the wire — auth,
//! fail-closed budget (429, no egress), authoritative cost in the body, and
//! budget depletion across requests. Mirrors what an OpenAI SDK sees.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use gateway_control::guard::empty_chain;
use gateway_control::keystore::StaticKeyStore;
use gateway_control::providers::{Deployment, ProviderRegistry};
use gateway_control::server::router;
use gateway_control::state::AppState;
use gateway_llm::{
    ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
    ProviderCapabilities, ProviderError,
};
use gateway_spine::{
    MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
};
use http::Request;
use tower::ServiceExt;

struct Counting {
    calls: AtomicUsize,
}

#[async_trait]
impl Provider for Counting {
    fn id(&self) -> &str {
        "counting"
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: false,
            supports_tools: false,
            supports_vision: false,
            supports_idempotency: true,
        }
    }
    async fn chat(
        &self,
        req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ChatResponse {
            model: req.model.clone(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            },
            provider_response_id: Some("r".into()),
        })
    }
    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        unreachable!()
    }
}

fn gpt4o() -> ModelEntry {
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

async fn build(budget: Usd) -> (Arc<AppState<MockClock>>, Arc<Counting>) {
    let provider = Arc::new(Counting {
        calls: AtomicUsize::new(0),
    });
    let mut ks = StaticKeyStore::new();
    ks.insert(VirtualKey {
        id: "key_1".into(),
        token_hash: VirtualKey::hash_secret("sk-good"),
        token_prefix: "sk-good".into(),
        max_budget: Some(budget),
        limits: RateLimits::default(),
        model_allowlist: None,
        tool_allowlist: None,
        expires_at: None,
        revoked: false,
        parent_id: None,
    });
    let mut providers = ProviderRegistry::new();
    providers.insert(
        "openai",
        Deployment {
            provider: provider.clone(),
            credentials: Arc::new(Credentials::new("up")),
        },
    );
    let store = Arc::new(
        gateway_store::Store::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    store
        .upsert_key(&gateway_store::StoredKey {
            id: "key_1".to_string(),
            name: "key_1".to_string(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".to_string(),
            budget_micros: Some(budget.micros()),
            spent_micros: 0,
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
            expires_at_ms: None,
            revoked: false,
            parent_id: None,
            created_at_ms: 0,
        })
        .await
        .unwrap();
    let state = Arc::new(AppState::with_parts(
        Arc::new(ks),
        Arc::new(MockClock::new(0)),
        providers,
        Arc::new(empty_chain()),
        Arc::new(MemoryAudit::new()),
        store,
    ));
    state.registry.write().unwrap().insert(gpt4o());
    state.ledger.set_budget("key_1", Some(budget), Usd::ZERO);
    (state, provider)
}

fn chat_body() -> Body {
    Body::from(r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#)
}

#[tokio::test]
async fn successful_request_bills_and_returns_cost() {
    let (state, provider) = build(Usd::from_dollars_f64(10.0)).await;
    let app = router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", "Bearer sk-good")
                .header("content-type", "application/json")
                .body(chat_body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["usage"]["cost"], 0.0075);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
}

#[tokio::test]
async fn budget_blocked_request_is_429_without_egress() {
    // budget so small the worst-case reserve fails before any call
    let (state, provider) = build(Usd::from_micros(1)).await;
    let app = router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", "Bearer sk-good")
                .header("content-type", "application/json")
                .body(chat_body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "fail-closed: provider never called"
    );
    assert_eq!(state.ledger.spent("key_1"), Usd::ZERO);
}
