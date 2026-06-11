//! The per-request lifecycle, free of HTTP. `Gateway::run` executes the design
//! §6 order exactly: authenticate (caller passes the resolved key) → allowlist
//! → rate-limit acquire → budget reserve → guard pre-hook → provider egress
//! (with the idempotency key) → commit ACTUAL cost from provider usage →
//! release the parallel slot → return the response + computed `usage.cost`.
//!
//! Failure handling is fail-closed and leak-free: a denial before egress
//! releases nothing (nothing was acquired past the failing step); a denial or
//! error AFTER reserve releases the reservation and the parallel slot so a
//! failed request never strands budget or a concurrency slot. The idempotency
//! key is minted once here and is the SAME value the provider sees on any retry
//! (retries are P1.5; the seam is correct now).

use std::sync::Arc;

use futures::stream::{Stream, StreamExt};
use gateway_guard::{GuardContext, GuardStage, GuardVerdict};
use gateway_llm::{ChatRequest, ChatResponse, ContentPart, ProviderError, Role, StreamDelta};
use gateway_spine::{AuditEvent, ReservationId, Usd, VirtualKey};

use crate::error::GatewayError;
use crate::state::AppState;

/// A completed non-streaming call: the provider response plus the authoritative
/// cost committed to the ledger (`usage.cost`).
#[derive(Debug)]
pub struct Completed {
    pub response: ChatResponse,
    pub cost: Usd,
    pub idempotency_key: String,
}

/// The output of a streaming run: the wrapped delta stream (commit-on-terminal)
/// plus the minted idempotency key for response headers. The stream yields the
/// SAME `StreamDelta`s the provider produced; the cost commit is a side effect
/// that fires on the terminal (usage-carrying) delta.
pub struct CompletedStream {
    pub stream: std::pin::Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>> + Send>>,
    pub idempotency_key: String,
}

/// A conservative per-request token estimate used for the pre-call budget
/// reservation and TPM check. True-up happens at commit from real usage.
fn estimate_tokens(req: &ChatRequest) -> i64 {
    // ~4 chars/token over all text parts, plus the max_tokens ceiling for output.
    let input_chars: usize = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|p| match p {
            gateway_llm::ContentPart::Text { text } => Some(text.len()),
            _ => None,
        })
        .sum();
    let input_est = (input_chars / 4).max(1) as i64;
    let output_est = req.max_tokens.unwrap_or(1024);
    input_est + output_est
}

impl<C: gateway_spine::Clock> AppState<C> {
    /// Estimate the worst-case USD for a request, for the fail-closed reserve.
    /// Uses the model's price if known; an unknown model is a hard error here
    /// (cost-correctness: we never reserve against a guessed price).
    fn estimate_cost(&self, model: &str, est_tokens: i64) -> Result<Usd, GatewayError> {
        let reg = self.registry.read().unwrap();
        let entry = reg.get(model).ok_or_else(|| {
            GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                model: model.to_string(),
            })
        })?;
        // Treat the whole estimate as output tokens (the most expensive bucket).
        let usage = gateway_spine::TokenUsage {
            output_tokens: est_tokens,
            ..Default::default()
        };
        Ok(entry.price.cost(&usage))
    }
}

