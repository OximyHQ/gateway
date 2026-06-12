//! Google Gemini `generateContent` egress transport. The model id lives in the
//! URL path (`/v1beta/models/{model}:generateContent`) and the API key is a query
//! param. Roles map user→user, assistant→model; system turns are hoisted into
//! `systemInstruction`. Gemini exposes NO idempotency header, so the transport
//! declares `supports_idempotency: false` and forwards our key as
//! `x-idempotency-key` purely for audit correlation (no upstream effect). Usage
//! `promptTokenCount` INCLUDES cached, so we split. Streaming in Task 14.
//!
//! Tool calls and multimodal images are forwarded with full fidelity: `req.tools`
//! map to `functionDeclarations`, `tool_choice` to `functionCallingConfig`,
//! assistant tool calls to `functionCall` parts, tool results to `functionResponse`
//! parts, and images to `inlineData` (URL images are fetched and inlined, since
//! Gemini does not fetch arbitrary URLs). Streaming stays TEXT-ONLY (tool-call
//! deltas in the stream path are deferred — see blockers).

use async_trait::async_trait;
use base64::Engine;
use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::provider::{Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};
use crate::toolcall::{ToolCall, ToolChoice, ToolDef};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const DEFAULT_IMAGE_MIME: &str = "image/jpeg";
/// Sent when fetching URL images to inline — many CDNs (e.g. Wikimedia) 403 a
/// request with no User-Agent, which would otherwise fail the whole call.
const IMAGE_FETCH_UA: &str = "oximy-gateway/0.1 (+https://github.com/OximyHQ/gateway)";

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

    /// Fetch the bytes of a URL image with the transport's own client and inline
    /// them as a base64 `inlineData` part. Gemini does not fetch arbitrary URLs,
    /// so inlining is mandatory. The MIME type is taken from the response
    /// `Content-Type`, falling back to `image/jpeg`.
    async fn fetch_inline_image(&self, url: &str) -> Result<WirePart, ProviderError> {
        let resp = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, IMAGE_FETCH_UA)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Transport(format!(
                "image fetch failed: {} {}",
                resp.status().as_u16(),
                url
            )));
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_IMAGE_MIME.to_string());
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(WirePart::inline_data(mime, data))
    }

    /// Build the request `contents` (and hoisted `systemInstruction`) from the
    /// unified messages. Async because URL images are fetched and inlined.
    async fn build_contents(
        &self,
        messages: &[Message],
    ) -> Result<(Option<WireContent>, Vec<WireContent>), ProviderError> {
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
                Role::User => {
                    let parts = self.user_parts(m).await?;
                    contents.push(WireContent {
                        role: Some("user"),
                        parts,
                    });
                }
                Role::Tool => {
                    contents.push(WireContent {
                        role: Some("user"),
                        parts: vec![tool_response_part(m)],
                    });
                }
                Role::Assistant => {
                    contents.push(WireContent {
                        role: Some("model"),
                        parts: assistant_parts(m),
                    });
                }
            }
        }
        let sys = if system.is_empty() {
            None
        } else {
            Some(WireContent {
                role: None,
                parts: vec![WirePart::text(system)],
            })
        };
        Ok((sys, contents))
    }

    /// Serialize a user message's content parts: text → `{"text":..}`, image →
    /// `{"inlineData":{..}}` (Base64 inlined directly, URL fetched and inlined).
    async fn user_parts(&self, m: &Message) -> Result<Vec<WirePart>, ProviderError> {
        let mut parts = Vec::with_capacity(m.content.len());
        for p in &m.content {
            match p {
                ContentPart::Text { text } => parts.push(WirePart::text(text.clone())),
                ContentPart::Image { source } => match source {
                    ImageSource::Base64 { media_type, data } => {
                        parts.push(WirePart::inline_data(media_type.clone(), data.clone()));
                    }
                    ImageSource::Url { url } => {
                        parts.push(self.fetch_inline_image(url).await?);
                    }
                },
            }
        }
        Ok(parts)
    }
}

