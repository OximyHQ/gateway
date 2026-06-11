//! Anthropic Messages INGRESS dialect (client → unified). Claude Code POSTs
//! `/v1/messages` bodies; we parse them into the unified `ChatRequest`. The
//! top-level `system` string becomes a leading `Role::System` message; content
//! blocks (`text`/`image`/`tool_use`/`tool_result`) map to parts/tool fields. We
//! serialize the unified response back into the `/v1/messages` body. `max_tokens`
//! is required by the dialect — its absence is a `MissingField`.

use serde::Deserialize;
use serde_json::Value;

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};
use crate::toolcall::{ToolChoice, ToolDef};
use crate::translate::warn::{IngressError, Translated, Warning};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    #[serde(default)]
    system: Option<Value>,
    messages: Vec<WireMessage>,
    #[serde(default)]
    max_tokens: Option<i64>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Vec<WireTool>,
    #[serde(default)]
    tool_choice: Option<Value>,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: Value,
}

#[derive(Deserialize)]
struct WireTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_schema: Option<Value>,
}

fn system_to_text(system: &Value) -> String {
    match system {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Anthropic content is a string or an array of typed blocks.
fn parse_content(content: Value) -> Result<(Vec<ContentPart>, Option<String>), IngressError> {
    match content {
        Value::String(s) => Ok((vec![ContentPart::text(s)], None)),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            let mut tool_call_id = None;
            for b in blocks {
                let ty = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                            parts.push(ContentPart::text(t));
                        }
                    }
                    "image" => {
                        let src = b.get("source").ok_or_else(|| {
                            IngressError::Malformed("image block missing source".into())
                        })?;
                        let media_type = src
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png")
                            .to_string();
                        let data = src
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        parts.push(ContentPart::Image {
                            source: ImageSource::Base64 { media_type, data },
                        });
                    }
                    "tool_result" => {
                        tool_call_id = b
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        if let Some(c) = b.get("content").and_then(|v| v.as_str()) {
                            parts.push(ContentPart::text(c));
                        }
                    }
                    "tool_use" => {
                        // Assistant tool calls are echoed in history; carried as text
                        // here (full assistant-tool replay is P1.4's history concern).
                    }
                    other => {
                        return Err(IngressError::Malformed(format!(
                            "unknown content block `{other}`"
                        )));
                    }
                }
            }
            Ok((parts, tool_call_id))
        }
        other => Err(IngressError::Malformed(format!("invalid content: {other}"))),
    }
}

fn parse_role(role: &str) -> Result<Role, IngressError> {
    match role {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        other => Err(IngressError::Malformed(format!("unknown role `{other}`"))),
    }
}

fn parse_tool_choice(v: &Value, warnings: &mut Vec<Warning>) -> Option<ToolChoice> {
    let ty = v.get("type").and_then(|t| t.as_str())?;
    match ty {
        "auto" => Some(ToolChoice::Auto),
        "any" => Some(ToolChoice::Required),
        "none" => Some(ToolChoice::None),
        "tool" => v
            .get("name")
            .and_then(|n| n.as_str())
            .map(|name| ToolChoice::Function {
                name: name.to_string(),
            }),
        other => {
            warnings.push(Warning::dropped_param(
                "tool_choice",
                &format!("unknown anthropic tool_choice `{other}`"),
            ));
            None
        }
    }
}

