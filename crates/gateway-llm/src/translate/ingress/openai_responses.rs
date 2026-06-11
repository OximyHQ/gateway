//! OpenAI Responses INGRESS dialect (`/v1/responses`, client → unified). Differs
//! from Chat Completions: `instructions` is the system prompt; `input` is a string
//! OR a list of typed items (`{role, content:[{type:"input_text"|"input_image"}]}`);
//! the unified response serializes back into an `output[]` array of `message`
//! items. Codex speaks this dialect. `reasoning.effort` maps to the unified
//! `reasoning_effort`. Unmodeled params warn; semantic gaps reject.

use serde::Deserialize;
use serde_json::Value;

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::req::{ChatRequest, ReasoningEffort};
use crate::resp::{ChatResponse, FinishReason};
use crate::translate::warn::{IngressError, Translated, Warning};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    #[serde(default)]
    instructions: Option<String>,
    input: Value,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    max_output_tokens: Option<i64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    reasoning: Option<Value>,
    #[serde(default)]
    user: Option<String>,
}

fn parse_input_role(role: &str) -> Role {
    match role {
        "system" | "developer" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

/// One input item's content is a list of typed parts (`input_text`/`input_image`).
fn parse_item_content(content: &Value) -> Result<Vec<ContentPart>, IngressError> {
    match content {
        Value::String(s) => Ok(vec![ContentPart::text(s)]),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for p in parts {
                let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "input_text" | "output_text" | "text" => {
                        if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                            out.push(ContentPart::text(t));
                        }
                    }
                    "input_image" => {
                        if let Some(url) = p.get("image_url").and_then(|v| v.as_str()) {
                            out.push(ContentPart::Image {
                                source: ImageSource::Url {
                                    url: url.to_string(),
                                },
                            });
                        }
                    }
                    other => {
                        return Err(IngressError::Unsupported {
                            feature: format!("responses input part `{other}`"),
                        });
                    }
                }
            }
            Ok(out)
        }
        other => Err(IngressError::Malformed(format!(
            "invalid input content: {other}"
        ))),
    }
}

fn parse_reasoning(reasoning: &Value) -> Option<ReasoningEffort> {
    match reasoning.get("effort").and_then(|e| e.as_str())? {
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        _ => None,
    }
}

/// Parse a raw OpenAI Responses body into a unified request + warnings.
pub fn parse_request(body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
    let wire: WireRequest =
        serde_json::from_value(body.clone()).map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings: Vec<Warning> = Vec::new();

    let mut messages = Vec::new();
    if let Some(instr) = &wire.instructions
        && !instr.is_empty()
    {
        messages.push(Message::text(Role::System, instr.clone()));
    }

    match &wire.input {
        Value::String(s) => messages.push(Message::text(Role::User, s.clone())),
        Value::Array(items) => {
            for item in items {
                let role = item
                    .get("role")
                    .and_then(|r| r.as_str())
                    .map(parse_input_role)
                    .unwrap_or(Role::User);
                let content = match item.get("content") {
                    Some(c) => parse_item_content(c)?,
                    None => Vec::new(),
                };
                messages.push(Message {
                    role,
                    content,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                });
            }
        }
        other => return Err(IngressError::Malformed(format!("invalid input: {other}"))),
    }

    if body.get("previous_response_id").is_some() {
        warnings.push(Warning::dropped_param(
            "previous_response_id",
            "server-side conversation state is not yet supported; send full history",
        ));
    }

    let reasoning_effort = wire.reasoning.as_ref().and_then(parse_reasoning);

    Ok(Translated {
        value: ChatRequest {
            model: wire.model,
            messages,
            tools: Vec::new(),
            tool_choice: None,
            temperature: wire.temperature,
            max_tokens: wire.max_output_tokens,
            stream: wire.stream,
            reasoning_effort,
            response_format: None,
            user: wire.user,
        },
        warnings,
    })
}

fn status_str(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Length | FinishReason::ContentFilter => "incomplete",
        _ => "completed",
    }
}

/// Serialize a unified response into an OpenAI Responses `output[]` body.
pub fn serialize_response(resp: &ChatResponse) -> Value {
    let text = resp.text();
    let output = serde_json::json!([{
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}],
    }]);
    serde_json::json!({
        "id": resp.provider_response_id.clone().unwrap_or_else(|| "resp_oximy".into()),
        "object": "response",
        "model": resp.model,
        "status": status_str(resp.finish_reason),
        "output": output,
        "usage": {
            "input_tokens": resp.usage.input_tokens + resp.usage.cache_read_tokens,
            "output_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.total(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::TokenUsage;
    use serde_json::json;

    #[test]
    fn string_input_with_instructions_parses() {
        let body = json!({
            "model": "gpt-4o",
            "instructions": "Be terse.",
            "input": "Hello",
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].role, Role::System);
        assert_eq!(t.value.messages[1].role, Role::User);
        assert_eq!(t.value.messages[1].text_content(), "Hello");
    }

    #[test]
    fn typed_input_items_parse() {
        let body = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "Hi"}],
            }],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].text_content(), "Hi");
    }

    #[test]
    fn reasoning_effort_maps() {
        let body = json!({
            "model": "o3",
            "input": "Hi",
            "reasoning": {"effort": "high"},
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.reasoning_effort, Some(ReasoningEffort::High));
    }

    #[test]
    fn previous_response_id_warns() {
        let body = json!({
            "model": "gpt-4o",
            "input": "Hi",
            "previous_response_id": "resp_prev",
        });
        let t = parse_request(&body).unwrap();
        assert!(
            t.warnings
                .iter()
                .any(|w| w.message.contains("previous_response_id"))
        );
    }

    #[test]
    fn serialize_response_shapes_output_array() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("done")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 4,
                output_tokens: 1,
                ..Default::default()
            },
            provider_response_id: Some("resp_1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["object"], "response");
        assert_eq!(j["status"], "completed");
        assert_eq!(j["output"][0]["content"][0]["text"], "done");
    }
}
