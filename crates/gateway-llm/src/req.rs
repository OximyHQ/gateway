//! The one internal request shape. Every ingress dialect (OpenAI chat,
//! Anthropic messages, Gemini generateContent — P1.3) maps INTO this; every
//! egress transport maps OUT of it. Optional knobs are `Option`/defaulted so a
//! transport can detect "unset" vs "explicitly chosen" and emit
//! `ProviderError::Unsupported` rather than silently dropping (no-silent-
//! degradation invariant). `reasoning_effort` and `response_format` are CARRIED
//! here but their provider-fidelity mapping is owned by P1.3.

use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::toolcall::{ToolChoice, ToolDef};

/// Provider-agnostic reasoning knob (Envoy-style); transports map to their own
/// thinking-budget. Mapping fidelity is P1.3's concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

/// Requested output shape. `JsonSchema` translation (forced-tool emulation,
/// per-provider equivalents) is DEFERRED to P1.3 — P1.2 only carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        #[serde(default)]
        strict: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Registry model id (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    /// Caller wants a streamed response.
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Optional per-end-user attribution tag (carried to providers that accept a
    /// `user` field; otherwise dropped without error — it is non-semantic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl ChatRequest {
    /// Minimal builder for tests/simple call sites: model + a single user turn.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        ChatRequest {
            model: model.into(),
            messages,
            tools: Vec::new(),
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            stream: false,
            reasoning_effort: None,
            response_format: None,
            user: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Role;

    #[test]
    fn new_sets_defaults() {
        let r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        assert_eq!(r.model, "gpt-4o");
        assert!(!r.stream);
        assert!(r.tools.is_empty());
        assert!(r.temperature.is_none());
        assert!(r.response_format.is_none());
    }

    #[test]
    fn empty_optionals_are_omitted_from_json() {
        let r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        let j = serde_json::to_value(&r).unwrap();
        assert!(j.get("tools").is_none());
        assert!(j.get("temperature").is_none());
        assert!(j.get("response_format").is_none());
        assert_eq!(j["stream"], false);
    }

    #[test]
    fn reasoning_effort_serializes_snake_case() {
        let mut r = ChatRequest::new("o3", vec![Message::text(Role::User, "hi")]);
        r.reasoning_effort = Some(ReasoningEffort::High);
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["reasoning_effort"], "high");
    }

    #[test]
    fn response_format_json_schema_roundtrips() {
        let mut r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        r.response_format = Some(ResponseFormat::JsonSchema {
            name: "out".into(),
            schema: serde_json::json!({"type": "object"}),
            strict: true,
        });
        let back: ChatRequest = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