/// The Gemini API addresses models by bare id (`gemini-2.0-flash`), but the
/// models.dev catalog namespaces them under the `google/` provider, so a registry
/// id like `google/gemini-2.0-flash` arrives here. Strip the provider prefix so the
/// `generateContent` URL is valid.
fn gemini_model_path(model: &str) -> &str {
    model.strip_prefix("google/").unwrap_or(model)
}

// ---- wire structs (private) ----

#[derive(Serialize)]
struct WireRequest {
    contents: Vec<WireContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<WireContent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTools>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<WireToolConfig>,
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

/// One request content part. Exactly one field is populated per part; the rest are
/// skipped so the emitted JSON is a single-key object Gemini accepts.
#[derive(Serialize, Default)]
struct WirePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<WireInlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<WireFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<WireFunctionResponse>,
}

impl WirePart {
    fn text(t: impl Into<String>) -> Self {
        WirePart {
            text: Some(t.into()),
            ..Default::default()
        }
    }

    fn inline_data(mime_type: String, data: String) -> Self {
        WirePart {
            inline_data: Some(WireInlineData { mime_type, data }),
            ..Default::default()
        }
    }
}

#[derive(Serialize)]
struct WireInlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Serialize)]
struct WireFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize)]
struct WireFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
struct WireTools {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<WireFunctionDecl>,
}

#[derive(Serialize)]
struct WireFunctionDecl {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct WireToolConfig {
    #[serde(rename = "functionCallingConfig")]
    function_calling_config: WireFnCallingConfig,
}

#[derive(Serialize)]
struct WireFnCallingConfig {
    mode: &'static str,
    #[serde(
        rename = "allowedFunctionNames",
        skip_serializing_if = "Option::is_none"
    )]
    allowed_function_names: Option<Vec<String>>,
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
    #[serde(rename = "functionCall", default)]
    function_call: Option<WireRespFunctionCall>,
}