/// Concatenate all user-authored prompt text in a request — the surface the
/// `PreRequest` guard inspects. System/assistant/tool turns are excluded so the
/// guard reasons about the caller's own content.
fn user_prompt_text(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace every user message's text content with `masked` (single combined
/// blob) while preserving non-text parts (images) and non-user turns. Used when
/// the `PreRequest` guard returns a `Mask` verdict.
fn apply_user_mask(req: &mut ChatRequest, masked: &str) {
    let mut applied = false;
    for m in req.messages.iter_mut() {
        if m.role != Role::User {
            continue;
        }
        // Keep any image parts; replace the text with the masked blob exactly once.
        let images: Vec<ContentPart> = m
            .content
            .iter()
            .filter(|p| matches!(p, ContentPart::Image { .. }))
            .cloned()
            .collect();
        m.content.clear();
        if !applied {
            m.content.push(ContentPart::text(masked.to_string()));
            applied = true;
        }
        m.content.extend(images);
    }
}

/// Replace a response's text content with `masked`, preserving any non-text
/// (image) parts and the tool calls. Used when the `PostResponse` guard masks.
fn mask_response_text(resp: &mut ChatResponse, masked: &str) {
    let images: Vec<ContentPart> = resp
        .content
        .iter()
        .filter(|p| matches!(p, ContentPart::Image { .. }))
        .cloned()
        .collect();
    resp.content.clear();
    resp.content.push(ContentPart::text(masked.to_string()));
    resp.content.extend(images);
}

/// The lifecycle. Generic over the clock so tests inject `MockClock`.
pub struct Gateway;

impl Gateway {
    /// Run one non-streaming chat call end-to-end. `key` is the already
    /// authenticated `VirtualKey` (the handler resolves it via `auth`).
    pub async fn run<C: gateway_spine::Clock>(
        state: &AppState<C>,
        key: &VirtualKey,
        req: &ChatRequest,
    ) -> Result<Completed, GatewayError> {
        // Owned request: the PreRequest guard may rewrite the prompt (Mask), so
        // the value we hand to egress can differ from the caller's input.
        let mut req = req.clone();
        let req = &mut req;
        let model = req.model.clone();
        let model = model.as_str();

        // 1. model allowlist
        if !key.allows_model(model) {
            Self::audit(
                state,
                key,
                "request.denied",
                model,
                "denied",
                "model_not_allowed",
            );
            return Err(GatewayError::Spine(
                gateway_spine::SpineError::ModelNotAllowed {
                    key_id: key.id.clone(),
                    model: model.to_string(),
                },
            ));
        }

        // 2. resolve the egress deployment for this model's provider (unknown
        //    model → 400 here, before any acquisition).
        let provider_id = {
            let reg = state.registry.read().unwrap();
            reg.get(model).map(|e| e.provider.clone()).ok_or_else(|| {
                GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                    model: model.to_string(),
                })
            })?
        };
        let deployment = state.providers.get(&provider_id).cloned().ok_or_else(|| {
            GatewayError::BadRequest(format!("no egress configured for provider {provider_id}"))
        })?;

        let est_tokens = estimate_tokens(req);

        // 3. rate-limit acquire (RPM/TPM/parallel). On failure nothing else has
        //    been acquired, so just propagate.
        state
            .limiter
            .acquire(&key.id, &key.limits, est_tokens)
            .map_err(|e| {
                Self::audit(
                    state,
                    key,
                    "request.denied",
                    model,
                    "denied",
                    "rate_limited",
                );
                GatewayError::Spine(e)
            })?;

        // 4. budget reserve (fail-closed, BEFORE egress). On failure, release the
        //    parallel slot we just acquired, then propagate.
        let est_cost = match state.estimate_cost(model, est_tokens) {
            Ok(c) => c,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(e);
            }
        };
        let reservation = match state.ledger.reserve(&key.id, est_cost) {
            Ok(r) => r,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                Self::audit(
                    state,
                    key,
                    "request.denied",
                    model,
                    "denied",
                    "budget_exceeded",
                );
                return Err(GatewayError::Spine(e));
            }
        };

        // 5. PreRequest guard over the user prompt. A Block releases the
        //    reservation + parallel slot before any egress (403); a Mask rewrites
        //    the outgoing prompt in place. Runs BEFORE egress so a secret-laden
        //    prompt is never forwarded upstream.
        let pre_ctx = GuardContext {
            stage: GuardStage::PreRequest,
            text: user_prompt_text(req),
            key_id: Some(key.id.clone()),
            model: Some(model.to_string()),
            tags: vec![],
        };
        match state.guard.run(&pre_ctx).await.final_verdict {
            GuardVerdict::Block { reason } => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                Self::audit(state, key, "request.denied", model, "denied", &reason);
                return Err(GatewayError::GuardBlocked(reason));
            }
            GuardVerdict::Mask { redacted_text } => {
                apply_user_mask(req, &redacted_text);
                Self::audit(state, key, "guard.masked", model, "masked", "pre_request");
            }
            GuardVerdict::Allow | GuardVerdict::Flag { .. } => {}
        }

        // 6. egress — mint ONE idempotency key (no-double-billing) and call the
        //    provider. Any error releases the reservation + parallel slot.
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let result = deployment
            .provider
            .chat(req, &deployment.credentials, &idempotency_key)
            .await;
        let mut response = match result {
            Ok(r) => r,
            Err(e) => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                Self::audit(state, key, "request.error", model, "error", &e.to_string());
                return Err(GatewayError::Provider(e));
            }
        };

        // 7. commit ACTUAL cost from provider usage (true-up). Cost is computed
        //    from the registry — never guessed. Then release the parallel slot.
        //    Cost is committed regardless of the PostResponse guard verdict: the
        //    provider was billed, so we record the true spend even if we then
        //    block/redact the content the caller sees.
        let actual_cost = {
            let reg = state.registry.read().unwrap();
            reg.cost(model, &response.usage).unwrap_or(Usd::ZERO)
        };
        state
            .ledger
            .commit(reservation, actual_cost)
            .map_err(GatewayError::Spine)?;
        state.limiter.release_parallel(&key.id);

        // 8. PostResponse guard over the assistant output. Block → 403 (the
        //    completion is withheld); Mask → redact the returned text in place.
        let post_ctx = GuardContext {
            stage: GuardStage::PostResponse,
            text: response.text(),
            key_id: Some(key.id.clone()),
            model: Some(model.to_string()),
            tags: vec![],
        };
        match state.guard.run(&post_ctx).await.final_verdict {
            GuardVerdict::Block { reason } => {
                Self::audit(state, key, "response.blocked", model, "denied", &reason);
                return Err(GatewayError::GuardBlocked(reason));
            }
            GuardVerdict::Mask { redacted_text } => {
                mask_response_text(&mut response, &redacted_text);
                Self::audit(state, key, "guard.masked", model, "masked", "post_response");
            }
            GuardVerdict::Allow | GuardVerdict::Flag { .. } => {}
        }

        Self::audit(
            state,
            key,
            "request.complete",
            model,
            "ok",
            &format!("{} µUSD", actual_cost.micros()),
        );

        Ok(Completed {
            response,
            cost: actual_cost,
            idempotency_key,
        })
    }

    /// Streaming variant. Same admission order as `run`; egress returns a
    /// delta stream that we wrap to commit the actual cost from the terminal
    /// delta's usage and release the parallel slot when the stream ends. If the
    /// stream is dropped before completion (client abort), the wrapper commits
    /// whatever usage arrived and releases the slot — usage is never lost and a
    /// reservation is never stranded.
    pub async fn run_stream<C: gateway_spine::Clock + 'static>(
        state: Arc<AppState<C>>,
        key: &VirtualKey,
        req: &ChatRequest,
    ) -> Result<CompletedStream, GatewayError> {
        // Owned request so the PreRequest guard can rewrite the prompt (Mask).
        let mut req = req.clone();
        let req = &mut req;
        let model = req.model.clone();

        if !key.allows_model(&model) {
            Self::audit(
                &state,
                key,
                "request.denied",
                &model,
                "denied",
                "model_not_allowed",
            );
            return Err(GatewayError::Spine(
                gateway_spine::SpineError::ModelNotAllowed {
                    key_id: key.id.clone(),
                    model: model.clone(),
                },
            ));
        }

        let provider_id = {
            let reg = state.registry.read().unwrap();
            reg.get(&model).map(|e| e.provider.clone()).ok_or_else(|| {
                GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                    model: model.clone(),
                })
            })?
        };
        let deployment = state.providers.get(&provider_id).cloned().ok_or_else(|| {
            GatewayError::BadRequest(format!("no egress configured for provider {provider_id}"))
        })?;

        let est_tokens = estimate_tokens(req);

        state
            .limiter
            .acquire(&key.id, &key.limits, est_tokens)
            .map_err(GatewayError::Spine)?;

        let est_cost = match state.estimate_cost(&model, est_tokens) {
            Ok(c) => c,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(e);
            }
        };
        let reservation = match state.ledger.reserve(&key.id, est_cost) {
            Ok(r) => r,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(GatewayError::Spine(e));
            }
        };

        // PreRequest guard over the prompt. Block → 403 before egress; Mask →
        // rewrite the prompt. PostResponse content guarding is not applied to
        // streamed output (we never buffer the whole stream — that would defeat
        // streaming); secrets/PII on the egress prompt are still caught here.
        let pre_ctx = GuardContext {
            stage: GuardStage::PreRequest,
            text: user_prompt_text(req),
            key_id: Some(key.id.clone()),
            model: Some(model.clone()),
            tags: vec![],
        };
        match state.guard.run(&pre_ctx).await.final_verdict {
            GuardVerdict::Block { reason } => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                Self::audit(&state, key, "request.denied", &model, "denied", &reason);
                return Err(GatewayError::GuardBlocked(reason));
            }
            GuardVerdict::Mask { redacted_text } => {
                apply_user_mask(req, &redacted_text);
            }
            GuardVerdict::Allow | GuardVerdict::Flag { .. } => {}
        }

        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let inner = match deployment
            .provider
            .stream(req, &deployment.credentials, &idempotency_key)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                return Err(GatewayError::Provider(e));
            }
        };

        let wrapped =
            Self::wrap_stream_for_commit(state, key.id.clone(), model, reservation, inner);
        Ok(CompletedStream {
            stream: Box::pin(wrapped),
            idempotency_key,
        })
    }

    /// Wrap a provider delta stream so the terminal usage commits cost + releases
    /// the parallel slot exactly once, whether the stream completes or is dropped.
    fn wrap_stream_for_commit<C: gateway_spine::Clock + 'static>(
        state: Arc<AppState<C>>,
        key_id: String,
        model: String,
        reservation: ReservationId,
        inner: std::pin::Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>> + Send>>,
    ) -> impl Stream<Item = Result<StreamDelta, ProviderError>> + Send {
        // State carried across the stream: the latest usage seen + a guard that
        // commits on Drop so an aborted stream still trues-up.
        struct CommitGuard<C: gateway_spine::Clock> {
            state: Arc<AppState<C>>,
            key_id: String,
            model: String,
            reservation: Option<ReservationId>,
            last_usage: Option<gateway_spine::TokenUsage>,
        }
        impl<C: gateway_spine::Clock> Drop for CommitGuard<C> {
            fn drop(&mut self) {
                if let Some(res) = self.reservation.take() {
                    let cost = {
                        let reg = self.state.registry.read().unwrap();
                        self.last_usage
                            .and_then(|u| reg.cost(&self.model, &u))
                            .unwrap_or(Usd::ZERO)
                    };
                    let _ = self.state.ledger.commit(res, cost);
                    self.state.limiter.release_parallel(&self.key_id);
                    self.state.audit.record(AuditEvent {
                        ts_ms: self.state.clock.now_ms(),
                        actor: self.key_id.clone(),
                        action: "request.complete".into(),
                        target: self.model.clone(),
                        outcome: "ok".into(),
                        detail: Some(format!("{} µUSD (stream)", cost.micros())),
                    });
                }
            }
        }

        let guard = CommitGuard {
            state,
            key_id,
            model,
            reservation: Some(reservation),
            last_usage: None,
        };

        futures::stream::unfold((inner, guard), |(mut inner, mut guard)| async move {
            match inner.next().await {
                Some(Ok(delta)) => {
                    if let Some(u) = delta.usage {
                        guard.last_usage = Some(u);
                    }
                    Some((Ok(delta), (inner, guard)))
                }
                Some(Err(e)) => Some((Err(e), (inner, guard))),
                None => None, // `guard` drops here → commit fires
            }
        })
    }

    fn audit<C: gateway_spine::Clock>(
        state: &AppState<C>,
        key: &VirtualKey,
        action: &str,
        target: &str,
        outcome: &str,
        detail: &str,
    ) {
        state.audit.record(AuditEvent {
            ts_ms: state.clock.now_ms(),
            actor: key.id.clone(),
            action: action.to_string(),
            target: target.to_string(),
            outcome: outcome.to_string(),
            detail: Some(detail.to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::empty_chain;
    use crate::keystore::StaticKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use gateway_llm::{
        ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Message, Provider,
        ProviderCapabilities, ProviderError, Role,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A provider that records how many times it was called + the last
    /// idempotency key, and returns a fixed usage.
    struct MockProvider {
        calls: AtomicUsize,
        last_idem: std::sync::Mutex<Option<String>>,
        last_prompt: std::sync::Mutex<Option<String>>,
        usage: TokenUsage,
    }

    impl MockProvider {
        fn new(usage: TokenUsage) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                last_idem: std::sync::Mutex::new(None),
                last_prompt: std::sync::Mutex::new(None),
                usage,
            }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
                supports_tools: true,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_idem.lock().unwrap() = Some(idempotency_key.to_string());
            *self.last_prompt.lock().unwrap() = Some(
                req.messages
                    .iter()
                    .map(|m| m.text_content())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![ContentPart::text("hello")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: self.usage,
                provider_response_id: Some("resp_1".into()),
            })
        }
        async fn stream(
            &self,
            _req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            unreachable!("non-streaming test")
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

    fn key(budget: Option<Usd>, allow: Option<Vec<String>>, limits: RateLimits) -> VirtualKey {
        VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-test"),
            token_prefix: "sk-test".into(),
            max_budget: budget,
            limits,
            model_allowlist: allow,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    /// Build an AppState wired to a shared MockProvider so tests can inspect it.
    fn state_with(provider: Arc<MockProvider>, budget: Option<Usd>) -> AppState<MockClock> {
        let mut ks = StaticKeyStore::new();
        ks.insert(key(budget, None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: provider.clone(),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        let state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(1_000)),
            providers,
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", budget, Usd::ZERO);
        state
    }

    /// Like `state_with` but installs the production `default_chain` guard
    /// (secrets-block + PII-mask) so guard integration can be exercised.
    fn state_with_default_guard(
        provider: Arc<MockProvider>,
        budget: Option<Usd>,
    ) -> AppState<MockClock> {
        let mut ks = StaticKeyStore::new();
        ks.insert(key(budget, None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: provider.clone(),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        let state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(1_000)),
            providers,
            Arc::new(crate::guard::default_chain()),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", budget, Usd::ZERO);
        state
    }

    fn chat_req() -> ChatRequest {
        ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi there")])
    }

    #[tokio::test]
    async fn guard_blocks_secret_prompt_before_egress() {
        let provider = Arc::new(MockProvider::new(TokenUsage::default()));
        let state = state_with_default_guard(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );

        // A live OpenAI-shaped key in the prompt must be blocked pre-egress.
        let req = ChatRequest::new(
            "gpt-4o",
            vec![Message::text(
                Role::User,
                "please use my key sk-ant-api03-FAKEFAKEFAKEFAKEFAKE to call the api",
            )],
        );
        let err = Gateway::run(&state, &k, &req).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(matches!(err, GatewayError::GuardBlocked(_)));
        // The provider must NOT have been called, and no spend/reservation leaked.
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            0,
            "secret prompt must never reach the provider"
        );
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO);
        assert_eq!(state.ledger.spent("key_1"), Usd::ZERO);
    }

    #[tokio::test]
    async fn guard_masks_pii_prompt_then_calls_provider() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let provider = Arc::new(MockProvider::new(usage));
        let state = state_with_default_guard(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );

        let req = ChatRequest::new(
            "gpt-4o",
            vec![Message::text(
                Role::User,
                "email me at alice@example.com about the order",
            )],
        );
        // PII is masked, not blocked — the call still succeeds.
        let done = Gateway::run(&state, &k, &req).await.unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(done.response.text(), "hello");
        // The provider saw the masked prompt (no raw email).
        let seen = provider.last_prompt.lock().unwrap().clone().unwrap();
        assert!(
            !seen.contains("alice@example.com"),
            "provider must see redacted prompt, got: {seen}"
        );
        assert!(seen.contains("[EMAIL]"));
    }

    #[tokio::test]
    async fn guard_allows_clean_prompt() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let provider = Arc::new(MockProvider::new(usage));
        let state = state_with_default_guard(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );
        let done = Gateway::run(&state, &k, &chat_req()).await.unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(done.response.text(), "hello");
    }

    #[tokio::test]
    async fn happy_path_commits_actual_cost() {
        // 1000 in + 500 out → $0.0025 + $0.005 = $0.0075 = 7_500 µUSD
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        let provider = Arc::new(MockProvider::new(usage));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );

        let done = Gateway::run(&state, &k, &chat_req()).await.unwrap();

        assert_eq!(done.cost, Usd::from_micros(7_500));
        assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
        assert_eq!(
            state.ledger.reserved("key_1"),
            Usd::ZERO,
            "reservation trued up"
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        // idempotency key was minted + passed
        assert!(provider.last_idem.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn budget_exceeded_never_calls_provider() {
        let usage = TokenUsage {
            output_tokens: 500,
            ..Default::default()
        };
        let provider = Arc::new(MockProvider::new(usage));
        // tiny budget that the worst-case estimate blows
        let state = state_with(provider.clone(), Some(Usd::from_micros(1)));
        let k = key(Some(Usd::from_micros(1)), None, RateLimits::default());

        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            0,
            "fail-closed: no egress"
        );
        // no stranded reservation or parallel slot
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO);
    }

    #[tokio::test]
    async fn disallowed_model_is_403_no_egress() {
        let provider = Arc::new(MockProvider::new(TokenUsage::default()));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            Some(vec!["claude-3-5-sonnet".into()]),
            RateLimits::default(),
        );
        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unknown_model_is_400() {
        let provider = Arc::new(MockProvider::new(TokenUsage::default()));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );
        let req = ChatRequest::new("mystery", vec![Message::text(Role::User, "hi")]);
        let err = Gateway::run(&state, &k, &req).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn provider_error_releases_reservation() {
        struct Failing;
        #[async_trait]
        impl Provider for Failing {
            fn id(&self) -> &str {
                "fail"
            }
            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_streaming: false,
                    supports_tools: false,
                    supports_vision: false,
                    supports_idempotency: false,
                }
            }
            async fn chat(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                Err(ProviderError::Upstream {
                    status: 500,
                    body: "boom".into(),
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

        let mut ks = StaticKeyStore::new();
        ks.insert(key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        ));
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(Failing),
                credentials: Arc::new(Credentials::new("x")),
            },
        );
        let state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );
        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(
            state.ledger.reserved("key_1"),
            Usd::ZERO,
            "released on egress error"
        );
        assert_eq!(state.ledger.spent("key_1"), Usd::ZERO, "nothing billed");
    }

    #[tokio::test]
    async fn streaming_commits_from_terminal_delta_usage() {
        use gateway_llm::FinishReason;

        struct StreamProvider {
            usage: TokenUsage,
        }
        #[async_trait]
        impl Provider for StreamProvider {
            fn id(&self) -> &str {
                "stream"
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
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn stream(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<DeltaStream, ProviderError> {
                let deltas = vec![
                    Ok(StreamDelta::text("hel")),
                    Ok(StreamDelta::text("lo")),
                    Ok(StreamDelta::finish(FinishReason::Stop, self.usage)),
                ];
                Ok(Box::pin(futures::stream::iter(deltas)))
            }
        }

        let mut ks = StaticKeyStore::new();
        ks.insert(key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        ));
        let mut providers = ProviderRegistry::new();
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(StreamProvider { usage }),
                credentials: Arc::new(Credentials::new("x")),
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
            .set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );
        let completed = Gateway::run_stream(state.clone(), &k, &chat_req())
            .await
            .unwrap();

        // drain the whole stream
        let mut s = completed.stream;
        let mut chunks = 0;
        while let Some(item) = s.next().await {
            item.unwrap();
            chunks += 1;
        }
        drop(s); // ensure the commit guard drops
        assert_eq!(chunks, 3);
        assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO);
    }

    #[tokio::test]
    async fn aborted_stream_still_commits_partial_usage() {
        use gateway_llm::FinishReason;

        struct AbortProvider {
            usage: TokenUsage,
        }
        #[async_trait]
        impl Provider for AbortProvider {
            fn id(&self) -> &str {
                "abort"
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
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn stream(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<DeltaStream, ProviderError> {
                // usage arrives on the FIRST delta, then more content would follow
                let deltas = vec![
                    Ok(StreamDelta::finish(FinishReason::Stop, self.usage)),
                    Ok(StreamDelta::text("more")),
                ];
                Ok(Box::pin(futures::stream::iter(deltas)))
            }
        }

        let mut ks = StaticKeyStore::new();
        ks.insert(key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        ));
        let mut providers = ProviderRegistry::new();
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(AbortProvider { usage }),
                credentials: Arc::new(Credentials::new("x")),
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
            .set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            None,
            RateLimits::default(),
        );
        let completed = Gateway::run_stream(state.clone(), &k, &chat_req())
            .await
            .unwrap();

        // read ONLY the first delta (which carried usage), then DROP the stream.
        let mut s = completed.stream;
        let first = s.next().await.unwrap();
        first.unwrap();
        drop(s); // abort → CommitGuard drops → commit fires from last_usage

        assert_eq!(
            state.ledger.spent("key_1"),
            Usd::from_micros(7_500),
            "usage not lost on abort"
        );
        assert_eq!(
            state.ledger.reserved("key_1"),
            Usd::ZERO,
            "no stranded reservation"
        );
    }
}
