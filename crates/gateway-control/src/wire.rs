//! HTTP-boundary wire types for the OpenAI `/v1/chat/completions` dialect, and
//! their conversions to/from the unified `gateway_llm` types. P1.3 owns full
//! cross-dialect translation + the fidelity matrix; P1.4 needs just enough to
//! accept an OpenAI request and emit an OpenAI response with `usage.cost`. The
//! Anthropic/Gemini/Responses ingress dialects reuse the unified types via the
//! P1.3 translators; until those land, `/v1/messages` etc. accept the unified
//! shape (documented in the route table).

use gateway_llm::{
    ChatRequest, ChatResponse, ContentPart, ImageSource, Message, Role, ToolCall, ToolChoice,
    ToolDef,
};
use gateway_spine::Usd;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// OpenAI tool definitions: `{type:"function", function:{name,description,parameters}}`.
    #[serde(default)]
    pub tools: Vec<WireReqTool>,
    /// `"auto"|"none"|"required"` or `{type:"function",function:{name}}`.
    #[serde(default)]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireReqTool {
    pub function: WireReqToolFunction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireReqToolFunction {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireMessage {
    pub role: String,
    /// OpenAI allows a bare string OR an array of typed parts (multimodal). `None`
    /// for an assistant turn that only emitted tool calls.
    #[serde(default)]
    pub content: Option<Value>,
    /// Assistant tool calls echoed back on a follow-up turn.
    #[serde(default)]
    pub tool_calls: Vec<WireReqToolCall>,
    /// For `role:"tool"` — which call this message answers.
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireReqToolCall {
    pub id: String,
    pub function: WireReqCallFunction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireReqCallFunction {
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}

impl WireChatRequest {
    pub fn to_unified(&self) -> ChatRequest {
        let messages = self
            .messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" | "developer" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool" | "function" => Role::Tool,
                    _ => Role::User,
                };
                Message {
                    role,
                    content: parse_content(m.content.as_ref()),
                    tool_calls: m
                        .tool_calls
                        .iter()
                        .map(|t| ToolCall {
                            id: t.id.clone(),
                            name: t.function.name.clone(),
                            arguments: t.function.arguments.clone(),
                        })
                        .collect(),
                    tool_call_id: m.tool_call_id.clone(),
                }
            })
            .collect();
        let mut req = ChatRequest::new(self.model.clone(), messages);
        req.temperature = self.temperature;
        req.max_tokens = self.max_tokens;
        req.stream = self.stream;
        req.tools = self
            .tools
            .iter()
            .map(|t| ToolDef {
                name: t.function.name.clone(),
                description: t.function.description.clone(),
                parameters: t
                    .function
                    .parameters
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({"type": "object"})),
            })
            .collect();
        req.tool_choice = self.tool_choice.as_ref().and_then(parse_tool_choice);
        req
    }
}

/// OpenAI message content is a bare string or an array of typed parts.
fn parse_content(content: Option<&Value>) -> Vec<ContentPart> {
    match content {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![ContentPart::text(s.clone())]
            }
        }
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let t = item.get("text").and_then(Value::as_str).unwrap_or_default();
                        parts.push(ContentPart::text(t));
                    }
                    Some("image_url") => {
                        if let Some(url) = item
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(Value::as_str)
                        {
                            parts.push(ContentPart::Image {
                                source: image_source_from_url(url),
                            });
                        }
                    }
                    _ => {}
                }
            }
            parts
        }
        Some(other) => vec![ContentPart::text(other.to_string())],
    }
}

/// A `data:<mime>;base64,<data>` URL becomes inline base64; anything else is a URL reference.
fn image_source_from_url(url: &str) -> ImageSource {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((meta, data)) = rest.split_once(";base64,")
    {
        return ImageSource::Base64 {
            media_type: meta.to_string(),
            data: data.to_string(),
        };
    }
    ImageSource::Url {
        url: url.to_string(),
    }
}

fn parse_tool_choice(v: &Value) -> Option<ToolChoice> {
    match v {
        Value::String(s) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" | "any" => Some(ToolChoice::Required),
            _ => None,
        },
        Value::Object(_) => v
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .map(|name| ToolChoice::Function {
                name: name.to_string(),
            }),
        _ => None,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireOutToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireOutToolCall {
    pub id: String,
    pub index: i64,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireOutCallFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireOutCallFunction {
    pub name: String,
    pub arguments: String,
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
                    tool_calls: if resp.tool_calls.is_empty() {
                        None
                    } else {
                        Some(
                            resp.tool_calls
                                .iter()
                                .enumerate()
                                .map(|(i, t)| WireOutToolCall {
                                    id: t.id.clone(),
                                    index: i as i64,
                                    kind: "function",
                                    function: WireOutCallFunction {
                                        name: t.name.clone(),
                                        arguments: t.arguments.clone(),
                                    },
                                })
                                .collect(),
                        )
                    },
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
    fn tools_and_multimodal_request_maps_to_unified() {
        let json = r#"{
            "model":"gpt-4o",
            "messages":[{"role":"user","content":[
                {"type":"text","text":"what is this?"},
                {"type":"image_url","image_url":{"url":"https://x/y.png"}}
            ]}],
            "tools":[{"type":"function","function":{"name":"get_weather","description":"w","parameters":{"type":"object"}}}],
            "tool_choice":"required"
        }"#;
        let unified = serde_json::from_str::<WireChatRequest>(json)
            .unwrap()
            .to_unified();
        assert_eq!(unified.tools.len(), 1);
        assert_eq!(unified.tools[0].name, "get_weather");
        assert_eq!(unified.tool_choice, Some(ToolChoice::Required));
        assert_eq!(unified.messages[0].content.len(), 2);
        assert!(matches!(
            unified.messages[0].content[1],
            ContentPart::Image { .. }
        ));
    }

    #[test]
    fn tool_calls_surface_in_response() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![],
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "get_weather".into(),
                arguments: "{\"city\":\"Tokyo\"}".into(),
            }],
            finish_reason: FinishReason::ToolCalls,
            usage: TokenUsage::default(),
            provider_response_id: Some("r1".into()),
        };
        let wire = WireChatResponse::from_unified(&resp, Usd::ZERO);
        let tc = wire.choices[0]
            .message
            .tool_calls
            .as_ref()
            .expect("tool_calls present");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "get_weather");
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(
            v["choices"][0]["message"]["tool_calls"][0]["type"],
            "function"
        );
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
    }

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
