//! The Axum HTTP surface. Handlers are thin: extract the bearer header,
//! deserialize the body, delegate the whole lifecycle to `Gateway`, set the
//! `x-overhead-duration-ms` benchmark header (design §5/§9) and serialize. The
//! router is built over `Arc<AppState<SystemClock>>` for production; tests build
//! it over `Arc<AppState<MockClock>>` and drive it with `tower::ServiceExt`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::StreamExt;
use gateway_spine::{Clock, SystemClock};

use crate::auth::authenticate;
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::sse_out::{delta_to_sse, done_event};
use crate::state::AppState;
use crate::wire::{WireChatRequest, WireChatResponse};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build the full API router over a shared state. Includes `/health` (unauthenticated
/// liveness probe) and all `/v1/*` endpoints. The `/` dashboard is mounted by the
/// binary via `gateway_dash::dash_router()` merged as the fallback layer.
pub fn router<C: Clock + 'static>(state: Arc<AppState<C>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions::<C>))
        .route("/v1/responses", post(chat_completions::<C>))
        .route("/v1/messages", post(chat_completions::<C>))
        .route("/v1/embeddings", post(embeddings::<C>))
        .route("/v1/models", get(models::<C>))
        .with_state(state)
}

/// Bind a `TcpListener` at `addr` and run the gateway server.
/// Use this from `oximy-gateway up` to actually start serving.
pub async fn serve(state: Arc<AppState<SystemClock>>, addr: SocketAddr) -> std::io::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}

/// `GET /health` — unauthenticated liveness probe. Returns `{"status":"ok","version":"..."}`.
async fn health() -> Response {
    Json(serde_json::json!({ "status": "ok", "version": VERSION })).into_response()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
}

async fn chat_completions<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Json(body): Json<WireChatRequest>,
) -> Response {
    let started = Instant::now();
    let key = match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let req = body.to_unified();

    if req.stream {
        match Gateway::run_stream(state.clone(), &key, &req).await {
            Ok(completed) => {
                let model = req.model.clone();
                let overhead = started.elapsed().as_millis() as u64;
                let inner = completed.stream;
                // Map each unified delta to an SSE frame, then append [DONE].
                let sse = inner
                    .map(move |item| {
                        let frame = match item {
                            Ok(delta) => delta_to_sse(&model, &delta, None),
                            Err(e) => format!(
                                "data: {}\n\n",
                                serde_json::json!({"error": {"message": e.to_string()}})
                            ),
                        };
                        Ok::<_, std::convert::Infallible>(frame)
                    })
                    .chain(futures::stream::once(async {
                        Ok::<_, std::convert::Infallible>(done_event())
                    }));
                let mut resp = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("x-overhead-duration-ms", overhead.to_string())
                    .header("x-idempotency-key", completed.idempotency_key)
                    .body(Body::from_stream(sse))
                    .unwrap();
                resp.headers_mut().insert(
                    "cache-control",
                    header::HeaderValue::from_static("no-cache"),
                );
                resp
            }
            Err(e) => e.into_response(),
        }
    } else {
        match Gateway::run(&state, &key, &req).await {
            Ok(completed) => {
                let overhead = started.elapsed().as_millis() as u64;
                let wire = WireChatResponse::from_unified(&completed.response, completed.cost);
                let mut resp = Json(wire).into_response();
                resp.headers_mut().insert(
                    "x-overhead-duration-ms",
                    header::HeaderValue::from_str(&overhead.to_string()).unwrap(),
                );
                resp.headers_mut().insert(
                    "x-idempotency-key",
                    header::HeaderValue::from_str(&completed.idempotency_key).unwrap(),
                );
                resp
            }
            Err(e) => e.into_response(),
        }
    }
}

async fn embeddings<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    // Auth still applies (auth-by-default), then a typed 501 until P5.
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => GatewayError::Unsupported("embeddings".into()).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn models<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    let reg = state.registry.read().unwrap();
    let data: Vec<serde_json::Value> = reg
        .ids()
        .into_iter()
        .filter_map(|id| {
            reg.get(&id).map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "object": "model",
                    "owned_by": e.provider,
                    "context_window": e.context_window,
                    "pricing": {
                        "input_per_mtok_micros": e.price.input_per_mtok,
                        "output_per_mtok_micros": e.price.output_per_mtok,
                    },
                })
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::AllowAll;
    use crate::keystore::StaticKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use gateway_llm::{
        ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
        ProviderCapabilities, ProviderError,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
    };
    use http::Request;
    use tower::ServiceExt;

    struct Echo;
    #[async_trait]
    impl Provider for Echo {
        fn id(&self) -> &str {
            "echo"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
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
                content: vec![ContentPart::text("hello")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage {
                    input_tokens: 1000,
                    output_tokens: 500,
                    ..Default::default()
                },
                provider_response_id: Some("resp_1".into()),
            })
        }
        async fn stream(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            let _ = req;
            let deltas = vec![
                Ok(gateway_llm::StreamDelta::text("hel")),
                Ok(gateway_llm::StreamDelta::text("lo")),
                Ok(gateway_llm::StreamDelta::finish(
                    FinishReason::Stop,
                    TokenUsage {
                        input_tokens: 1000,
                        output_tokens: 500,
                        ..Default::default()
                    },
                )),
            ];
            Ok(Box::pin(futures::stream::iter(deltas)))
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

    fn test_state() -> Arc<AppState<MockClock>> {
        let mut ks = StaticKeyStore::new();
        ks.insert(VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        });
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(Echo),
                credentials: Arc::new(Credentials::new("up")),
            },
        );
        let state = Arc::new(AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);
        state
    }

    #[tokio::test]
    async fn unauthenticated_chat_is_401() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authenticated_chat_returns_cost_and_overhead_header() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("x-overhead-duration-ms"));
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
        assert_eq!(v["usage"]["cost"], 0.0075);
    }

    #[tokio::test]
    async fn streaming_chat_emits_sse_then_done() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("chat.completion.chunk"));
        assert!(text.contains("\"content\":\"hel\""));
        assert!(text.trim_end().ends_with("data: [DONE]"));
    }

    #[tokio::test]
    async fn models_lists_registry() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["object"], "list");
        assert_eq!(v["data"][0]["id"], "gpt-4o");
    }

    #[tokio::test]
    async fn embeddings_authed_is_501() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"text-embedding-3-small","input":"hi"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
