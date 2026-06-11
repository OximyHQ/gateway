//! The `TargetExecutor` seam: the router calls *this* rather than a concrete
//! `Provider` directly, which keeps `gateway-route` independent of
//! `gateway-control` (no circular crate dep). The control plane wires up a
//! concrete impl that resolves credentials from the `ProviderRegistry`.

use async_trait::async_trait;

use gateway_llm::{ChatRequest, ChatResponse, ProviderError};

use crate::route::RouteTarget;

/// The interface the router uses to execute a call against one target.
/// Implementors look up provider + credentials and call the underlying
/// `gateway_llm::Provider::chat`.
#[async_trait]
pub trait TargetExecutor: Send + Sync {
    async fn execute(
        &self,
        target: &RouteTarget,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ProviderError>;
}
