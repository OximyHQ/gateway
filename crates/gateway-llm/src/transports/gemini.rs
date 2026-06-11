//! Google Gemini `generateContent` egress transport. The model id lives in the
//! URL path (`/v1beta/models/{model}:generateContent`) and the API key is a query
//! param. Roles map user→user, assistant→model; system turns are hoisted into
//! `systemInstruction`. Gemini exposes NO idempotency header, so the transport
//! declares `supports_idempotency: false` and forwards our key as
//! `x-idempotency-key` purely for audit correlation (no upstream effect). Usage
//! `promptTokenCount` INCLUDES cached, so we split. Streaming in Task 14.

use async_trait::async_trait;
use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::{ContentPart, Message, Role};
use crate::provider::{Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub struct Gemini {
    http: reqwest::Client,
}

impl Default for Gemini {
    fn default() -> Self {
        Self::new()
    }
}

impl Gemini {
    pub fn new() -> Self {
        Gemini {
            http: reqwest::Client::new(),
        }
    }

    fn base_url<'a>(&self, creds: &'a Credentials) -> &'a str {
        creds.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
    }
}

// ---- wire structs (private) ----

#[derive(Serialize)]
struct WireRequest {
    contents: Vec<WireContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<WireContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<WireGenConfig>,
}

#[derive(Serialize)]
struct WireGenConfig {
    #[serde(rename = "temperature", skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<i64>,
}

#[derive(Serialize)]
struct WireContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    parts: Vec<WirePart>,
}

#[derive(Serialize)]
struct WirePart {
    text: String,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    candidates: Vec<WireCandidate>,
    #[serde(rename = "modelVersion", default)]
    model_version: Option<String>,
    #[serde(rename = "usageMetadata", default)]
    usage_metadata: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireCandidate {
    #[serde(default)]
    content: Option<WireRespContent>,
    #[serde(rename = "finishReason", default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireRespContent {
    #[serde(default)]
    parts: Vec<WireRespPart>,
}

#[derive(Deserialize)]
struct WireRespPart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: i64,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: i64,
    #[serde(rename = "cachedContentTokenCount", default)]
    cached_content_token_count: i64,
}

// ---- mapping ----

fn split_messages(messages: &[Message]) -> (Option<WireContent>, Vec<WireContent>) {
    let mut system = String::new();
    let mut contents = Vec::new();
    for m in messages {
        match m.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&m.text_content());
            }
            Role::User | Role::Tool => contents.push(WireContent {
                role: Some("user"),
                parts: vec![WirePart {
                    text: m.text_content(),
                }],
            }),
            Role::Assistant => contents.push(WireContent {
                role: Some("model"),
                parts: vec![WirePart {
                    text: m.text_content(),
                }],
            }),
        }
    }
    let sys = if system.is_empty() {
        None
    } else {
        Some(WireContent {
            role: None,
            parts: vec![WirePart { text: system }],
        })
    };
    (sys, contents)
}

