//! # gateway-llm
//!
//! The provider-agnostic LLM core: the unified request/response/streaming types,
//! the `Provider` egress trait, and reference transports (OpenAI, Anthropic,
//! Gemini). Ingress dialects map INTO the unified types; transports map OUT to
//! provider wire formats. Usage is normalized into `gateway_spine::TokenUsage`
//! (non-overlapping buckets) so the spine prices it exactly.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod message;
pub mod provider;
pub mod req;
pub mod resp;
pub mod sse;
pub mod stream;
pub mod toolcall;
pub mod translate;
pub mod transports;

pub use message::{ContentPart, ImageSource, Message, Role};
pub use provider::{Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError};
pub use req::{ChatRequest, ReasoningEffort, ResponseFormat};
pub use resp::{ChatResponse, FinishReason};
pub use sse::{SseDecoder, SseEvent};
pub use stream::{StreamDelta, ToolCallDelta};
pub use toolcall::{ToolCall, ToolChoice, ToolDef};
pub use translate::{
    Dialect, IngressError, ProviderFamily, StructuredOutputPlan, ToolCallAggregator, Translated,
    Warning,
};
