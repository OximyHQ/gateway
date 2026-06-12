//! The control-plane [`TargetExecutor`]: bridges `gateway-route`'s
//! provider-agnostic routing to the concrete `gateway-llm` providers held in the
//! [`ProviderRegistry`].
//!
//! `gateway-route` calls `TargetExecutor::execute(target, request)` for each
//! attempt; this impl resolves `target.provider_id` to a [`Deployment`] and
//! calls its `Provider::chat`. The **same** idempotency key is reused across all
//! retries/failovers of one logical request (held as a field, minted once by the
//! lifecycle) so the spine bills the call once — matching the route-crate
//! invariant.

use async_trait::async_trait;

use gateway_llm::{ChatRequest, ChatResponse, ProviderError};
use gateway_route::{RouteTarget, TargetExecutor};

use crate::providers::ProviderRegistry;

/// A `TargetExecutor` backed by the live [`ProviderRegistry`].
///
/// One instance per logical request: it owns the single idempotency key that
/// every attempt reuses. `target.model` (not the request's model) is sent
/// upstream so a failover target can address a different model id on its own
/// provider.
pub struct RegistryExecutor<'a> {
    registry: &'a ProviderRegistry,
    idempotency_key: String,
}

impl<'a> RegistryExecutor<'a> {
    pub fn new(registry: &'a ProviderRegistry, idempotency_key: impl Into<String>) -> Self {
        Self {
            registry,
            idempotency_key: idempotency_key.into(),
        }
    }
}

#[async_trait]
impl TargetExecutor for RegistryExecutor<'_> {
    async fn execute(
        &self,
        target: &RouteTarget,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        let deployment = self.registry.get(&target.provider_id).ok_or_else(|| {
            // A misconfigured route (target points at an unregistered provider)
            // is the client's/operator's fault — surface as a non-retryable 400
            // so the router does NOT failover-spin on it.
            ProviderError::Upstream {
                status: 400,
                body: format!("no egress configured for provider {}", target.provider_id),
            }
        })?;

        // Address the target's own model id on its provider (may differ from the
        // request's registry model for a cross-provider failover target).
        let mut req = request.clone();
        req.model = target.model.clone();

        deployment
            .provider
            .chat(&req, &deployment.credentials, &self.idempotency_key)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Deployment;
    use async_trait::async_trait;
    use gateway_llm::{
        ContentPart, Credentials, DeltaStream, FinishReason, Message, Provider,
        ProviderCapabilities, Role,
    };
    use gateway_spine::TokenUsage;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RecordingProvider {
        idem_seen: std::sync::Mutex<Vec<String>>,
        model_seen: std::sync::Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    impl RecordingProvider {
        fn new() -> Self {
            Self {
                idem_seen: std::sync::Mutex::new(vec![]),
                model_seen: std::sync::Mutex::new(vec![]),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl Provider for RecordingProvider {
        fn id(&self) -> &str {
            "rec"
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
            idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.idem_seen
                .lock()
                .unwrap()
                .push(idempotency_key.to_string());
            self.model_seen.lock().unwrap().push(req.model.clone());
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![ContentPart::text("ok")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage::default(),
                provider_response_id: None,
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

    fn req() -> ChatRequest {
        ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")])
    }

    #[tokio::test]
    async fn dispatches_to_registry_provider_with_target_model() {
        let provider = Arc::new(RecordingProvider::new());
        let reg = ProviderRegistry::new();
        reg.insert(
            "openai",
            Deployment {
                provider: provider.clone(),
                credentials: Arc::new(Credentials::new("up")),
            },
        );
        let exec = RegistryExecutor::new(&reg, "idem-123");
        let target = RouteTarget::new("openai", "gpt-4o-mini");
        let resp = exec.execute(&target, &req()).await.unwrap();
        assert_eq!(resp.model, "gpt-4o-mini");
        assert_eq!(provider.idem_seen.lock().unwrap()[0], "idem-123");
        assert_eq!(provider.model_seen.lock().unwrap()[0], "gpt-4o-mini");
    }

    #[tokio::test]
    async fn unknown_provider_is_non_retryable_400() {
        let reg = ProviderRegistry::new();
        let exec = RegistryExecutor::new(&reg, "idem");
        let target = RouteTarget::new("ghost", "x");
        let err = exec.execute(&target, &req()).await.unwrap_err();
        assert!(matches!(err, ProviderError::Upstream { status: 400, .. }));
    }
}
