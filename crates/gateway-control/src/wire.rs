//! HTTP-boundary wire types for the OpenAI `/v1/chat/completions` dialect, and
//! their conversions to/from the unified `gateway_llm` types. P1.3 owns full
//! cross-dialect translation + the fidelity matrix; P1.4 needs just enough to
//! accept an OpenAI request and emit an OpenAI response with `usage.cost`. The
//! Anthropic/Gemini/Responses ingress dialects reuse the unified types via the
//! P1.3 translators; until those land, `/v1/messages` etc. accept the unified
//! shape (documented in the route table).

use gateway_llm::{ChatRequest, ChatResponse, ContentPart, Message, Role};
use gateway_spine::Usd;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct WireChatRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireMessage {
    pub role: String,
    /// OpenAI allows string or array content; P1.4 accepts the string form.
    pub content: String,
}

impl WireChatRequest {
    pub fn to_unified(&self) -> ChatRequest {
        let messages = self
            .messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User,
                };
                Message::text(role, m.content.clone())
            })
            .collect();
        let mut req = ChatRequest::new(self.model.clone(), messages);
        req.temperature = self.temperature;
        req.max_tokens = self.max_tokens;
        req.stream = self.stream;
        req
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WireChatResponse {
    pub id: String,
    pub object: &'static str,
    pub model: String,
    pub choices: Vec<WireChoice>,
    pub usage: WireUsage,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireChoice {
    pub index: i64,
    pub message: WireOutMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireOutMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    /// Oximy extension (design §5): authoritative call-time USD.
    pub cost: f64,
}

impl WireChatResponse {
    /// Build an OpenAI-shaped response from the unified response + committed cost.
    pub fn from_unified(resp: &ChatResponse, cost: Usd) -> Self {
        let content = resp
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let finish_reason = match resp.finish_reason {
            gateway_llm::FinishReason::Stop => "stop",
            gateway_llm::FinishReason::Length => "length",
            gateway_llm::FinishReason::ToolCalls => "tool_calls",
            gateway_llm::FinishReason::ContentFilter => "content_filter",
            gateway_llm::FinishReason::Unknown => "stop",
        };
        WireChatResponse {
            id: resp
                .provider_response_id
                .clone()
                .unwrap_or_else(|| "chatcmpl-oximy".into()),
            object: "chat.completion",
            model: resp.model.clone(),
            choices: vec![WireChoice {
                index: 0,
                message: WireOutMessage {
                    role: "assistant",
                    content,
                },
                finish_reason: finish_reason.into(),
            }],
            usage: WireUsage {
                prompt_tokens: resp.usage.input_tokens + resp.usage.cache_read_tokens,
                completion_tokens: resp.usage.output_tokens,
                total_tokens: resp.usage.total(),
                cost: cost.as_dollars_f64(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::{ChatResponse, FinishReason};
    use gateway_spine::TokenUsage;

    #[test]
    fn request_deserializes_and_maps_to_unified() {
        let json = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"max_tokens":256,"stream":true}"#;
        let wire: WireChatRequest = serde_json::from_str(json).unwrap();
        let unified = wire.to_unified();
        assert_eq!(unified.model, "gpt-4o");
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.max_tokens, Some(256));
        assert!(unified.stream);
        assert_eq!(unified.messages[0].text_content(), "hi");
    }

    #[test]
    fn response_includes_cost_and_openai_shape() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("hello")],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            },
            provider_response_id: Some("resp_1".into()),
        };
        let wire = WireChatResponse::from_unified(&resp, Usd::from_micros(7_500));
        assert_eq!(wire.object, "chat.completion");
        assert_eq!(wire.choices[0].message.content, "hello");
        assert_eq!(wire.choices[0].finish_reason, "stop");
        assert_eq!(wire.usage.prompt_tokens, 1000);
        assert_eq!(wire.usage.completion_tokens, 500);
        assert_eq!(wire.usage.total_tokens, 1500);
        assert!((wire.usage.cost - 0.0075).abs() < 1e-9);

        // round-trips through serde to the OpenAI JSON shape
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["usage"]["cost"], 0.0075);
    }
}
