//! The Axum HTTP surface. Handlers are thin: extract the bearer header,
//! deserialize the body, delegate the whole lifecycle to `Gateway`, set the
//! `x-overhead-duration-ms` benchmark header (design §5/§9) and serialize. The
//! router is built over `Arc<AppState<SystemClock>>` for production; tests build
//! it over `Arc<AppState<MockClock>>` and drive it with `tower::ServiceExt`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::StreamExt;
use gateway_cache::CacheControl;
use gateway_spine::{Clock, SystemClock};
use gateway_telemetry::{CacheStatus, CaptureMode, RequestKind, RequestLogRow};

use crate::auth::authenticate;
use crate::cache_handle::{StoreArgs, parse_cache_control};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::sse_out::{delta_to_sse, done_event};
use crate::state::AppState;
use crate::wire::{WireChatRequest, WireChatResponse};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build the full API router over a shared state. Includes `/health` (unauthenticated
/// liveness probe), `/metrics` (authenticated Prometheus, design §2/§11), and all
/// `/v1/*` endpoints. Unknown `/v1/*` paths return 404 — NOT the SPA HTML — so the
/// SPA catch-all in the binary only covers non-API paths. The `/` dashboard is
/// mounted by the binary via `gateway_dash::dash_router()` merged as the fallback
/// layer after this router.
pub fn router<C: Clock + 'static>(state: Arc<AppState<C>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler::<C>))
        .route("/v1/chat/completions", post(chat_completions::<C>))
        .route("/v1/responses", post(chat_completions::<C>))
        .route("/v1/messages", post(chat_completions::<C>))
        .route("/v1/embeddings", post(embeddings::<C>))
        .route("/v1/models", get(models::<C>))
        // Authenticated MCP gateway (JSON-RPC 2.0 over the federation).
        .route("/mcp", post(crate::mcp::mcp_handler::<C>))
        // Explicit 404 for any other /v1/* path so the SPA fallback never
        // intercepts an API miss and falsely returns 200 with HTML.
        .route("/v1/{*rest}", get(v1_not_found).post(v1_not_found))
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

/// `GET /metrics` — authenticated Prometheus text exposition (design §2/§11).
/// Uses the same bearer authentication as all other API endpoints (any valid
/// virtual key), so `/metrics` is never accidentally open.
async fn metrics_handler<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    // Auth-by-default: same path as every other authenticated handler.
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    let text = state.metrics.render();
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            gateway_telemetry::METRICS_CONTENT_TYPE,
        )],
        text,
    )
        .into_response()
}

/// Catch-all for unknown `/v1/*` paths — always 404 (never the SPA HTML).
async fn v1_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": { "message": "not found", "type": "invalid_request_error" } })),
    )
        .into_response()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
}

