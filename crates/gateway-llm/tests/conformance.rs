//! Golden-fixture conformance harness (design §3 item 8). Real client request
//! shapes recorded from Codex / Claude Code / the OpenAI SDK are parsed into the
//! unified `ChatRequest`; load-bearing fields are asserted; then a unified
//! response is serialized back into each dialect and asserted lossless on the
//! fields a client depends on. This is the merge gate that stops streaming/
//! translation regressions. Adding a provider/dialect = adding a fixture + a case.

use gateway_llm::message::{ContentPart, Role};
use gateway_llm::resp::{ChatResponse, FinishReason};
use gateway_llm::translate::dialect::Dialect;
use gateway_spine::TokenUsage;

fn load(name: &str) -> serde_json::Value {
    let path = format!("tests/fixtures/ingress/{name}");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn sample_response(model: &str) -> ChatResponse {
    ChatResponse {
        model: model.into(),
        content: vec![ContentPart::text("Done.")],
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            ..Default::default()
        },
        provider_response_id: Some("id_1".into()),
    }
}

#[test]
fn openai_chat_codex_fixture_parses_and_round_trips() {
    let body = load("openai_chat_codex.json");
    let t = Dialect::OpenAiChat.parse_request(&body).unwrap();

    // Request fidelity on load-bearing fields.
    assert_eq!(t.value.model, "gpt-4o");
    assert_eq!(t.value.messages[0].role, Role::System);
    assert_eq!(
        t.value.messages[1].text_content(),
        "Refactor this function."
    );
    assert_eq!(t.value.tools.len(), 1);
    assert_eq!(t.value.tools[0].name, "apply_patch");
    assert!(t.value.stream);
    assert_eq!(t.value.temperature, Some(0.2));
    // No-silent-degradation: logit_bias was dropped WITH a warning.
    assert!(t.warnings.iter().any(|w| w.message.contains("logit_bias")));

    // Response serialization fidelity.
    let out = Dialect::OpenAiChat.serialize_response(&sample_response(&t.value.model));
    assert_eq!(out["object"], "chat.completion");
    assert_eq!(out["choices"][0]["message"]["content"], "Done.");
    assert_eq!(out["choices"][0]["finish_reason"], "stop");
    assert_eq!(out["usage"]["completion_tokens"], 20);
}

#[test]
fn anthropic_claude_code_fixture_parses_and_round_trips() {
    let body = load("anthropic_claude_code.json");
    let t = Dialect::AnthropicMessages.parse_request(&body).unwrap();

    assert_eq!(t.value.model, "claude-3-5-sonnet-20241022");
    assert_eq!(t.value.messages[0].role, Role::System);
    assert_eq!(t.value.messages[0].text_content(), "You are Claude Code.");
    assert_eq!(t.value.messages[1].text_content(), "List the files.");
    assert_eq!(t.value.max_tokens, Some(4096));
    assert_eq!(t.value.tools[0].name, "list_files");
    assert!(t.value.stream);

    let out = Dialect::AnthropicMessages.serialize_response(&sample_response(&t.value.model));
    assert_eq!(out["type"], "message");
    assert_eq!(out["content"][0]["text"], "Done.");
    assert_eq!(out["stop_reason"], "end_turn");
    assert_eq!(out["usage"]["input_tokens"], 100);
    assert_eq!(out["usage"]["cache_read_input_tokens"], 10);
}

#[test]
fn openai_responses_sdk_fixture_parses_and_round_trips() {
    let body = load("openai_responses_sdk.json");
    let t = Dialect::OpenAiResponses.parse_request(&body).unwrap();

    assert_eq!(t.value.model, "gpt-4o");
    assert_eq!(t.value.messages[0].role, Role::System);
    assert_eq!(t.value.messages[0].text_content(), "Answer concisely.");
    assert_eq!(t.value.messages[1].text_content(), "What is 2+2?");
    assert_eq!(t.value.max_tokens, Some(256));
    assert_eq!(
        t.value.reasoning_effort,
        Some(gateway_llm::req::ReasoningEffort::Low)
    );

    let out = Dialect::OpenAiResponses.serialize_response(&sample_response(&t.value.model));
    assert_eq!(out["object"], "response");
    assert_eq!(out["status"], "completed");
    assert_eq!(out["output"][0]["content"][0]["text"], "Done.");
    assert_eq!(out["usage"]["output_tokens"], 20);
}
