//! Maps a provider id (the `provider` field on a `ModelEntry`) to a concrete
//! egress `Provider` (P1.2) plus its `Credentials`. The lifecycle selects the
//! transport for a model by looking up its provider here. P1.5 grows this into
//! the multi-deployment / fallback array; P1.4 ships exactly one deployment per
//! provider id.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gateway_llm::{Credentials, Provider};

/// One configured egress deployment: a transport + the credentials to call it.
#[derive(Clone)]
pub struct Deployment {
    pub provider: Arc<dyn Provider>,
    pub credentials: Arc<Credentials>,
}

/// Maps provider ids to their live egress deployment. Uses interior mutability
/// (`RwLock`) so the registry can be mutated at runtime (e.g. the admin
/// `POST /v1/admin/providers` route) while every reader keeps an `&self`
/// signature and the hot path stays lock-friendly (a single short read lock per
/// lookup). `get` returns an owned `Deployment` (two cheap `Arc` clones) because
/// the value cannot outlive the read guard.
#[derive(Default)]
pub struct ProviderRegistry {
    by_id: RwLock<HashMap<String, Deployment>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a deployment for `provider_id`. Takes `&self` via
    /// interior mutability so it can be called at runtime on a shared registry.
    pub fn insert(&self, provider_id: impl Into<String>, deployment: Deployment) {
        self.by_id
            .write()
            .unwrap()
            .insert(provider_id.into(), deployment);
    }

    /// Look up the deployment for `provider_id`, returning an owned clone (cheap:
    /// two `Arc`s) so the result is independent of the read lock.
    pub fn get(&self, provider_id: &str) -> Option<Deployment> {
        self.by_id.read().unwrap().get(provider_id).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.read().unwrap().is_empty()
    }

    pub fn count(&self) -> usize {
        self.by_id.read().unwrap().len()
    }

    /// Sorted list of all registered provider ids.
    pub fn all_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_id.read().unwrap().keys().cloned().collect();
        v.sort();
        v
    }
}

/// A provider added at runtime via the admin API, in the shape the binary
/// persists to its JSON state file. The credentials build an OpenAI-compatible
/// deployment (`base_url` + `api_key`) on the next boot.
#[derive(Clone, Debug)]
pub struct RuntimeProvider {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
}

/// Persistence seam for runtime-added providers. The binary implements this to
/// write the provider into its state file so it survives a restart; tests and
/// library-only embeddings can leave it `None` (runtime registration still
/// works, it just won't be durable). Kept on `AppState` behind an `Option` so
/// the control crate has no hard dependency on the file format.
pub trait ProviderPersist: Send + Sync {
    /// Persist a newly-registered runtime provider. Best-effort: an `Err` is
    /// logged by the caller but does not fail the request (the provider is
    /// already live in memory).
    fn persist(&self, provider: &RuntimeProvider) -> anyhow::Result<()>;
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
        let r = ProviderRegistry::new();
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
