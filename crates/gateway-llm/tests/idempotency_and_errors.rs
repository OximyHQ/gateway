//! Invariant proof (design §2 — no double-billing): one idempotency key reused
//! across two calls of the same logical request yields a byte-identical
//! `Idempotency-Key` header upstream. Plus the transport error taxonomy mapping.

use gateway_llm::message::{Message, Role};
use gateway_llm::provider::{Credentials, Provider, ProviderError};
use gateway_llm::req::ChatRequest;
use gateway_llm::transports::openai::OpenAi;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ok_body() -> String {
    std::fs::read_to_string("tests/fixtures/openai_chat.json").unwrap()
}

#[tokio::test]
async fn same_idempotency_key_sends_identical_header_across_retries() {
    let server = MockServer::start().await;
    // The mock REQUIRES idempotency-key == "stable-key" and expects exactly 2 hits.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("idempotency-key", "stable-key"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ok_body()))
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);

    // Two calls = original + one retry of the SAME logical request.
    provider.chat(&req, &creds, "stable-key").await.unwrap();
    provider.chat(&req, &creds, "stable-key").await.unwrap();
    // If either call had sent a different/absent header, the mock's `.expect(2)`
    // on the header-matched route would fail on drop.
}

#[tokio::test]
async fn unauthorized_maps_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("{}"))
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-bad").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);
    let err = provider.chat(&req, &creds, "k").await.unwrap_err();
    assert!(matches!(err, ProviderError::Auth));
}

#[tokio::test]
async fn rate_limited_maps_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "12")
                .set_body_string("{}"),
        )
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);
    let err = provider.chat(&req, &creds, "k").await.unwrap_err();
    assert!(matches!(
        err,
        ProviderError::RateLimited {
            retry_after_secs: Some(12)
        }
    ));
}

#[tokio::test]
async fn server_error_maps_to_upstream() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);
    let err = provider.chat(&req, &creds, "k").await.unwrap_err();
    match err {
        ProviderError::Upstream { status, body } => {
            assert_eq!(status, 500);
            assert_eq!(body, "boom");
        }
        other => panic!("expected Upstream, got {other:?}"),
    }
}
