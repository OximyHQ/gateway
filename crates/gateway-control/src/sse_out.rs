//! Renders unified `StreamDelta`s into the OpenAI `chat.completion.chunk` SSE
//! wire sequence. The exact event order matters: each delta → one
//! `data: {chunk}\n\n`; a terminal `data: [DONE]\n\n` closes the stream (strict
//! SDK clients reject a missing `[DONE]`). The final chunk carries `usage`
//! (incl. the Oximy `cost`) when present — never dropped on abort (the lifecycle
//! commits regardless; this only formats what arrived).

use gateway_llm::StreamDelta;
use gateway_spine::Usd;

/// Format one delta as a single SSE `data:` line block (no trailing `[DONE]`).
pub fn delta_to_sse(model: &str, delta: &StreamDelta, cost: Option<Usd>) -> String {
    let mut chunk = serde_json::json!({
        "id": "chatcmpl-oximy",
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": serde_json::Value::Null,
        }],
    });

    if let Some(text) = &delta.content_delta {
        chunk["choices"][0]["delta"]["content"] = serde_json::Value::String(text.clone());
    }
    if let Some(reason) = delta.finish_reason {
        let s = match reason {
            gateway_llm::FinishReason::Stop => "stop",
            gateway_llm::FinishReason::Length => "length",
            gateway_llm::FinishReason::ToolCalls => "tool_calls",
            gateway_llm::FinishReason::ContentFilter => "content_filter",
            gateway_llm::FinishReason::Unknown => "stop",
        };
        chunk["choices"][0]["finish_reason"] = serde_json::Value::String(s.into());
    }
    if let Some(usage) = &delta.usage {
        chunk["usage"] = serde_json::json!({
            "prompt_tokens": usage.input_tokens + usage.cache_read_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.total(),
            "cost": cost.map(|c| c.as_dollars_f64()),
        });
    }

    format!("data: {}\n\n", chunk)
}

/// The terminal sentinel every OpenAI stream must end with.
pub fn done_event() -> String {
    "data: [DONE]\n\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::FinishReason;
    use gateway_spine::TokenUsage;

    #[test]
    fn content_delta_renders_chunk() {
        let d = StreamDelta::text("hel");
        let s = delta_to_sse("gpt-4o", &d, None);
        assert!(s.starts_with("data: "));
        assert!(s.ends_with("\n\n"));
        let body = s.strip_prefix("data: ").unwrap().trim_end();
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "hel");
    }

    #[test]
    fn terminal_delta_carries_usage_and_cost() {
        let d = StreamDelta::finish(
            FinishReason::Stop,
            TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            },
        );
        let s = delta_to_sse("gpt-4o", &d, Some(Usd::from_micros(7_500)));
        let body = s.strip_prefix("data: ").unwrap().trim_end();
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["total_tokens"], 1500);
        assert_eq!(v["usage"]["cost"], 0.0075);
    }

    #[test]
    fn done_event_is_exact() {
        assert_eq!(done_event(), "data: [DONE]\n\n");
    }
}
