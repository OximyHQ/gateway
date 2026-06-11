//! The one internal non-streaming response shape. Maps `gateway_spine::TokenUsage`
//! for cost (the spine prices it — this crate never computes dollars). Every
//! egress transport produces this from a provider's JSON body; every ingress
//! dialect serializes this back out (P1.3/P1.4).

use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::ContentPart;
use crate::toolcall::ToolCall;

/// Why generation stopped — normalized across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of turn.
    Stop,
    /// Hit max_tokens / output cap.
    Length,
    /// Model emitted tool call(s) and is waiting on results.
    ToolCalls,
    /// Provider content filter / safety stop.
    ContentFilter,
    /// Stream ended without a provider-reported reason (e.g. aborted upstream).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Provider/registry model id that actually served the request.
    pub model: String,
    /// Assistant content parts (text, possibly empty when only tools were called).
    pub content: Vec<ContentPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    /// Normalized, non-overlapping token usage for cost. The spine prices this.
    pub usage: TokenUsage,
    /// Provider-native request/response id, when available (for audit/debug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
}

impl ChatResponse {
    /// Concatenated text across all text content parts.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_concatenates_content_parts() {
        let r = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("Hello "), ContentPart::text("world")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 2,
                ..Default::default()
            },
            provider_response_id: Some("resp_1".into()),
        };
        assert_eq!(r.text(), "Hello world");
        assert_eq!(r.usage.output_tokens, 2);
    }

    #[test]
    fn finish_reason_serializes_snake_case() {
        let j = serde_json::to_string(&FinishReason::ToolCalls).unwrap();
        assert_eq!(j, "\"tool_calls\"");
    }

    #[test]
    fn response_roundtrips() {
        let r = ChatResponse {
            model: "claude-3-5-sonnet".into(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "f".into(),
                arguments: "{}".into(),
            }],
            finish_reason: FinishReason::ToolCalls,
            usage: TokenUsage::default(),
            provider_response_id: None,
        };
        let back: ChatResponse = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