#[derive(Deserialize)]
struct WireRespFunctionCall {
    #[serde(default)]
    name: String,
    #[serde(default)]
    args: serde_json::Value,
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

/// Map an assistant message into Gemini `model` parts: any text first, then a
/// `functionCall` part per tool call (args = parsed `ToolCall.arguments`, or `{}`
/// if it does not parse).
fn assistant_parts(m: &Message) -> Vec<WirePart> {
    let mut parts = Vec::new();
    let text = m.text_content();
    if !text.is_empty() {
        parts.push(WirePart::text(text));
    }
    for call in &m.tool_calls {
        let args = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .unwrap_or_else(|_| serde_json::json!({}));
        parts.push(WirePart {
            function_call: Some(WireFunctionCall {
                name: call.name.clone(),
                args,
            }),
            ..Default::default()
        });
    }
    if parts.is_empty() {
        parts.push(WirePart::text(String::new()));
    }
    parts
}

/// Map a `Role::Tool` message into a Gemini `functionResponse` part. The function
/// name is recovered from `tool_call_id` when present (Gemini keys responses by
/// name, not id); the text content becomes `{"result": <text>}`.
fn tool_response_part(m: &Message) -> WirePart {
    let name = m.tool_call_id.clone().unwrap_or_default();
    WirePart {
        function_response: Some(WireFunctionResponse {
            name,
            response: serde_json::json!({ "result": m.text_content() }),
        }),
        ..Default::default()
    }
}

fn map_tools(tools: &[ToolDef]) -> Vec<WireTools> {
    if tools.is_empty() {
        return Vec::new();
    }
    let function_declarations = tools
        .iter()
        .map(|t| WireFunctionDecl {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        })
        .collect();
    vec![WireTools {
        function_declarations,
    }]
}

fn map_tool_config(tc: Option<&ToolChoice>) -> Option<WireToolConfig> {
    let tc = tc?;
    let (mode, allowed) = match tc {
        ToolChoice::Auto => ("AUTO", None),
        ToolChoice::Required => ("ANY", None),
        ToolChoice::None => ("NONE", None),
        ToolChoice::Function { name } => ("ANY", Some(vec![name.clone()])),
    };
    Some(WireToolConfig {
        function_calling_config: WireFnCallingConfig {
            mode,
            allowed_function_names: allowed,
        },
    })
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
    let mut tool_calls = Vec::new();
    let mut finish = FinishReason::Unknown;
    if let Some(c) = w.candidates.into_iter().next() {
        if let Some(cc) = c.content {
            for p in cc.parts {
                if let Some(fc) = p.function_call {
                    let idx = tool_calls.len();
                    tool_calls.push(ToolCall {
                        id: format!("call_{idx}"),
                        name: fc.name,
                        arguments: serde_json::to_string(&fc.args)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                } else if let Some(t) = p.text {
                    content.push(ContentPart::text(t));
                }
            }
        }
        finish = map_finish(c.finish_reason.as_deref());
    }
    // A tool call always supersedes the provider finish reason: the turn is
    // waiting on tool results regardless of what Gemini labeled it.
    if !tool_calls.is_empty() {
        finish = FinishReason::ToolCalls;
    }
    ChatResponse {
        model: w
            .model_version
            .unwrap_or_else(|| requested_model.to_string()),
        content,
        tool_calls,
        finish_reason: finish,
        usage: map_usage(w.usage_metadata),
        provider_response_id: None,
    }
}

/// Parse one Gemini stream `data:` payload (a partial `generateContent` body)
/// into a unified delta. Text fragments become content deltas; the chunk bearing
/// `finishReason`/`usageMetadata` becomes the terminal delta carrying both.
/// Streaming stays TEXT-ONLY — tool-call deltas in the stream path are deferred.
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
            gemini_model_path(&req.model)
        );
        let (system_instruction, contents) = self.build_contents(&req.messages).await?;
        let generation_config = if req.temperature.is_some() || req.max_tokens.is_some() {
            Some(WireGenConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            })
        } else {
            None
        };
        let tools = map_tools(&req.tools);
        // toolConfig only makes sense alongside declared tools.
        let tool_config = if tools.is_empty() {
            None
        } else {
            map_tool_config(req.tool_choice.as_ref())
        };
        let wire = WireRequest {
            contents,
            system_instruction,
            tools,
            tool_config,
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
            gemini_model_path(&req.model)
        );
        let (system_instruction, contents) = self.build_contents(&req.messages).await?;
        let generation_config = if req.temperature.is_some() || req.max_tokens.is_some() {
            Some(WireGenConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            })
        } else {
            None
        };
        let tools = map_tools(&req.tools);
        let tool_config = if tools.is_empty() {
            None
        } else {
            map_tool_config(req.tool_choice.as_ref())
        };
        let wire = WireRequest {
            contents,
            system_instruction,
            tools,
            tool_config,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use serde_json::json;

    #[test]
    fn strips_google_provider_prefix() {
        assert_eq!(
            gemini_model_path("google/gemini-2.0-flash"),
            "gemini-2.0-flash"
        );
        assert_eq!(gemini_model_path("gemini-1.5-pro"), "gemini-1.5-pro");
        assert_eq!(
            gemini_model_path("gemini-flash-latest"),
            "gemini-flash-latest"
        );
    }

    #[test]
    fn tools_map_to_function_declarations() {
        let tools = vec![ToolDef {
            name: "get_weather".into(),
            description: Some("Look up weather".into()),
            parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        }];
        let wire = map_tools(&tools);
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v[0]["functionDeclarations"][0]["name"], "get_weather");
        assert_eq!(
            v[0]["functionDeclarations"][0]["description"],
            "Look up weather"
        );
        assert_eq!(
            v[0]["functionDeclarations"][0]["parameters"]["type"],
            "object"
        );
    }

    #[test]
    fn empty_tools_emit_no_declarations() {
        assert!(map_tools(&[]).is_empty());
    }

    #[test]
    fn tool_choice_auto_maps_to_auto() {
        let cfg = map_tool_config(Some(&ToolChoice::Auto)).unwrap();
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["functionCallingConfig"]["mode"], "AUTO");
        assert!(
            v["functionCallingConfig"]
                .get("allowedFunctionNames")
                .is_none()
        );
    }

    #[test]
    fn tool_choice_required_maps_to_any() {
        let cfg = map_tool_config(Some(&ToolChoice::Required)).unwrap();
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["functionCallingConfig"]["mode"], "ANY");
    }

    #[test]
    fn tool_choice_none_maps_to_none() {
        let cfg = map_tool_config(Some(&ToolChoice::None)).unwrap();
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["functionCallingConfig"]["mode"], "NONE");
    }

    #[test]
    fn tool_choice_function_maps_to_any_with_allowed_names() {
        let cfg = map_tool_config(Some(&ToolChoice::Function {
            name: "get_weather".into(),
        }))
        .unwrap();
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["functionCallingConfig"]["mode"], "ANY");
        assert_eq!(
            v["functionCallingConfig"]["allowedFunctionNames"][0],
            "get_weather"
        );
    }

    #[tokio::test]
    async fn base64_image_becomes_inline_data() {
        let g = Gemini::new();
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentPart::text("describe"),
                ContentPart::Image {
                    source: ImageSource::Base64 {
                        media_type: "image/png".into(),
                        data: "AAAA".into(),
                    },
                },
            ],
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        let parts = g.user_parts(&msg).await.unwrap();
        let v = serde_json::to_value(&parts).unwrap();
        assert_eq!(v[0]["text"], "describe");
        assert_eq!(v[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(v[1]["inlineData"]["data"], "AAAA");
        // single-key part objects: no stray null fields
        assert!(v[1].get("text").is_none());
    }

    #[test]
    fn assistant_tool_call_becomes_function_call_part() {
        let m = Message {
            role: Role::Assistant,
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call_0".into(),
                name: "get_weather".into(),
                arguments: "{\"city\":\"SF\"}".into(),
            }],
            tool_call_id: None,
        };
        let parts = assistant_parts(&m);
        let v = serde_json::to_value(&parts).unwrap();
        assert_eq!(v[0]["functionCall"]["name"], "get_weather");
        assert_eq!(v[0]["functionCall"]["args"]["city"], "SF");
    }

    #[test]
    fn assistant_bad_args_fall_back_to_empty_object() {
        let m = Message {
            role: Role::Assistant,
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call_0".into(),
                name: "f".into(),
                arguments: "not json".into(),
            }],
            tool_call_id: None,
        };
        let parts = assistant_parts(&m);
        let v = serde_json::to_value(&parts).unwrap();
        assert_eq!(v[0]["functionCall"]["args"], json!({}));
    }

    #[test]
    fn tool_message_becomes_function_response_part() {
        let m = Message {
            role: Role::Tool,
            content: vec![ContentPart::text("72F and sunny")],
            tool_calls: Vec::new(),
            tool_call_id: Some("get_weather".into()),
        };
        let part = tool_response_part(&m);
        let v = serde_json::to_value(&part).unwrap();
        assert_eq!(v["functionResponse"]["name"], "get_weather");
        assert_eq!(v["functionResponse"]["response"]["result"], "72F and sunny");
    }

    #[test]
    fn response_function_call_becomes_tool_call_and_finish_tool_calls() {
        let w: WireResponse = serde_json::from_value(json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "let me check"},
                        {"functionCall": {"name": "get_weather", "args": {"city": "SF"}}}
                    ]
                },
                "finishReason": "STOP"
            }],
            "modelVersion": "gemini-2.0-flash"
        }))
        .unwrap();
        let resp = map_response(w, "google/gemini-2.0-flash");
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_0");
        assert_eq!(resp.tool_calls[0].name, "get_weather");
        let args: serde_json::Value = serde_json::from_str(&resp.tool_calls[0].arguments).unwrap();
        assert_eq!(args["city"], "SF");
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.text(), "let me check");
    }

    #[test]
    fn response_text_only_still_works() {
        let w: WireResponse = serde_json::from_value(json!({
            "candidates": [{
                "content": {"parts": [{"text": "hello"}]},
                "finishReason": "STOP"
            }]
        }))
        .unwrap();
        let resp = map_response(w, "gemini-1.5-pro");
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.text(), "hello");
    }
}
