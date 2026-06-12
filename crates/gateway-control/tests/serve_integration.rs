//! Integration test: binds `serve` to an ephemeral port, makes REAL HTTP requests
//! through the running server, and asserts the full lifecycle over the wire.
//! Covers: 200 with content, `x-overhead-duration-ms` header, 401 for missing/bad
//! key, and `/health` returns 200. Proves the server genuinely binds and serves.

use std::sync::Arc;

use async_trait::async_trait;
use gateway_control::guard::empty_chain;
use gateway_control::keystore::StaticKeyStore;
use gateway_control::providers::{Deployment, ProviderRegistry};
use gateway_control::state::AppState;
use gateway_llm::{
    ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
    ProviderCapabilities, ProviderError,
};
use gateway_spine::{
    MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
};

/// A mock provider that echoes back "hello" so we don't need a real LLM key.
struct EchoProvider;

#[async_trait]
impl Provider for EchoProvider {
    fn id(&self) -> &str {
        "echo-integration"
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
        Ok(ChatResponse {
            model: req.model.clone(),
            content: vec![ContentPart::text("hello from integration test")],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            provider_response_id: Some("resp_integration".into()),
        })
    }
    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        unreachable!("streaming not tested here")
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

/// Build an AppState with a MockClock + EchoProvider + a valid key "sk-test".
/// NOTE: AppState<MockClock> uses MockClock but `serve` needs SystemClock.
/// We use `router` directly + bind our own listener here to allow MockClock.
async fn build_state() -> Arc<AppState<MockClock>> {
    let mut ks = StaticKeyStore::new();
    ks.insert(VirtualKey {
        id: "key_integration".into(),
        token_hash: VirtualKey::hash_secret("sk-test"),
        token_prefix: "sk-test".into(),
        max_budget: Some(Usd::from_dollars_f64(100.0)),
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
            provider: Arc::new(EchoProvider),
            credentials: Arc::new(Credentials::new("fake")),
        },
    );
    let store = Arc::new(
        gateway_store::Store::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    store
        .upsert_key(&gateway_store::StoredKey {
            id: "key_integration".to_string(),
            name: "key_integration".to_string(),
            token_hash: VirtualKey::hash_secret("sk-test"),
            token_prefix: "sk-test".to_string(),
            budget_micros: Some(Usd::from_dollars_f64(100.0).micros()),
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
    state.ledger.set_budget(
        "key_integration",
        Some(Usd::from_dollars_f64(100.0)),
        Usd::ZERO,
    );
    state
}

/// Start the router on an ephemeral port, return the base URL and a shutdown handle.
async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    use gateway_control::server::router;

    let state = build_state().await;
    let app = router(state);

    // Bind port 0 → OS assigns an ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base_url, handle)
}

#[tokio::test]
async fn health_returns_200() {
    let (base, handle) = start_server().await;
    // Give the server a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("GET /health");

    assert_eq!(resp.status(), 200, "health endpoint must return 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok", "health body must contain status:ok");
    assert!(
        body["version"].is_string(),
        "health body must include version"
    );

    handle.abort();
}

#[tokio::test]
async fn missing_bearer_returns_401() {
    let (base, handle) = start_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .expect("POST without auth");

    assert_eq!(resp.status(), 401, "missing bearer must return 401");

    handle.abort();
}

#[tokio::test]
async fn bad_bearer_returns_401() {
    let (base, handle) = start_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .header("authorization", "Bearer sk-wrong-key-not-valid")
        .header("content-type", "application/json")
        .body(r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .expect("POST with bad bearer");

    assert_eq!(resp.status(), 401, "bad bearer must return 401");

    handle.abort();
}

#[tokio::test]
async fn authenticated_chat_returns_200_with_overhead_header() {
    let (base, handle) = start_server().await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .expect("POST with valid bearer");

    assert_eq!(resp.status(), 200, "authenticated chat must return 200");
    assert!(
        resp.headers().contains_key("x-overhead-duration-ms"),
        "x-overhead-duration-ms header must be present"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(
        body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .contains("hello"),
        "response body must contain echo content"
    );

    handle.abort();
}