fn map_finish(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("STOP") => FinishReason::Stop,
        Some("MAX_TOKENS") => FinishReason::Length,
        Some("SAFETY") | Some("RECITATION") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

fn map_usage(u: Option<WireUsage>) -> TokenUsage {
    let Some(u) = u else {
        return TokenUsage::default();
    };
    let cached = u.cached_content_token_count;
    TokenUsage {
        input_tokens: (u.prompt_token_count - cached).max(0),
        output_tokens: u.candidates_token_count,
        cache_read_tokens: cached,
        cache_write_tokens: 0,
    }
}

fn map_response(w: WireResponse, requested_model: &str) -> ChatResponse {
    let mut content = Vec::new();
    let mut finish = FinishReason::Unknown;
    if let Some(c) = w.candidates.into_iter().next() {
        if let Some(cc) = c.content {
            for p in cc.parts {
                if let Some(t) = p.text {
                    content.push(ContentPart::text(t));
                }
            }
        }
        finish = map_finish(c.finish_reason.as_deref());
    }
    ChatResponse {
        model: w
            .model_version
            .unwrap_or_else(|| requested_model.to_string()),
        content,
        tool_calls: Vec::new(),
        finish_reason: finish,
        usage: map_usage(w.usage_metadata),
        provider_response_id: None,
    }
}

/// Parse one Gemini stream `data:` payload (a partial `generateContent` body)
/// into a unified delta. Text fragments become content deltas; the chunk bearing
/// `finishReason`/`usageMetadata` becomes the terminal delta carrying both.
fn parse_stream_chunk(payload: &str) -> Result<Option<crate::stream::StreamDelta>, ProviderError> {
    use crate::stream::StreamDelta;
    let chunk: WireResponse =
        serde_json::from_str(payload).map_err(|e| ProviderError::Decode(e.to_string()))?;
    let mut delta = StreamDelta::default();
    if let Some(cand) = chunk.candidates.into_iter().next() {
        if let Some(content) = cand.content {
            let mut text = String::new();
            for p in content.parts {
                if let Some(t) = p.text {
                    text.push_str(&t);
                }
            }
            if !text.is_empty() {
                delta.content_delta = Some(text);
            }
        }
        if let Some(reason) = cand.finish_reason {
            delta.finish_reason = Some(map_finish(Some(reason.as_str())));
        }
    }
    if let Some(u) = chunk.usage_metadata {
        delta.usage = Some(map_usage(Some(u)));
    }
    if delta.is_empty() {
        Ok(None)
    } else {
        Ok(Some(delta))
    }
}

#[async_trait]
impl Provider for Gemini {
    fn id(&self) -> &str {
        "gemini"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: true,
            supports_tools: true,
            supports_vision: true,
            supports_idempotency: false,
        }
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url(creds),
            req.model
        );
        let (system_instruction, contents) = split_messages(&req.messages);
        let generation_config = if req.temperature.is_some() || req.max_tokens.is_some() {
            Some(WireGenConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            })
        } else {
            None
        };
        let wire = WireRequest {
            contents,
            system_instruction,
            generation_config,
        };
        let mut rb = self
            .http
            .post(url)
            .query(&[("key", creds.api_key.as_str())])
            .header("x-idempotency-key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::Auth);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited {
                retry_after_secs: None,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                status: status.as_u16(),
                body,
            });
        }
        let wire: WireResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(map_response(wire, &req.model))
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        use crate::sse::{SseDecoder, SseEvent};
        use futures::StreamExt;

        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent",
            self.base_url(creds),
            req.model
        );
        let (system_instruction, contents) = split_messages(&req.messages);
        let generation_config = if req.temperature.is_some() || req.max_tokens.is_some() {
            Some(WireGenConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            })
        } else {
            None
        };
        let wire = WireRequest {
            contents,
            system_instruction,
            generation_config,
        };
        let mut rb = self
            .http
            .post(url)
            .query(&[("alt", "sse"), ("key", creds.api_key.as_str())])
            .header("x-idempotency-key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                status: status.as_u16(),
                body,
            });
        }

        let byte_stream = resp.bytes_stream();
        type State = (
            std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
            SseDecoder,
            Vec<Result<crate::stream::StreamDelta, ProviderError>>,
            bool,
        );
        let init: State = (Box::pin(byte_stream), SseDecoder::new(), Vec::new(), false);
        let out = futures::stream::unfold(
            init,
            |(mut bytes, mut decoder, mut pending, mut done)| async move {
                loop {
                    if let Some(item) = pending.pop() {
                        return Some((item, (bytes, decoder, pending, done)));
                    }
                    if done {
                        return None;
                    }
                    match bytes.next().await {
                        Some(Ok(chunk)) => {
                            let mut produced = Vec::new();
                            for ev in decoder.push(chunk) {
                                match ev {
                                    SseEvent::Done => done = true,
                                    SseEvent::Data(payload) => match parse_stream_chunk(&payload) {
                                        Ok(Some(d)) => produced.push(Ok(d)),
                                        Ok(None) => {}
                                        Err(e) => produced.push(Err(e)),
                                    },
                                }
                            }
                            produced.reverse();
                            pending = produced;
                        }
                        Some(Err(e)) => {
                            done = true;
                            pending = vec![Err(ProviderError::Transport(e.to_string()))];
                        }
                        None => done = true,
                    }
                }
            },
        );
        Ok(Box::pin(out))
    }
}
