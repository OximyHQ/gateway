//! OpenAI Chat Completions INGRESS dialect (client → unified). The inverse of the
//! OpenAI egress transport's wire-request mapping: clients (OpenAI SDK, Codex)
//! POST a `/v1/chat/completions` body; we parse it into the unified `ChatRequest`.
//! Multimodal content arrays, tool defs/choice, and `response_format` map across;
//! params with no unified home are DROPPED WITH A WARNING (never silently), and a
//! semantic-changing unsupported feature is rejected with `Unsupported`.

use serde::Deserialize;
use serde_json::Value;

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::req::{ChatRequest, ReasoningEffort, ResponseFormat};
use crate::resp::{ChatResponse, FinishReason};
use crate::stream::StreamDelta;
use crate::toolcall::{ToolChoice, ToolDef};
use crate::translate::warn::{IngressError, Translated, Warning};

/// Params we knowingly drop (no unified equivalent yet) — warn, never fail.
const DROPPED_PARAMS: &[(&str, &str)] = &[
    ("logit_bias", "no unified equivalent"),
    ("frequency_penalty", "not modeled in the unified request"),
    ("presence_penalty", "not modeled in the unified request"),
    ("seed", "determinism is not portable across providers"),
    ("n", "only a single choice is supported"),
];

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(default)]
    tools: Vec<WireTool>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    max_tokens: Option<i64>,
    #[serde(default)]
    max_completion_tokens: Option<i64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    response_format: Option<Value>,
    #[serde(default)]
    user: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    tool_call_id: Option<String>,
}

#[derive(Deserialize)]
struct WireTool {
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Value>,
}

fn parse_role(role: &str) -> Result<Role, IngressError> {
    match role {
        "system" | "developer" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" | "function" => Ok(Role::Tool),
        other => Err(IngressError::Malformed(format!("unknown role `{other}`"))),
    }
}

/// Content may be a bare string or an array of typed parts.
fn parse_content(content: Option<Value>) -> Result<Vec<ContentPart>, IngressError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![ContentPart::text(s)]),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        let t = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        parts.push(ContentPart::text(t));
                    }
                    "image_url" => {
                        let url = item
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                IngressError::Malformed("image_url missing url".into())
                            })?;
                        parts.push(ContentPart::Image {
                            source: ImageSource::Url {
                                url: url.to_string(),
                            },
                        });
                    }
                    "input_audio" => {
                        return Err(IngressError::Unsupported {
                            feature: "audio input".into(),
                        });
                    }
                    other => {
                        return Err(IngressError::Malformed(format!(
                            "unknown content part type `{other}`"
                        )));
                    }
                }
            }
            Ok(parts)
        }
        Some(other) => Err(IngressError::Malformed(format!("invalid content: {other}"))),
    }
}

fn parse_tool_choice(v: Value, warnings: &mut Vec<Warning>) -> Option<ToolChoice> {
    match v {
        Value::String(s) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" => Some(ToolChoice::Required),
            other => {
                warnings.push(Warning::dropped_param(
                    "tool_choice",
                    &format!("unknown value `{other}`, defaulting to auto"),
                ));
                Some(ToolChoice::Auto)
            }
        },
        Value::Object(map) => map
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .map(|name| ToolChoice::Function {
                name: name.to_string(),
            }),
        _ => None,
    }
}

fn parse_response_format(v: Value) -> Result<ResponseFormat, IngressError> {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("text");
    match ty {
        "text" => Ok(ResponseFormat::Text),
        "json_object" => Ok(ResponseFormat::JsonObject),
        "json_schema" => {
            let js = v
                .get("json_schema")
                .ok_or_else(|| IngressError::Malformed("json_schema field missing".into()))?;
            Ok(ResponseFormat::JsonSchema {
                name: js
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("schema")
                    .to_string(),
                schema: js.get("schema").cloned().unwrap_or(Value::Null),
                strict: js.get("strict").and_then(|s| s.as_bool()).unwrap_or(false),
            })
        }
        other => Err(IngressError::Unsupported {
            feature: format!("response_format type `{other}`"),
        }),
    }
}

