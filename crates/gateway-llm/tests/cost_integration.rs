//! Cross-crate seam (P1.1 ↔ P1.2): usage extracted by a transport, priced by the
//! spine registry, equals the exact µUSD. This is the commit-cost contract the
//! HTTP lifecycle (P1.4) wires.

use gateway_llm::message::{Message, Role};
use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::transports::openai::OpenAi;
use gateway_spine::{ModelEntry, ModelPrice, ModelRegistry, Usd};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn registry() -> ModelRegistry {
    let mut r = ModelRegistry::new();
    r.insert(ModelEntry {
        id: "gpt-4o".into(),
        provider: "openai".into(),
        price: ModelPrice {
            input_per_mtok: 2_500_000,   // $2.50/M
            output_per_mtok: 10_000_000, // $10.00/M
            cache_read_per_mtok: 1_250_000,
            cache_write_per_mtok: 0,
        },
        context_window: Some(128_000),
        max_output_tokens: Some(16_384),
        supports_tools: true,
        supports_vision: true,
        supports_streaming: true,
    });
    r
}

#[tokio::test]
async fn transport_usage_prices_exactly_through_registry() {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string("tests/fixtures/openai_chat.json").unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);
    let resp = provider.chat(&req, &creds, "k").await.unwrap();

    // fixture usage: input 800, cache_read 200, output 500.
    // cost = 800*2.5 + 200*1.25 + 500*10 (per-M, in µUSD):
    //   input:  800 * 2_500_000 / 1e6 = 2_000 µUSD
    //   cache:  200 * 1_250_000 / 1e6 =   250 µUSD
    //   output: 500 * 10_000_000 / 1e6 = 5_000 µUSD
    //   total = 7_250 µUSD
    let cost = registry()
        .cost(&resp.model, &resp.usage)
        .expect("known model");
    assert_eq!(cost, Usd::from_micros(7_250));
}
