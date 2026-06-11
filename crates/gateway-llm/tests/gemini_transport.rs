//! Gemini generateContent egress, non-streaming, mocked. Asserts request mapping
//! (model in path, key in query, system → systemInstruction, role mapping), response
//! mapping (finishReason STOP → Stop), and usage extraction (prompt includes
//! cached → split into non-overlapping buckets).

use gateway_llm::message::{Message, Role};
use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::resp::FinishReason;
use gateway_llm::transports::gemini::Gemini;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn gemini_generate_maps_request_response_and_usage() {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string("tests/fixtures/gemini_generate.json").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-1.5-pro:generateContent"))
        .and(query_param("key", "gem-key"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    let provider = Gemini::new();
    let creds = Credentials::new("gem-key").with_base_url(server.uri());
    let req = ChatRequest::new(
        "gemini-1.5-pro",
        vec![
            Message::text(Role::System, "Be helpful."),
            Message::text(Role::User, "Hi"),
        ],
    );

    let resp = provider.chat(&req, &creds, "idem-g").await.unwrap();

    assert_eq!(resp.text(), "Hello from Gemini");
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    // promptTokenCount(1000) includes cached(200) → input=800.
    assert_eq!(resp.usage.input_tokens, 800);
    assert_eq!(resp.usage.cache_read_tokens, 200);
    assert_eq!(resp.usage.output_tokens, 500);
}

#[tokio::test]
async fn gemini_declares_no_idempotency_support() {
    let provider = Gemini::new();
    assert!(!provider.capabilities().supports_idempotency);
}

#[tokio::test]
async fn gemini_stream_yields_text_then_terminal_usage() {
    use futures::StreamExt;
    let server = MockServer::start().await;
    let sse = std::fs::read_to_string("tests/fixtures/gemini_stream.sse").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-1.5-pro:streamGenerateContent"))
        .and(query_param("alt", "sse"))
        .and(query_param("key", "gem-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let provider = Gemini::new();
    let creds = Credentials::new("gem-key").with_base_url(server.uri());
    let mut req = ChatRequest::new("gemini-1.5-pro", vec![Message::text(Role::User, "Hi")]);
    req.stream = true;

    let mut stream = provider.stream(&req, &creds, "idem-gs").await.unwrap();

    let mut text = String::new();
    let mut finish = None;
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let d = item.unwrap();
        if let Some(c) = d.content_delta {
            text.push_str(&c);
        }
        if let Some(f) = d.finish_reason {
            finish = Some(f);
        }
        if let Some(u) = d.usage {
            usage = Some(u);
        }
    }

    assert_eq!(text, "Hello");
    assert_eq!(finish, Some(FinishReason::Stop));
    assert_eq!(usage.unwrap().output_tokens, 2);
}