fn parse_reasoning(effort: &str) -> Option<ReasoningEffort> {
    match effort {
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        _ => None,
    }
}

/// Parse a raw OpenAI Chat Completions body into a unified request + warnings.
pub fn parse_request(body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
    let wire: WireRequest =
        serde_json::from_value(body.clone()).map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings = Vec::new();

    for (name, reason) in DROPPED_PARAMS {
        if body.get(name).is_some() {
            warnings.push(Warning::dropped_param(name, reason));
        }
    }

    let mut messages = Vec::with_capacity(wire.messages.len());
    for m in wire.messages {
        let role = parse_role(&m.role)?;
        messages.push(Message {
            role,
            content: parse_content(m.content)?,
            tool_calls: Vec::new(),
            tool_call_id: m.tool_call_id,
        });
    }

    let tools = wire
        .tools
        .into_iter()
        .map(|t| ToolDef {
            name: t.function.name,
            description: t.function.description,
            parameters: t.function.parameters.unwrap_or(Value::Null),
        })
        .collect();

    let tool_choice = wire
        .tool_choice
        .and_then(|v| parse_tool_choice(v, &mut warnings));

    if wire.max_tokens.is_some() && wire.max_completion_tokens.is_some() {
        warnings.push(Warning::dropped_param(
            "max_tokens",
            "both max_tokens and max_completion_tokens set; using max_completion_tokens",
        ));
    }
    let max_tokens = wire.max_completion_tokens.or(wire.max_tokens);

    let response_format = match wire.response_format {
        Some(v) => Some(parse_response_format(v)?),
        None => None,
    };

    let reasoning_effort = wire.reasoning_effort.as_deref().and_then(parse_reasoning);

    Ok(Translated {
        value: ChatRequest {
            model: wire.model,
            messages,
            tools,
            tool_choice,
            temperature: wire.temperature,
            max_tokens,
            stream: wire.stream,
            reasoning_effort,
            response_format,
            user: wire.user,
        },
        warnings,
    })
}

fn finish_str(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Unknown => "stop",
    }
}

/// Serialize a unified response into an OpenAI Chat Completions JSON body.
pub fn serialize_response(resp: &ChatResponse) -> Value {
    let tool_calls: Vec<Value> = resp
        .tool_calls
        .iter()
        .map(|c| json_tool_call(&c.id, &c.name, &c.arguments))
        .collect();
    let mut message = serde_json::Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert("content".into(), Value::String(resp.text()));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    serde_json::json!({
        "id": resp.provider_response_id.clone().unwrap_or_else(|| "chatcmpl-oximy".into()),
        "object": "chat.completion",
        "model": resp.model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_str(resp.finish_reason),
        }],
        "usage": {
            "prompt_tokens": resp.usage.input_tokens + resp.usage.cache_read_tokens,
            "completion_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.total(),
        },
    })
}

fn json_tool_call(id: &str, name: &str, arguments: &str) -> Value {
    serde_json::json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments },
    })
}

