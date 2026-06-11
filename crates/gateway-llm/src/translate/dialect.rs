//! Uniform dialect dispatch. P1.4 maps each `/v1/...` route to a `Dialect` and
//! calls `parse_request`/`serialize_response` without knowing which dialect module
//! backs it. This is the single switchboard between the HTTP surface and the
//! per-dialect parsers/serializers.

use serde_json::Value;

use crate::req::ChatRequest;
use crate::resp::ChatResponse;
use crate::translate::ingress::{anthropic_messages, openai_chat, openai_responses};
use crate::translate::structured::ProviderFamily;
use crate::translate::warn::{IngressError, Translated};

/// A client-facing wire dialect served by the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `/v1/chat/completions`
    OpenAiChat,
    /// `/v1/messages`
    AnthropicMessages,
    /// `/v1/responses`
    OpenAiResponses,
}

impl Dialect {
    pub fn parse_request(&self, body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
        match self {
            Dialect::OpenAiChat => openai_chat::parse_request(body),
            Dialect::AnthropicMessages => anthropic_messages::parse_request(body),
            Dialect::OpenAiResponses => openai_responses::parse_request(body),
        }
    }

    pub fn serialize_response(&self, resp: &ChatResponse) -> Value {
        match self {
            Dialect::OpenAiChat => openai_chat::serialize_response(resp),
            Dialect::AnthropicMessages => anthropic_messages::serialize_response(resp),
            Dialect::OpenAiResponses => openai_responses::serialize_response(resp),
        }
    }

    /// The provider family a downstream egress maps to (used to compile a
    /// structured-output plan). The dialect a CLIENT speaks is independent of the
    /// PROVIDER actually serving it — this returns the family for the request's
    /// resolved provider, defaulting by dialect when the router has not chosen yet.
    pub fn default_family(&self) -> ProviderFamily {
        match self {
            Dialect::OpenAiChat | Dialect::OpenAiResponses => ProviderFamily::OpenAi,
            Dialect::AnthropicMessages => ProviderFamily::Anthropic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dispatches_openai_chat_parse() {
        let body = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "Hi"}]});
        let t = Dialect::OpenAiChat.parse_request(&body).unwrap();
        assert_eq!(t.value.model, "gpt-4o");
    }

    #[test]
    fn dispatches_anthropic_parse() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = Dialect::AnthropicMessages.parse_request(&body).unwrap();
        assert_eq!(t.value.max_tokens, Some(16));
    }

    #[test]
    fn dispatches_responses_parse() {
        let body = json!({"model": "gpt-4o", "input": "Hi"});
        let t = Dialect::OpenAiResponses.parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].text_content(), "Hi");
    }

    #[test]
    fn families_match_dialect() {
        assert_eq!(Dialect::OpenAiChat.default_family(), ProviderFamily::OpenAi);
        assert_eq!(
            Dialect::AnthropicMessages.default_family(),
            ProviderFamily::Anthropic
        );
    }
}