/// Parse a raw Anthropic Messages body into a unified request + warnings.
pub fn parse_request(body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
    let wire: WireRequest =
        serde_json::from_value(body.clone()).map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings = Vec::new();

    let max_tokens = wire
        .max_tokens
        .ok_or_else(|| IngressError::MissingField("max_tokens".into()))?;

    let mut messages = Vec::new();
    if let Some(sys) = &wire.system {
        let text = system_to_text(sys);
        if !text.is_empty() {
            messages.push(Message::text(Role::System, text));
        }
    }
    for m in wire.messages {
        let role = parse_role(&m.role)?;
        let (content, tool_call_id) = parse_content(m.content)?;
        let role = if tool_call_id.is_some() {
            Role::Tool
        } else {
            role
        };
        messages.push(Message {
            role,
            content,
            tool_calls: Vec::new(),
            tool_call_id,
        });
    }

    let tools = wire
        .tools
        .into_iter()
        .map(|t| ToolDef {
            name: t.name,
            description: t.description,
            parameters: t.input_schema.unwrap_or(Value::Null),
        })
        .collect();

    let tool_choice = wire
        .tool_choice
        .as_ref()
        .and_then(|v| parse_tool_choice(v, &mut warnings));

    Ok(Translated {
        value: ChatRequest {
            model: wire.model,
            messages,
            tools,
            tool_choice,
            temperature: wire.temperature,
            max_tokens: Some(max_tokens),
            stream: wire.stream,
            reasoning_effort: None,
            response_format: None,
            user: None,
        },
        warnings,
    })
}

fn stop_reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "end_turn",
        FinishReason::Length => "max_tokens",
        FinishReason::ToolCalls => "tool_use",
        FinishReason::ContentFilter => "end_turn",
        FinishReason::Unknown => "end_turn",
    }
}

/// Serialize a unified response into an Anthropic `/v1/messages` body.
pub fn serialize_response(resp: &ChatResponse) -> Value {
    let mut blocks: Vec<Value> = resp
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(serde_json::json!({"type": "text", "text": text})),
            ContentPart::Image { .. } => None,
        })
        .collect();
    for c in &resp.tool_calls {
        let input: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
        blocks.push(serde_json::json!({
            "type": "tool_use", "id": c.id, "name": c.name, "input": input,
        }));
    }
    serde_json::json!({
        "id": resp.provider_response_id.clone().unwrap_or_else(|| "msg_oximy".into()),
        "type": "message",
        "role": "assistant",
        "model": resp.model,
        "content": blocks,
        "stop_reason": stop_reason(resp.finish_reason),
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens,
            "cache_read_input_tokens": resp.usage.cache_read_tokens,
            "cache_creation_input_tokens": resp.usage.cache_write_tokens,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolcall::ToolCall;
    use gateway_spine::TokenUsage;
    use serde_json::json;

    #[test]
    fn system_is_hoisted_into_leading_message() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "system": "Be terse.",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].role, Role::System);
        assert_eq!(t.value.messages[0].text_content(), "Be terse.");
        assert_eq!(t.value.messages[1].role, Role::User);
        assert_eq!(t.value.max_tokens, Some(1024));
    }

    #[test]
    fn missing_max_tokens_is_a_missing_field_error() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        assert!(matches!(
            parse_request(&body).unwrap_err(),
            IngressError::MissingField(_)
        ));
    }

    #[test]
    fn tool_result_block_becomes_tool_role() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "tu_1", "content": "42"}],
            }],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].role, Role::Tool);
        assert_eq!(t.value.messages[0].tool_call_id.as_deref(), Some("tu_1"));
    }

    #[test]
    fn tool_choice_any_maps_to_required() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"name": "f", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "any"},
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.tool_choice, Some(ToolChoice::Required));
    }

    #[test]
    fn serialize_response_shapes_messages_body() {
        let resp = ChatResponse {
            model: "claude-3-5-sonnet-20241022".into(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![ToolCall {
                id: "tu_1".into(),
                name: "f".into(),
                arguments: "{\"x\":1}".into(),
            }],
            finish_reason: FinishReason::ToolCalls,
            usage: TokenUsage {
                input_tokens: 800,
                output_tokens: 5,
                cache_read_tokens: 200,
                ..Default::default()
            },
            provider_response_id: Some("msg_1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["type"], "message");
        assert_eq!(j["stop_reason"], "tool_use");
        assert_eq!(j["content"][0]["text"], "ok");
        assert_eq!(j["content"][1]["type"], "tool_use");
        assert_eq!(j["content"][1]["input"]["x"], 1);
        assert_eq!(j["usage"]["cache_read_input_tokens"], 200);
    }
}