/// Serialize one unified delta into an OpenAI streaming chunk JSON (the body of
/// one `data:` SSE frame). Returns `None` for an empty delta (caller skips it).
pub fn serialize_delta(model: &str, delta: &StreamDelta) -> Option<Value> {
    if delta.is_empty() {
        return None;
    }
    let mut inner = serde_json::Map::new();
    if let Some(c) = &delta.content_delta {
        inner.insert("content".into(), Value::String(c.clone()));
    }
    if !delta.tool_call_deltas.is_empty() {
        let calls: Vec<Value> = delta
            .tool_call_deltas
            .iter()
            .map(|t| {
                let mut f = serde_json::Map::new();
                if let Some(n) = &t.name {
                    f.insert("name".into(), Value::String(n.clone()));
                }
                if let Some(a) = &t.arguments_delta {
                    f.insert("arguments".into(), Value::String(a.clone()));
                }
                serde_json::json!({
                    "index": t.index,
                    "id": t.id,
                    "function": Value::Object(f),
                })
            })
            .collect();
        inner.insert("tool_calls".into(), Value::Array(calls));
    }
    let mut chunk = serde_json::json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": Value::Object(inner),
            "finish_reason": delta.finish_reason.map(finish_str),
        }],
    });
    if let Some(u) = &delta.usage {
        chunk["usage"] = serde_json::json!({
            "prompt_tokens": u.input_tokens + u.cache_read_tokens,
            "completion_tokens": u.output_tokens,
            "total_tokens": u.total(),
        });
    }
    Some(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::TokenUsage;
    use serde_json::json;

    #[test]
    fn parses_minimal_request() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.model, "gpt-4o");
        assert_eq!(t.value.messages.len(), 1);
        assert_eq!(t.value.messages[0].text_content(), "Hi");
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn dropped_params_warn_not_fail() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
            "logit_bias": {"50256": -100},
            "seed": 42,
        });
        let t = parse_request(&body).unwrap();
        let codes: Vec<_> = t.warnings.iter().map(|w| w.message.as_str()).collect();
        assert!(codes.iter().any(|m| m.contains("logit_bias")));
        assert!(codes.iter().any(|m| m.contains("seed")));
    }

    #[test]
    fn multimodal_content_array_parses() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "look:"},
                    {"type": "image_url", "image_url": {"url": "https://x/y.png"}},
                ],
            }],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].content.len(), 2);
    }

    #[test]
    fn audio_input_is_unsupported_not_dropped() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [{"type": "input_audio", "input_audio": {"data": "...", "format": "wav"}}],
            }],
        });
        let err = parse_request(&body).unwrap_err();
        assert!(matches!(err, IngressError::Unsupported { .. }));
    }

    #[test]
    fn tools_and_tool_choice_function_parse() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.tools.len(), 1);
        assert_eq!(t.value.tools[0].name, "get_weather");
        assert_eq!(
            t.value.tool_choice,
            Some(ToolChoice::Function {
                name: "get_weather".into()
            })
        );
    }

    #[test]
    fn json_schema_response_format_parses() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "out", "schema": {"type": "object"}, "strict": true},
            },
        });
        let t = parse_request(&body).unwrap();
        assert!(matches!(
            t.value.response_format,
            Some(ResponseFormat::JsonSchema { strict: true, .. })
        ));
    }

    #[test]
    fn serialize_response_shapes_openai_body() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("Hello")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 8,
                output_tokens: 2,
                cache_read_tokens: 2,
                ..Default::default()
            },
            provider_response_id: Some("chatcmpl-1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["object"], "chat.completion");
        assert_eq!(j["choices"][0]["message"]["content"], "Hello");
        assert_eq!(j["choices"][0]["finish_reason"], "stop");
        // prompt_tokens recombines input + cache_read (the OpenAI total view).
        assert_eq!(j["usage"]["prompt_tokens"], 10);
        assert_eq!(j["usage"]["completion_tokens"], 2);
    }

    #[test]
    fn serialize_delta_emits_chunk_and_skips_empty() {
        assert!(serialize_delta("gpt-4o", &StreamDelta::default()).is_none());
        let c = serialize_delta("gpt-4o", &StreamDelta::text("ab")).unwrap();
        assert_eq!(c["object"], "chat.completion.chunk");
        assert_eq!(c["choices"][0]["delta"]["content"], "ab");
    }

    #[test]
    fn serialize_delta_carries_usage_on_final_chunk() {
        let d = StreamDelta::finish(
            FinishReason::Stop,
            TokenUsage {
                input_tokens: 5,
                output_tokens: 3,
                ..Default::default()
            },
        );
        let c = serialize_delta("gpt-4o", &d).unwrap();
        assert_eq!(c["choices"][0]["finish_reason"], "stop");
        assert_eq!(c["usage"]["completion_tokens"], 3);
    }
}
