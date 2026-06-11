//! Maps a provider id (the `provider` field on a `ModelEntry`) to a concrete
//! egress `Provider` (P1.2) plus its `Credentials`. The lifecycle selects the
//! transport for a model by looking up its provider here. P1.5 grows this into
//! the multi-deployment / fallback array; P1.4 ships exactly one deployment per
//! provider id.

use std::collections::HashMap;
use std::sync::Arc;

use gateway_llm::{Credentials, Provider};

/// One configured egress deployment: a transport + the credentials to call it.
#[derive(Clone)]
pub struct Deployment {
    pub provider: Arc<dyn Provider>,
    pub credentials: Arc<Credentials>,
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    by_id: HashMap<String, Deployment>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, provider_id: impl Into<String>, deployment: Deployment) {
        self.by_id.insert(provider_id.into(), deployment);
    }

    pub fn get(&self, provider_id: &str) -> Option<&Deployment> {
        self.by_id.get(provider_id)
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gateway_llm::{
        ChatRequest, ChatResponse, DeltaStream, ProviderCapabilities, ProviderError,
    };

    struct Dummy;

    #[async_trait]
    impl Provider for Dummy {
        fn id(&self) -> &str {
            "dummy"
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
            unreachable!()
        }
    }

    #[test]
    fn lookup_by_provider_id() {
        let mut r = ProviderRegistry::new();
        r.insert(
            "openai",
            Deployment {
                provider: Arc::new(Dummy),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        assert!(r.get("openai").is_some());
        assert_eq!(r.get("openai").unwrap().provider.id(), "dummy");
        assert!(r.get("anthropic").is_none());
    }
}