async fn chat_completions<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    raw: axum::body::Bytes,
) -> Response {
    let started = Instant::now();
    // Authenticate BEFORE parsing the body — an unauthenticated request must get
    // 401 regardless of Content-Type, and we never parse bodies for unauthorized
    // callers (defense in depth).
    let key = match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let body: WireChatRequest = match serde_json::from_slice(&raw) {
        Ok(b) => b,
        Err(e) => {
            return GatewayError::BadRequest(format!("invalid JSON request body: {e}"))
                .into_response();
        }
    };
    let req = body.to_unified();

    if req.stream {
        // Streaming: no cache (noted: streaming cache replay is deferred to P2).
        match Gateway::run_stream(state.clone(), &key, &req).await {
            Ok(completed) => {
                let model = req.model.clone();
                let overhead = started.elapsed().as_millis() as u64;
                let inner = completed.stream;

                // Capture values needed for the telemetry row after the stream ends.
                let telem_sink = state.telemetry.clone();
                let telem_key_id = key.id.clone();
                let telem_model = req.model.clone();
                let telem_ts_ms = state.clock.now_ms();

                // Map each unified delta to an SSE frame, then append [DONE].
                // Track the last usage delta to build the telemetry row on completion.
                // Use a shared slot so the map and chain closures can both reach it.
                let last_usage: Arc<Mutex<Option<gateway_spine::TokenUsage>>> =
                    Arc::new(Mutex::new(None));
                let last_usage_writer = Arc::clone(&last_usage);
                let sse = inner
                    .map(move |item| {
                        let frame = match item {
                            Ok(ref delta) => {
                                if let Some(u) = delta.usage {
                                    *last_usage_writer.lock().unwrap() = Some(u);
                                }
                                delta_to_sse(&model, delta, None)
                            }
                            Err(ref e) => format!(
                                "data: {}\n\n",
                                serde_json::json!({"error": {"message": e.to_string()}})
                            ),
                        };
                        Ok::<_, std::convert::Infallible>(frame)
                    })
                    .chain(futures::stream::once({
                        async move {
                            // Fire telemetry on stream completion (off hot path — try_send only).
                            // usage may be None if the stream was empty/errored.
                            let usage = last_usage.lock().unwrap().unwrap_or_default();
                            telem_sink.log(RequestLogRow {
                                ts_ms: telem_ts_ms,
                                kind: RequestKind::Llm,
                                key_id: telem_key_id,
                                team_id: None,
                                user_id: None,
                                tags: vec![],
                                model: telem_model,
                                provider: String::new(),
                                usage,
                                cost: gateway_spine::Usd::ZERO,
                                latency_ms: overhead as i64,
                                ttft_ms: None,
                                status: 200,
                                served_by: String::new(),
                                fallback_fired: false,
                                cache_status: CacheStatus::Bypass,
                                capture_mode: CaptureMode::Metadata,
                                request_text: None,
                                response_text: None,
                            });
                            Ok::<_, std::convert::Infallible>(done_event())
                        }
                    }));
                let mut resp = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("x-overhead-duration-ms", overhead.to_string())
                    .header("x-idempotency-key", completed.idempotency_key)
                    .header("x-cache", "BYPASS") // streaming is not cached
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
        // Non-streaming: check L1 cache before calling the provider.

        // Parse cache-control directives from the `x-oximy-cache` header.
        let cache_ctl = parse_cache_control(&headers);

        // Reconstruct the body as a JSON value for cache-key hashing. Use a
        // deterministic subset (model + messages + temperature + max_tokens).
        let body_value = serde_json::json!({
            "model": body.model,
            "messages": body.messages.iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "temperature": body.temperature,
            "max_tokens": body.max_tokens,
        });

        // Cache lookup (bypass if no cache installed or skip directive set).
        if let Some(cache) = &state.cache {
            let outcome = cache
                .lookup(
                    "default",
                    "/v1/chat/completions",
                    &req.model,
                    &body_value,
                    &cache_ctl,
                )
                .await;
            // Collapse: HIT AND entry present → serve from cache.
            if outcome.status == gateway_cache::CacheStatus::Hit
                && let Some(entry) = outcome.value
            {
                // HIT: reconstruct the response from cache, charge $0.
                let age_ms = outcome.age_ms.unwrap_or(0);
                let overhead = started.elapsed().as_millis() as u64;
                let cached_resp = match entry.body {
                    gateway_cache::CachedBody::Unary(r) => r,
                    gateway_cache::CachedBody::Stream(_) => {
                        // Stream body in cache is not served as unary — treat as MISS.
                        return run_unary_miss(
                            state, &key, &req, body_value, cache_ctl, started, headers,
                        )
                        .await;
                    }
                };
                // Log telemetry with $0 cost (cache hit = no provider call).
                state.telemetry.log(RequestLogRow {
                    ts_ms: state.clock.now_ms(),
                    kind: RequestKind::Llm,
                    key_id: key.id.clone(),
                    team_id: None,
                    user_id: None,
                    tags: vec![],
                    model: req.model.clone(),
                    provider: "cache".into(),
                    usage: entry.usage,
                    cost: gateway_spine::Usd::ZERO,
                    latency_ms: overhead as i64,
                    ttft_ms: None,
                    status: 200,
                    served_by: "cache".into(),
                    fallback_fired: false,
                    cache_status: CacheStatus::Hit,
                    capture_mode: CaptureMode::Metadata,
                    request_text: None,
                    response_text: None,
                });
                let wire = WireChatResponse::from_unified(&cached_resp, gateway_spine::Usd::ZERO);
                let mut resp = Json(wire).into_response();
                let h = resp.headers_mut();
                h.insert(
                    "x-overhead-duration-ms",
                    header::HeaderValue::from_str(&overhead.to_string()).unwrap(),
                );
                h.insert("x-cache", header::HeaderValue::from_static("HIT"));
                h.insert(
                    "x-cache-age-ms",
                    header::HeaderValue::from_str(&age_ms.to_string()).unwrap(),
                );
                h.insert(
                    "x-idempotency-key",
                    header::HeaderValue::from_static("cached"),
                );
                h.insert("x-served-by", header::HeaderValue::from_static("cache"));
                h.insert(
                    "x-fallback-fired",
                    header::HeaderValue::from_static("false"),
                );
                return resp;
            }
        }

        run_unary_miss(state, &key, &req, body_value, cache_ctl, started, headers).await
    }
}

/// Execute a non-streaming call that was a cache MISS (or has no cache).
/// On success, store the response in the cache before returning.
async fn run_unary_miss<C: Clock + 'static>(
    state: Arc<AppState<C>>,
    key: &gateway_spine::VirtualKey,
    req: &gateway_llm::ChatRequest,
    body_value: serde_json::Value,
    cache_ctl: CacheControl,
    started: Instant,
    _headers: HeaderMap,
) -> Response {
    match Gateway::run(&state, key, req).await {
        Ok(completed) => {
            let overhead = started.elapsed().as_millis() as u64;
            let latency_ms = overhead as i64;

            // Store in cache on success (non-streaming 200).
            if let Some(cache) = &state.cache {
                cache
                    .store_unary(StoreArgs {
                        tenant_id: "default",
                        endpoint: "/v1/chat/completions",
                        model: &req.model,
                        body: &body_value,
                        ctl: &cache_ctl,
                        response: completed.response.clone(),
                        usage: completed.response.usage,
                        original_cost: completed.cost,
                    })
                    .await;
            }

            // Log the row off the hot path — non-blocking try_send.
            state.telemetry.log(RequestLogRow {
                ts_ms: state.clock.now_ms(),
                kind: RequestKind::Llm,
                key_id: key.id.clone(),
                team_id: None,
                user_id: None,
                tags: vec![],
                model: req.model.clone(),
                provider: String::new(),
                usage: completed.response.usage,
                cost: completed.cost,
                latency_ms,
                ttft_ms: None,
                status: 200,
                served_by: completed.served_by.clone(),
                fallback_fired: completed.fallback_fired,
                cache_status: CacheStatus::Miss,
                capture_mode: CaptureMode::Metadata,
                request_text: None,
                response_text: None,
            });

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
            if let Ok(v) = header::HeaderValue::from_str(&completed.served_by) {
                resp.headers_mut().insert("x-served-by", v);
            }
            resp.headers_mut().insert(
                "x-fallback-fired",
                header::HeaderValue::from_static(if completed.fallback_fired {
                    "true"
                } else {
                    "false"
                }),
            );
            resp.headers_mut()
                .insert("x-cache", header::HeaderValue::from_static("MISS"));
            resp
        }
        Err(e) => e.into_response(),
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
    use crate::cache_handle::memory_cache_handle;
    use crate::guard::empty_chain;
    use crate::keystore::StaticKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use gateway_llm::{
        ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
        ProviderCapabilities, ProviderError,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, SystemClock, TokenUsage, Usd,
        VirtualKey,
    };
    use http::Request;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// A counting provider that records how many times it was called.
    struct CountingProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for CountingProvider {
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
                content: vec![ContentPart::text("provider-response")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    ..Default::default()
                },
                provider_response_id: Some("resp_cached".into()),
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
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);
        state
    }

    /// Build a state with a cache and a counting provider (SystemClock required for cache).
    fn state_with_cache(calls: Arc<AtomicUsize>) -> Arc<AppState<SystemClock>> {
        let mut ks = StaticKeyStore::new();
        ks.insert(VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".into(),
            max_budget: Some(Usd::from_dollars_f64(100.0)),
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
                provider: Arc::new(CountingProvider { calls }),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        let mut state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(SystemClock),
            providers,
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(100.0)), Usd::ZERO);
        // Install the in-memory cache.
        state.cache = Some(memory_cache_handle(SystemClock, 3600));
        Arc::new(state)
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

    // ── New tests for P1.7 ────────────────────────────────────────────────────

    #[tokio::test]
    async fn metrics_without_bearer_is_401() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_with_bad_token_is_401() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .header("authorization", "Bearer sk-wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_with_valid_bearer_returns_prometheus_text() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Content-Type must be the OpenMetrics MIME type (design §2/§11).
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.contains("application/openmetrics-text"),
            "expected OpenMetrics content-type, got {ct:?}"
        );
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        // Body must contain at least one registered series name.
        assert!(
            text.contains("gateway_requests") || text.contains("gateway_dropped_rows"),
            "expected Prometheus series in body, got: {text}"
        );
    }

    #[tokio::test]
    async fn unknown_v1_path_is_404_not_spa_html() {
        // Before the fix, the SPA catch-all intercepted /v1/* and returned 200
        // with HTML.  After the fix, the explicit fallback returns 404 JSON.
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/nonexistent")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        // Must be JSON, not HTML.
        assert!(
            !text.contains("<!DOCTYPE"),
            "expected JSON error, got HTML: {text}"
        );
    }

    #[tokio::test]
    async fn successful_chat_records_telemetry_row() {
        use gateway_telemetry::spawn as spawn_telem;
        use gateway_telemetry::{DEFAULT_CHANNEL_CAPACITY, GatewayMetrics, MemorySpendStore};

        let metrics = Arc::new(GatewayMetrics::new());
        let store = Arc::new(MemorySpendStore::new());
        let (sink, _writer) = spawn_telem(
            Arc::clone(&store),
            Arc::clone(&metrics),
            DEFAULT_CHANNEL_CAPACITY,
        );

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
        let state = Arc::new(AppState::with_parts_and_telemetry(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
            sink,
            Arc::clone(&metrics),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);

        let app = router(state);
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
        // Drain: give the async writer a few ticks to process the row.
        // Call row_count via the trait (Arc<MemorySpendStore> needs explicit deref).
        use gateway_telemetry::SpendStore;
        for _ in 0..500 {
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            if SpendStore::row_count(store.as_ref()) >= 1 {
                break;
            }
        }
        // The store should have received at least one telemetry row.
        assert!(
            SpendStore::row_count(store.as_ref()) >= 1,
            "telemetry row not recorded after chat"
        );
        // The Prometheus metrics text must reference the request counter.
        let prom_text = metrics.render();
        assert!(
            prom_text.contains("gateway_requests_total"),
            "metrics counter not incremented: {prom_text}"
        );
    }

    // ── Cache integration tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn cache_miss_returns_x_cache_miss_header() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = state_with_cache(Arc::clone(&calls));
        let app = router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"cache-test"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("x-cache").unwrap(), "MISS");
        // Provider was called once.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_hit_skips_provider_and_returns_hit_header() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = state_with_cache(Arc::clone(&calls));

        // First call: MISS — populates cache.
        let resp1 = router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"cache-hit-test"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        assert_eq!(resp1.headers().get("x-cache").unwrap(), "MISS");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second call: HIT — provider NOT called, x-cache: HIT.
        let resp2 = router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"cache-hit-test"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        assert_eq!(resp2.headers().get("x-cache").unwrap(), "HIT");
        // Provider must NOT have been called again.
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "provider must NOT be called on cache HIT"
        );
        let bytes = to_bytes(resp2.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // The cached response must be the provider's response.
        assert_eq!(v["choices"][0]["message"]["content"], "provider-response");
        // Cost must be $0 on a cache HIT.
        assert_eq!(v["usage"]["cost"], 0.0);
    }

    #[tokio::test]
    async fn cache_no_store_directive_skips_write() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = state_with_cache(Arc::clone(&calls));

        // First call with no-store: MISS, response NOT stored.
        let resp1 = router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .header("x-oximy-cache", "no-store")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"no-store-test"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second identical call: still MISS (nothing stored).
        let resp2 = router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"no-store-test"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        assert_eq!(resp2.headers().get("x-cache").unwrap(), "MISS");
        // Provider called twice (nothing was cached).
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
