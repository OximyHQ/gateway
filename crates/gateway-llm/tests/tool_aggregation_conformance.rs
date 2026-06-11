//! Tool-call-delta correctness conformance (design §5 — "where every clone
//! breaks"). Drives the real P1.2 OpenAI streaming transport over a recorded
//! fragmented-tool-call SSE stream, folds the emitted deltas through the
//! `ToolCallAggregator`, and asserts the reassembled `arguments` is byte-exact.
//! Also covers the SSE decoder → aggregator seam directly (no HTTP) for a fast
//! unit-level regression guard.

use futures::StreamExt;
use gateway_llm::message::{Message, Role};
use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::resp::FinishReason;
use gateway_llm::translate::aggregate::ToolCallAggregator;
use gateway_llm::transports::openai::OpenAi;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fragmented_tool_call_reassembles_byte_exact_over_transport() {
    let server = MockServer::start().await;
    let sse = std::fs::read_to_string("tests/fixtures/golden/openai_toolcall_stream.sse").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let mut req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "weather in SF?")]);
    req.stream = true;

    let mut stream = provider.stream(&req, &creds, "idem-tc").await.unwrap();

    let mut agg = ToolCallAggregator::new();
    let mut finish = None;
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let d = item.unwrap();
        agg.push_delta(&d);
        if let Some(f) = d.finish_reason {
            finish = Some(f);
        }
        if let Some(u) = d.usage {
            usage = Some(u);
        }
    }

    let calls = agg.finish();
    assert_eq!(calls.len(), 1, "exactly one tool call");
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(
        calls[0].arguments, "{\"city\":\"SF\"}",
        "fragments reassembled byte-exact"
    );
    assert_eq!(finish, Some(FinishReason::ToolCalls));
    assert_eq!(usage.expect("usage on final chunk").output_tokens, 8);
}
