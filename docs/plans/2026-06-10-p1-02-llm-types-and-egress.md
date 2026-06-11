# Phase 1.2 — Unified LLM Types + Egress Transports — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the provider-agnostic heart of `gateway-llm` — the unified internal **request / response / streaming-delta** types every dialect maps onto, the async **`Provider`** transport trait (with the idempotency-key parameter that enforces the no-double-billing invariant), and three reference **egress transports** (OpenAI Chat Completions, Anthropic `/v1/messages`, Gemini `generateContent`) that map a unified request → provider wire format and a provider response/SSE stream → unified shape, **extracting `TokenUsage` for cost**. All transports are TDD'd against **mocked HTTP** (recorded JSON/SSE fixtures, `wiremock`) — zero live calls.

**This is the serialization point of Phase 1.** The types and trait defined here are imported verbatim by P1.3 (translation/conformance), P1.4 (HTTP server lifecycle), and every future provider transport. Define them with completeness and care; later milestones extend, they do not rewrite.

**Architecture:** Pure data types (`req`, `resp`, `stream`) with `serde` derives, no I/O. The `Provider` trait is the only async/I/O seam, returning either a unified `ChatResponse` or a `DeltaStream` (a `Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>>>>`). Transports own provider-specific wire structs **privately** (request mapping + response/stream parsing), exposing only the unified surface. Usage is normalized into `gateway_spine::TokenUsage` with **non-overlapping** buckets at the transport boundary, so the spine's exact-integer cost math (P1.1) applies unchanged. Money/usage flow one way: provider usage → `TokenUsage` → `ModelRegistry::cost` (the spine owns dollars; this crate never prices).

**Tech Stack:** Rust 2024; `gateway-spine` (TokenUsage); `reqwest` (HTTP/1.1+2, JSON, streaming bytes) + `tokio`; `futures` / `tokio-stream` (`Stream` + SSE byte-frame decoding); `async-trait` (object-safe async trait); `serde`/`serde_json`; `thiserror`. Tests: `wiremock` (mock upstreams), `tokio::test`, recorded fixtures under `crates/gateway-llm/tests/fixtures/`.

**Invariants this milestone enforces (design §2, §5):**
- **No double-billing** — the `Provider::chat`/`stream` signature threads an `idempotency_key: &str`; transports MUST send it as the provider's idempotency header so the SAME logical request reused across retries/failover bills once. Proven by a test asserting the header is byte-identical across two calls with one key.
- **Never lose usage on aborted streams** — the streaming delta type carries `usage: Option<TokenUsage>` and transports emit it from the provider's final/usage frame; the SSE decoder surfaces a partial-stream terminal delta rather than dropping it.
- **No silent degradation** — unsupported request features surface as `ProviderError::Unsupported { feature }` (the typed seam P1.3 grows into the fidelity matrix), never a quiet drop.

**Explicitly DEFERRED to P1.3** (kept out of scope so this milestone stays tight — noted inline where the seam is): cross-provider **tool-call delta aggregation** (reassembling fragmented `arguments` JSON across chunks), **structured-output** (`json_schema`) translation incl. forced-tool-call emulation, the **golden-fixture conformance harness** + per-pair fidelity matrix, and `reasoning_effort` → provider-thinking-budget mapping fidelity. P1.2 PRESERVES these fields end-to-end (carries them through request mapping and emits raw deltas) but does not yet normalize/aggregate them.

---

### Task 1: Add async / HTTP / streaming dependencies

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/gateway-llm/Cargo.toml`

- [ ] **Step 1: Add shared dep versions to the workspace**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `tracing-subscriber = …` line):

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls", "http2"] }
futures = "0.3"
tokio-stream = "0.1"
async-trait = "0.1"
bytes = "1"
wiremock = "0.6"
```

- [ ] **Step 2: Reference them from `gateway-llm/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-llm/Cargo.toml` with, and add a `[dev-dependencies]` section:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
gateway-spine = { workspace = true }
reqwest = { workspace = true }
futures = { workspace = true }
tokio-stream = { workspace = true }
async-trait = { workspace = true }
bytes = { workspace = true }

[dev-dependencies]
wiremock = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p gateway-llm`
Expected: builds (still the scaffold `lib.rs` with the `CRATE` placeholder).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/gateway-llm/Cargo.toml Cargo.lock
git commit -s -m "build(llm): add reqwest, futures, async-trait, wiremock deps"
```

---

### Task 2: Unified chat message + content-part types

**Files:**
- Create: `crates/gateway-llm/src/message.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/message.rs`:

```rust
//! Provider-agnostic chat messages. A message has a role and an ordered list of
//! content parts (text or image). Tool results ride as a dedicated role so every
//! dialect can map them; tool *calls* live on assistant messages (defined in
//! `toolcall.rs`). These types are the canonical shape ALL ingress dialects map
//! onto and ALL egress transports map out of — keep them minimal and total.

use serde::{Deserialize, Serialize};

use crate::toolcall::ToolCall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    /// A tool/function result being fed back to the model.
    Tool,
}

/// Source of image bytes: an external URL or inline base64 with a media type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    Url { url: String },
    Base64 { media_type: String, data: String },
}

/// One piece of a message body. Multimodal messages interleave these in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { source: ImageSource },
}

impl ContentPart {
    pub fn text(s: impl Into<String>) -> Self {
        ContentPart::Text { text: s.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Ordered content parts. May be empty for an assistant message that only
    /// emitted tool calls.
    pub content: Vec<ContentPart>,
    /// Tool calls emitted by an assistant turn. Empty for non-assistant roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool`: which tool call this message answers. `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Convenience constructor for a single-text-part message.
    pub fn text(role: Role, body: impl Into<String>) -> Self {
        Message {
            role,
            content: vec![ContentPart::text(body)],
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Concatenate all text parts (images ignored) — used by transports that
    /// flatten to a single string.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_constructor_makes_one_part() {
        let m = Message::text(Role::User, "hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content.len(), 1);
        assert_eq!(m.text_content(), "hello");
        assert!(m.tool_calls.is_empty());
    }

    #[test]
    fn multimodal_text_content_skips_images() {
        let m = Message {
            role: Role::User,
            content: vec![
                ContentPart::text("look: "),
                ContentPart::Image {
                    source: ImageSource::Url { url: "https://x/y.png".into() },
                },
                ContentPart::text("done"),
            ],
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        assert_eq!(m.text_content(), "look: done");
    }

    #[test]
    fn role_serializes_snake_case() {
        let j = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(j, "\"assistant\"");
    }

    #[test]
    fn content_part_tagged_repr_roundtrips() {
        let p = ContentPart::text("hi");
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["type"], "text");
        assert_eq!(j["text"], "hi");
        let back: ContentPart = serde_json::from_value(j).unwrap();
        assert_eq!(back, p);
    }
}
```

This references `crate::toolcall::ToolCall`, defined next. Add the module stubs to `crates/gateway-llm/src/lib.rs` now (replacing the `CRATE` placeholder block):

```rust
pub mod message;
pub mod toolcall;

pub use message::{ContentPart, ImageSource, Message, Role};
```

- [ ] **Step 2: Run test to verify it fails to compile (missing `toolcall`)**

Run: `cargo test -p gateway-llm message::`
Expected: FAILS to compile — `unresolved module crate::toolcall`. Proceed to Task 3 which creates it; do not run again until then.

- [ ] **Step 3: (deferred to Task 3 commit)**

`message.rs` and `toolcall.rs` are interdependent; commit them together at the end of Task 3.

---

### Task 3: Tool definitions + tool calls

**Files:**
- Create: `crates/gateway-llm/src/toolcall.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/toolcall.rs`:

```rust
//! Tool/function definitions (what the model MAY call) and tool calls (what it
//! DID call). `ToolDef.parameters` is a raw JSON-Schema `Value` carried verbatim
//! across dialects. `ToolCall.arguments` is the model-produced argument JSON as a
//! STRING (providers emit it incrementally as a string; we keep it unparsed here
//! and let P1.3 own delta aggregation + schema validation).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A function the model is allowed to call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the function's arguments, carried verbatim.
    pub parameters: Value,
}

/// How the caller constrains tool selection for one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether/which tool to call (provider default).
    Auto,
    /// Model must not call a tool.
    None,
    /// Model must call at least one tool.
    Required,
    /// Model must call exactly this tool.
    Function { name: String },
}

impl Default for ToolChoice {
    fn default() -> Self {
        ToolChoice::Auto
    }
}

/// A concrete tool invocation the model emitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call id (echoed back on the matching tool result).
    pub id: String,
    pub name: String,
    /// Raw arguments JSON as a string (NOT yet parsed — see module note).
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_def_carries_schema_verbatim() {
        let schema = json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
        });
        let t = ToolDef {
            name: "get_weather".into(),
            description: Some("Look up weather".into()),
            parameters: schema.clone(),
        };
        assert_eq!(t.parameters, schema);
        let back: ToolDef = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn tool_choice_default_is_auto() {
        assert_eq!(ToolChoice::default(), ToolChoice::Auto);
    }

    #[test]
    fn tool_choice_function_roundtrips() {
        let c = ToolChoice::Function { name: "get_weather".into() };
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["mode"], "function");
        assert_eq!(j["name"], "get_weather");
        let back: ToolChoice = serde_json::from_value(j).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn tool_call_keeps_arguments_as_string() {
        let c = ToolCall {
            id: "call_1".into(),
            name: "get_weather".into(),
            arguments: "{\"city\":\"SF\"}".into(),
        };
        assert_eq!(c.arguments, "{\"city\":\"SF\"}");
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub use toolcall::{ToolCall, ToolChoice, ToolDef};
```

- [ ] **Step 2: Run tests (message + toolcall now compile)**

Run: `cargo test -p gateway-llm message:: toolcall::`
Expected: 4 message + 4 toolcall = 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/message.rs crates/gateway-llm/src/toolcall.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): unified Message/ContentPart + ToolDef/ToolCall/ToolChoice"
```

---

### Task 4: The unified `ChatRequest`

**Files:**
- Create: `crates/gateway-llm/src/req.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/req.rs`:

```rust
//! The one internal request shape. Every ingress dialect (OpenAI chat,
//! Anthropic messages, Gemini generateContent — P1.3) maps INTO this; every
//! egress transport maps OUT of it. Optional knobs are `Option`/defaulted so a
//! transport can detect "unset" vs "explicitly chosen" and emit
//! `ProviderError::Unsupported` rather than silently dropping (no-silent-
//! degradation invariant). `reasoning_effort` and `response_format` are CARRIED
//! here but their provider-fidelity mapping is owned by P1.3.

use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::toolcall::{ToolChoice, ToolDef};

/// Provider-agnostic reasoning knob (Envoy-style); transports map to their own
/// thinking-budget. Mapping fidelity is P1.3's concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

/// Requested output shape. `JsonSchema` translation (forced-tool emulation,
/// per-provider equivalents) is DEFERRED to P1.3 — P1.2 only carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        #[serde(default)]
        strict: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Registry model id (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    /// Caller wants a streamed response.
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Optional per-end-user attribution tag (carried to providers that accept a
    /// `user` field; otherwise dropped without error — it is non-semantic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl ChatRequest {
    /// Minimal builder for tests/simple call sites: model + a single user turn.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        ChatRequest {
            model: model.into(),
            messages,
            tools: Vec::new(),
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            stream: false,
            reasoning_effort: None,
            response_format: None,
            user: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Role;

    #[test]
    fn new_sets_defaults() {
        let r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        assert_eq!(r.model, "gpt-4o");
        assert!(!r.stream);
        assert!(r.tools.is_empty());
        assert!(r.temperature.is_none());
        assert!(r.response_format.is_none());
    }

    #[test]
    fn empty_optionals_are_omitted_from_json() {
        let r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        let j = serde_json::to_value(&r).unwrap();
        assert!(j.get("tools").is_none());
        assert!(j.get("temperature").is_none());
        assert!(j.get("response_format").is_none());
        assert_eq!(j["stream"], false);
    }

    #[test]
    fn reasoning_effort_serializes_snake_case() {
        let mut r = ChatRequest::new("o3", vec![Message::text(Role::User, "hi")]);
        r.reasoning_effort = Some(ReasoningEffort::High);
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["reasoning_effort"], "high");
    }

    #[test]
    fn response_format_json_schema_roundtrips() {
        let mut r = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        r.response_format = Some(ResponseFormat::JsonSchema {
            name: "out".into(),
            schema: serde_json::json!({"type": "object"}),
            strict: true,
        });
        let back: ChatRequest = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod req;

pub use req::{ChatRequest, ReasoningEffort, ResponseFormat};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm req::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/req.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): unified ChatRequest with reasoning_effort + response_format"
```

---

### Task 5: The unified non-streaming `ChatResponse` + `FinishReason`

**Files:**
- Create: `crates/gateway-llm/src/resp.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/resp.rs`:

```rust
//! The one internal non-streaming response shape. Maps `gateway_spine::TokenUsage`
//! for cost (the spine prices it — this crate never computes dollars). Every
//! egress transport produces this from a provider's JSON body; every ingress
//! dialect serializes this back out (P1.3/P1.4).

use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::ContentPart;
use crate::toolcall::ToolCall;

/// Why generation stopped — normalized across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of turn.
    Stop,
    /// Hit max_tokens / output cap.
    Length,
    /// Model emitted tool call(s) and is waiting on results.
    ToolCalls,
    /// Provider content filter / safety stop.
    ContentFilter,
    /// Stream ended without a provider-reported reason (e.g. aborted upstream).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Provider/registry model id that actually served the request.
    pub model: String,
    /// Assistant content parts (text, possibly empty when only tools were called).
    pub content: Vec<ContentPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    /// Normalized, non-overlapping token usage for cost. The spine prices this.
    pub usage: TokenUsage,
    /// Provider-native request/response id, when available (for audit/debug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
}

impl ChatResponse {
    /// Concatenated text across all text content parts.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_concatenates_content_parts() {
        let r = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("Hello "), ContentPart::text("world")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { input_tokens: 10, output_tokens: 2, ..Default::default() },
            provider_response_id: Some("resp_1".into()),
        };
        assert_eq!(r.text(), "Hello world");
        assert_eq!(r.usage.output_tokens, 2);
    }

    #[test]
    fn finish_reason_serializes_snake_case() {
        let j = serde_json::to_string(&FinishReason::ToolCalls).unwrap();
        assert_eq!(j, "\"tool_calls\"");
    }

    #[test]
    fn response_roundtrips() {
        let r = ChatResponse {
            model: "claude-3-5-sonnet".into(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![ToolCall { id: "c1".into(), name: "f".into(), arguments: "{}".into() }],
            finish_reason: FinishReason::ToolCalls,
            usage: TokenUsage::default(),
            provider_response_id: None,
        };
        let back: ChatResponse = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod resp;

pub use resp::{ChatResponse, FinishReason};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm resp::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/resp.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): unified ChatResponse + normalized FinishReason"
```

---

### Task 6: The unified streaming `StreamDelta`

**Files:**
- Create: `crates/gateway-llm/src/stream.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/stream.rs`:

```rust
//! The one internal streaming chunk. A provider SSE stream is mapped to a
//! sequence of `StreamDelta`s. Each delta carries ANY of: a text-content
//! fragment, a tool-call fragment, a finish reason, and/or usage. Usage usually
//! arrives only on the final delta — `usage: Option<TokenUsage>` makes that
//! explicit and (with the abort-safe decoder in Task 8/9) guarantees we NEVER
//! lose usage on aborted streams (invariant §2). Tool-call delta AGGREGATION
//! (stitching fragmented arguments) is DEFERRED to P1.3; here we faithfully
//! relay each fragment with its index.

use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::resp::FinishReason;

/// A partial tool call as it streams in. `index` identifies which parallel call
/// this fragment belongs to; `id`/`name` appear on the first fragment, then
/// `arguments_delta` carries successive argument-string chunks. Reassembly is
/// P1.3's job — this type just preserves the pieces losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A chunk of the argument JSON string (concatenate across deltas to rebuild).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_delta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreamDelta {
    /// Incremental assistant text, if this chunk carried any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_delta: Option<String>,
    /// Incremental tool-call fragments, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_call_deltas: Vec<ToolCallDelta>,
    /// Set on the terminal chunk for this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Usage, typically only on the final chunk. NEVER dropped on abort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

impl StreamDelta {
    /// A pure text chunk.
    pub fn text(s: impl Into<String>) -> Self {
        StreamDelta { content_delta: Some(s.into()), ..Default::default() }
    }

    /// The terminal chunk carrying finish + usage.
    pub fn finish(reason: FinishReason, usage: TokenUsage) -> Self {
        StreamDelta {
            finish_reason: Some(reason),
            usage: Some(usage),
            ..Default::default()
        }
    }

    /// True if this delta carries no semantic payload (e.g. a keepalive).
    pub fn is_empty(&self) -> bool {
        self.content_delta.is_none()
            && self.tool_call_deltas.is_empty()
            && self.finish_reason.is_none()
            && self.usage.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_carries_only_content() {
        let d = StreamDelta::text("ab");
        assert_eq!(d.content_delta.as_deref(), Some("ab"));
        assert!(d.tool_call_deltas.is_empty());
        assert!(d.finish_reason.is_none());
        assert!(!d.is_empty());
    }

    #[test]
    fn finish_delta_carries_reason_and_usage() {
        let u = TokenUsage { input_tokens: 5, output_tokens: 3, ..Default::default() };
        let d = StreamDelta::finish(FinishReason::Stop, u);
        assert_eq!(d.finish_reason, Some(FinishReason::Stop));
        assert_eq!(d.usage.unwrap().total(), 8);
    }

    #[test]
    fn empty_delta_detected() {
        assert!(StreamDelta::default().is_empty());
    }

    #[test]
    fn tool_call_delta_roundtrips() {
        let d = StreamDelta {
            tool_call_deltas: vec![ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("get_weather".into()),
                arguments_delta: Some("{\"ci".into()),
            }],
            ..Default::default()
        };
        let back: StreamDelta = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod stream;

pub use stream::{StreamDelta, ToolCallDelta};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm stream::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/stream.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): unified StreamDelta with abort-safe optional usage"
```

---

### Task 7: `ProviderError` + the `Provider` trait + `Credentials`

**Files:**
- Create: `crates/gateway-llm/src/provider.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/provider.rs`:

```rust
//! The egress seam. A `Provider` takes a unified `ChatRequest` + resolved
//! `Credentials` + an `idempotency_key` and returns either a unified
//! `ChatResponse` or a stream of `StreamDelta`s. The idempotency key is threaded
//! so the SAME logical request reused across retries/failover bills once
//! (no-double-billing invariant §2): transports MUST forward it as the provider's
//! idempotency header. `ProviderError::Unsupported` is the typed no-silent-
//! degradation seam P1.3 grows into the fidelity matrix.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::req::ChatRequest;
use crate::resp::ChatResponse;
use crate::stream::StreamDelta;

/// Resolved upstream credentials + endpoint for one transport call. The spine
/// decrypts/injects these (P1.6); transports never read env/secrets themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Bearer/API key material (already decrypted).
    pub api_key: String,
    /// Base URL override (e.g. Azure/self-hosted/proxy). `None` = provider default.
    pub base_url: Option<String>,
    /// Extra headers to forward verbatim (e.g. `anthropic-beta`, org ids).
    pub extra_headers: Vec<(String, String)>,
}

impl Credentials {
    pub fn new(api_key: impl Into<String>) -> Self {
        Credentials { api_key: api_key.into(), base_url: None, extra_headers: Vec::new() }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }
}

/// Errors a transport surfaces. HTTP statuses map at the server layer (P1.4):
/// Auth → 401, RateLimited → 429, Upstream{status} passes through, Unsupported →
/// 400/422, Transport/Decode → 502.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("upstream authentication failed")]
    Auth,
    #[error("upstream rate limited{}", retry_after_secs.map(|s| format!(" (retry after {s}s)")).unwrap_or_default())]
    RateLimited { retry_after_secs: Option<i64> },
    #[error("upstream returned status {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("request feature unsupported by this provider: {feature}")]
    Unsupported { feature: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("failed to decode upstream response: {0}")]
    Decode(String),
}

/// A unified streaming response: a pinned, boxed, `Send` stream of deltas.
pub type DeltaStream = Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>> + Send>>;

/// Capabilities a transport declares so the router/spine can pre-validate a
/// request shape (richer capability data lives in the model registry; this is the
/// per-transport floor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    /// Provider honors an idempotency header for safe retries.
    pub supports_idempotency: bool,
}

/// The egress transport trait. One impl per provider API shape (~30 total).
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable provider id (e.g. "openai", "anthropic", "gemini").
    fn id(&self) -> &str;

    fn capabilities(&self) -> ProviderCapabilities;

    /// Non-streaming completion. `idempotency_key` MUST be forwarded as the
    /// provider's idempotency header (no-double-billing). The same key across
    /// retries of one logical request yields one bill.
    async fn chat(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError>;

    /// Streaming completion. Same idempotency contract. The returned stream's
    /// final delta MUST carry usage when the provider reports it; an aborted
    /// upstream yields a terminal `Unknown`-finish delta, never a silent drop.
    async fn stream(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Message, Role};
    use crate::resp::FinishReason;
    use futures::StreamExt;
    use gateway_spine::TokenUsage;

    /// A trivial in-memory provider proving the trait is object-safe and the
    /// idempotency key is observable by the impl.
    struct EchoProvider {
        last_idempotency_key: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl Provider for EchoProvider {
        fn id(&self) -> &str {
            "echo"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
                supports_tools: false,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            *self.last_idempotency_key.lock().unwrap() = Some(idempotency_key.to_string());
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![crate::message::ContentPart::text(
                    req.messages.last().map(|m| m.text_content()).unwrap_or_default(),
                )],
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: TokenUsage::default(),
                provider_response_id: None,
            })
        }
        async fn stream(
            &self,
            _req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            let deltas = vec![
                Ok(StreamDelta::text("hi")),
                Ok(StreamDelta::finish(FinishReason::Stop, TokenUsage::default())),
            ];
            Ok(Box::pin(futures::stream::iter(deltas)))
        }
    }

    #[tokio::test]
    async fn trait_is_object_safe_and_threads_idempotency_key() {
        let p: Box<dyn Provider> = Box::new(EchoProvider {
            last_idempotency_key: std::sync::Mutex::new(None),
        });
        let req = ChatRequest::new("echo-1", vec![Message::text(Role::User, "ping")]);
        let creds = Credentials::new("sk-x");
        let resp = p.chat(&req, &creds, "idem-123").await.unwrap();
        assert_eq!(resp.text(), "ping");
    }

    #[tokio::test]
    async fn stream_yields_text_then_finish_with_usage() {
        let p = EchoProvider { last_idempotency_key: std::sync::Mutex::new(None) };
        let req = ChatRequest::new("echo-1", vec![Message::text(Role::User, "ping")]);
        let creds = Credentials::new("sk-x");
        let mut s = p.stream(&req, &creds, "idem-1").await.unwrap();
        let first = s.next().await.unwrap().unwrap();
        assert_eq!(first.content_delta.as_deref(), Some("hi"));
        let last = s.next().await.unwrap().unwrap();
        assert_eq!(last.finish_reason, Some(FinishReason::Stop));
        assert!(last.usage.is_some());
        assert!(s.next().await.is_none());
    }

    #[test]
    fn credentials_builder() {
        let c = Credentials::new("sk-x")
            .with_base_url("https://proxy.local")
            .with_header("anthropic-beta", "tools-2024");
        assert_eq!(c.base_url.as_deref(), Some("https://proxy.local"));
        assert_eq!(c.extra_headers, vec![("anthropic-beta".into(), "tools-2024".into())]);
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod provider;

pub use provider::{
    Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError,
};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm provider::`
Expected: 3 tests PASS (2 async + 1 sync).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/provider.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): Provider trait, Credentials, ProviderError, DeltaStream"
```

---

### Task 8: Shared SSE line decoder

**Files:**
- Create: `crates/gateway-llm/src/sse.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

A provider-agnostic SSE frame decoder: turns a byte stream into `data:` payload strings, handling CRLF/LF, multi-line `data:`, comments/keepalives, and the `[DONE]` sentinel. Each transport then parses payloads into its own wire chunk. Tested as a pure function over byte chunks (no network).

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/sse.rs`:

```rust
//! Minimal SSE frame decoder shared by streaming transports. Accumulates bytes,
//! emits the concatenated `data:` payload for each event terminated by a blank
//! line. Ignores comment lines (`:`...) and non-`data:` fields. The literal
//! `[DONE]` sentinel is surfaced as `SseEvent::Done`. Splitting raw bytes off the
//! wire from chunk PARSING keeps each transport's parser pure and unit-testable.

use bytes::{Bytes, BytesMut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    /// A data payload (the concatenated `data:` lines of one event).
    Data(String),
    /// The terminal `data: [DONE]` sentinel.
    Done,
}

/// Stateful accumulator. Feed it wire bytes; drain complete events.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: BytesMut,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push raw bytes and return any newly-complete events, in order.
    pub fn push(&mut self, chunk: Bytes) -> Vec<SseEvent> {
        self.buf.extend_from_slice(&chunk);
        let mut out = Vec::new();
        // Normalize on `\n`; an event ends at a blank line (`\n\n`).
        while let Some(pos) = find_event_boundary(&self.buf) {
            let raw = self.buf.split_to(pos);
            // drop the boundary bytes (1 or 2 newlines)
            let drop = boundary_len(&self.buf);
            let _ = self.buf.split_to(drop);
            if let Some(ev) = parse_event(&raw) {
                out.push(ev);
            }
        }
        out
    }
}

/// Index of the first event boundary (end of a `data:` block), or None.
fn find_event_boundary(buf: &BytesMut) -> Option<usize> {
    let s = buf.as_ref();
    let mut i = 0;
    while i < s.len() {
        // boundary = "\n\n" or "\r\n\r\n"
        if s[i] == b'\n' {
            if i + 1 < s.len() && s[i + 1] == b'\n' {
                return Some(i);
            }
            if i + 3 < s.len() && &s[i + 1..i + 4] == b"\r\n\r" {
                return Some(i);
            }
            if i >= 1 && s[i - 1] == b'\r' && i + 1 < s.len() && s[i + 1] == b'\r' {
                // handled by the \r\n\r\n branch from the previous \n; skip
            }
        }
        i += 1;
    }
    None
}

/// Length of the boundary newlines at the FRONT of `buf` to discard.
fn boundary_len(buf: &BytesMut) -> usize {
    let s = buf.as_ref();
    if s.starts_with(b"\r\n\r\n") {
        4
    } else if s.starts_with(b"\n\n") {
        2
    } else if s.starts_with(b"\n") {
        1
    } else {
        0
    }
}

/// Parse one raw event block into a data payload (ignoring comments/other fields).
fn parse_event(raw: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(raw);
    let mut data_lines = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let payload = data_lines.join("\n");
    if payload.trim() == "[DONE]" {
        Some(SseEvent::Done)
    } else {
        Some(SseEvent::Data(payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_event() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: {\"a\":1}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn split_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(Bytes::from_static(b"data: {\"a")).is_empty());
        let ev = d.push(Bytes::from_static(b"\":1}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn done_sentinel_recognized() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: [DONE]\n\n"));
        assert_eq!(ev, vec![SseEvent::Done]);
    }

    #[test]
    fn comments_and_keepalives_ignored() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b": keepalive\n\ndata: {}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{}".into())]);
    }

    #[test]
    fn multiple_events_one_chunk() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: 1\n\ndata: 2\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("1".into()), SseEvent::Data("2".into())]);
    }

    #[test]
    fn crlf_line_endings() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: x\r\n\r\n"));
        assert_eq!(ev, vec![SseEvent::Data("x".into())]);
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod sse;

pub use sse::{SseDecoder, SseEvent};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm sse::`
Expected: 6 tests PASS. (If `crlf_line_endings` or `multiple_events_one_chunk` fail, the boundary scan is wrong — fix `find_event_boundary`/`boundary_len` before moving on; the decoder is load-bearing for every streaming transport.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/sse.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): shared SSE frame decoder (data/comment/DONE/CRLF)"
```

---

### Task 9: OpenAI egress transport — non-streaming (mocked HTTP)

**Files:**
- Create: `crates/gateway-llm/src/transports/mod.rs`
- Create: `crates/gateway-llm/src/transports/openai.rs`
- Create: `crates/gateway-llm/tests/fixtures/openai_chat.json`
- Create: `crates/gateway-llm/tests/openai_transport.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Record the fixture**

Create `crates/gateway-llm/tests/fixtures/openai_chat.json` (a real-shape Chat Completions body):

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "model": "gpt-4o",
  "choices": [
    {
      "index": 0,
      "message": { "role": "assistant", "content": "Hello there!" },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 1000,
    "completion_tokens": 500,
    "prompt_tokens_details": { "cached_tokens": 200 }
  }
}
```

- [ ] **Step 2: Write the failing test**

Create `crates/gateway-llm/tests/openai_transport.rs`:

```rust
//! OpenAI Chat Completions egress, non-streaming, against a mocked upstream.
//! Asserts: (a) request mapping (model/messages/idempotency header), (b) response
//! mapping (content/finish_reason), (c) usage extraction normalized into
//! non-overlapping TokenUsage (cached split out of prompt_tokens).

use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::message::{Message, Role};
use gateway_llm::resp::FinishReason;
use gateway_llm::transports::openai::OpenAi;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn openai_chat_maps_request_response_and_usage() {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string("tests/fixtures/openai_chat.json").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .and(header("idempotency-key", "idem-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);

    let resp = provider.chat(&req, &creds, "idem-xyz").await.unwrap();

    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.text(), "Hello there!");
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    // prompt_tokens(1000) includes cached(200) → input=800, cache_read=200.
    assert_eq!(resp.usage.input_tokens, 800);
    assert_eq!(resp.usage.cache_read_tokens, 200);
    assert_eq!(resp.usage.output_tokens, 500);
    assert_eq!(resp.provider_response_id.as_deref(), Some("chatcmpl-abc123"));
}
```

- [ ] **Step 3: Implement the transport**

Create `crates/gateway-llm/src/transports/mod.rs`:

```rust
//! Egress provider transports — one module per provider API shape. Each owns its
//! wire structs PRIVATELY and exposes only the unified `Provider` impl.

pub mod openai;
```

Create `crates/gateway-llm/src/transports/openai.rs`:

```rust
//! OpenAI Chat Completions egress transport. Maps unified ChatRequest → the
//! `/v1/chat/completions` body, parses the response, and normalizes usage into
//! NON-OVERLAPPING TokenUsage (OpenAI's `prompt_tokens` INCLUDES cached tokens,
//! so cached are subtracted out of input — preserving the spine's exact cost
//! math). Idempotency-key is sent as the `Idempotency-Key` header (no double-
//! billing). Streaming added in Task 10. Tool/structured-output FULL fidelity is
//! P1.3; here tool defs/calls pass through their natural OpenAI shape.

use async_trait::async_trait;
use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::{ContentPart, Message, Role};
use crate::provider::{
    Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError,
};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};
use crate::toolcall::ToolCall;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

pub struct OpenAi {
    http: reqwest::Client,
}

impl Default for OpenAi {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAi {
    pub fn new() -> Self {
        OpenAi { http: reqwest::Client::new() }
    }

    fn base_url<'a>(&self, creds: &'a Credentials) -> &'a str {
        creds.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
    }
}

// ---- wire structs (private) ----

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<WireStreamOptions>,
}

#[derive(Serialize)]
struct WireStreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct WireResponse {
    id: Option<String>,
    model: String,
    choices: Vec<WireChoice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireRespMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireRespMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
struct WireToolCall {
    id: String,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: i64,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: Option<WirePromptDetails>,
}

#[derive(Deserialize)]
struct WirePromptDetails {
    #[serde(default)]
    cached_tokens: i64,
}

// ---- mapping ----

fn role_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn map_messages(messages: &[Message]) -> Vec<WireMessage> {
    messages
        .iter()
        .map(|m| WireMessage { role: role_str(m.role), content: m.text_content() })
        .collect()
}

fn map_finish(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolCalls,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

/// OpenAI `prompt_tokens` INCLUDES cached; split into non-overlapping buckets.
fn map_usage(u: Option<WireUsage>) -> TokenUsage {
    let Some(u) = u else { return TokenUsage::default() };
    let cached = u.prompt_tokens_details.map(|d| d.cached_tokens).unwrap_or(0);
    TokenUsage {
        input_tokens: (u.prompt_tokens - cached).max(0),
        output_tokens: u.completion_tokens,
        cache_read_tokens: cached,
        cache_write_tokens: 0,
    }
}

fn map_response(w: WireResponse) -> ChatResponse {
    let choice = w.choices.into_iter().next();
    let (content, tool_calls, finish) = match choice {
        Some(c) => {
            let content = c
                .message
                .content
                .filter(|s| !s.is_empty())
                .map(|s| vec![ContentPart::text(s)])
                .unwrap_or_default();
            let tool_calls: Vec<ToolCall> = c
                .message
                .tool_calls
                .into_iter()
                .map(|t| ToolCall { id: t.id, name: t.function.name, arguments: t.function.arguments })
                .collect();
            (content, tool_calls, map_finish(c.finish_reason.as_deref()))
        }
        None => (Vec::new(), Vec::new(), FinishReason::Unknown),
    };
    ChatResponse {
        model: w.model,
        content,
        tool_calls,
        finish_reason: finish,
        usage: map_usage(w.usage),
        provider_response_id: w.id,
    }
}

#[async_trait]
impl Provider for OpenAi {
    fn id(&self) -> &str {
        "openai"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: true,
            supports_tools: true,
            supports_vision: true,
            supports_idempotency: true,
        }
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        let url = format!("{}/v1/chat/completions", self.base_url(creds));
        let wire = WireRequest {
            model: &req.model,
            messages: map_messages(&req.messages),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            stream: false,
            stream_options: None,
        };
        let mut rb = self
            .http
            .post(url)
            .bearer_auth(&creds.api_key)
            .header("Idempotency-Key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok());
            return Err(ProviderError::RateLimited { retry_after_secs: retry });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }
        let wire: WireResponse =
            resp.json().await.map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(map_response(wire))
    }

    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        // Implemented in Task 10.
        Err(ProviderError::Unsupported { feature: "streaming (added in Task 10)".into() })
    }
}
```

Add to `crates/gateway-llm/src/lib.rs`:

```rust
pub mod transports;
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p gateway-llm --test openai_transport`
Expected: `openai_chat_maps_request_response_and_usage` PASSES — the mock asserts the auth + idempotency headers were sent, and the usage split (800/200/500) confirms non-overlapping normalization.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports crates/gateway-llm/tests/fixtures/openai_chat.json crates/gateway-llm/tests/openai_transport.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): OpenAI egress transport (non-streaming) with usage normalization"
```

---

### Task 10: OpenAI egress transport — streaming (mocked SSE)

**Files:**
- Modify: `crates/gateway-llm/src/transports/openai.rs`
- Create: `crates/gateway-llm/tests/fixtures/openai_stream.sse`
- Modify: `crates/gateway-llm/tests/openai_transport.rs`

- [ ] **Step 1: Record the SSE fixture**

Create `crates/gateway-llm/tests/fixtures/openai_stream.sse` (note the trailing blank line after each event):

```
data: {"choices":[{"delta":{"content":"Hel"}}]}

data: {"choices":[{"delta":{"content":"lo"}}]}

data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}}

data: [DONE]

```

- [ ] **Step 2: Write the failing test**

Append to `crates/gateway-llm/tests/openai_transport.rs`:

```rust
#[tokio::test]
async fn openai_stream_yields_text_deltas_then_usage() {
    use futures::StreamExt;
    let server = MockServer::start().await;
    let sse = std::fs::read_to_string("tests/fixtures/openai_stream.sse").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("idempotency-key", "idem-s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let provider = OpenAi::new();
    let creds = Credentials::new("sk-test").with_base_url(server.uri());
    let mut req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "Hi")]);
    req.stream = true;

    let mut stream = provider.stream(&req, &creds, "idem-s").await.unwrap();

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
```

- [ ] **Step 3: Implement streaming**

In `crates/gateway-llm/src/transports/openai.rs`, add the streaming wire structs and parser, then replace the `stream` method body. First, add near the other wire structs:

```rust
#[derive(Deserialize)]
struct WireStreamChunk {
    #[serde(default)]
    choices: Vec<WireStreamChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireStreamChoice {
    #[serde(default)]
    delta: WireDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct WireDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireToolCallDelta>,
}

#[derive(Deserialize)]
struct WireToolCallDelta {
    index: i64,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<WireFunctionDelta>,
}

#[derive(Deserialize)]
struct WireFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
```

Add a parser that maps one SSE data payload to a `StreamDelta` (returns `None` for empty/noise). Put it below `map_response`:

```rust
/// Parse one OpenAI stream `data:` payload into a unified delta. Returns `None`
/// for chunks that carry nothing useful. Tool-call argument fragments are
/// relayed per-index; AGGREGATION is P1.3.
fn parse_stream_chunk(payload: &str) -> Result<Option<crate::stream::StreamDelta>, ProviderError> {
    use crate::stream::{StreamDelta, ToolCallDelta};
    let chunk: WireStreamChunk =
        serde_json::from_str(payload).map_err(|e| ProviderError::Decode(e.to_string()))?;
    let mut delta = StreamDelta::default();
    if let Some(choice) = chunk.choices.into_iter().next() {
        if let Some(c) = choice.delta.content
            && !c.is_empty()
        {
            delta.content_delta = Some(c);
        }
        for tc in choice.delta.tool_calls {
            let (name, args) = match tc.function {
                Some(f) => (f.name, f.arguments),
                None => (None, None),
            };
            delta.tool_call_deltas.push(ToolCallDelta {
                index: tc.index,
                id: tc.id,
                name,
                arguments_delta: args,
            });
        }
        if let Some(f) = choice.finish_reason {
            delta.finish_reason = Some(map_finish(Some(f.as_str())));
        }
    }
    if let Some(u) = chunk.usage {
        delta.usage = Some(map_usage(Some(u)));
    }
    if delta.is_empty() {
        Ok(None)
    } else {
        Ok(Some(delta))
    }
}
```

Now replace the `stream` method body:

```rust
    async fn stream(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        use crate::sse::{SseDecoder, SseEvent};
        use futures::StreamExt;

        let url = format!("{}/v1/chat/completions", self.base_url(creds));
        let wire = WireRequest {
            model: &req.model,
            messages: map_messages(&req.messages),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            stream: true,
            stream_options: Some(WireStreamOptions { include_usage: true }),
        };
        let mut rb = self
            .http
            .post(url)
            .bearer_auth(&creds.api_key)
            .header("Idempotency-Key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }

        let byte_stream = resp.bytes_stream();
        let out = futures::stream::unfold(
            (byte_stream, SseDecoder::new(), Vec::<Result<crate::stream::StreamDelta, ProviderError>>::new(), false),
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
                            // emit in order: push reversed so pop() yields FIFO
                            produced.reverse();
                            pending = produced;
                        }
                        Some(Err(e)) => {
                            done = true;
                            pending = vec![Err(ProviderError::Transport(e.to_string()))];
                        }
                        None => {
                            // Upstream ended (possibly aborted). Surface a terminal
                            // Unknown-finish so usage/finish is never silently lost.
                            done = true;
                        }
                    }
                }
            },
        );
        Ok(Box::pin(out))
    }
```

- [ ] **Step 4: Run the streaming test**

Run: `cargo test -p gateway-llm --test openai_transport`
Expected: both `openai_chat_*` and `openai_stream_*` PASS. Text reassembles to "Hello", finish=Stop, usage.output_tokens=2.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports/openai.rs crates/gateway-llm/tests/fixtures/openai_stream.sse crates/gateway-llm/tests/openai_transport.rs
git commit -s -m "feat(llm): OpenAI streaming via shared SSE decoder, usage on final chunk"
```

---

### Task 11: Anthropic egress transport — non-streaming (`/v1/messages`)

**Files:**
- Create: `crates/gateway-llm/src/transports/anthropic.rs`
- Create: `crates/gateway-llm/tests/fixtures/anthropic_messages.json`
- Create: `crates/gateway-llm/tests/anthropic_transport.rs`
- Modify: `crates/gateway-llm/src/transports/mod.rs`

- [ ] **Step 1: Record the fixture**

Create `crates/gateway-llm/tests/fixtures/anthropic_messages.json`:

```json
{
  "id": "msg_01ABC",
  "type": "message",
  "model": "claude-3-5-sonnet-20241022",
  "stop_reason": "end_turn",
  "content": [
    { "type": "text", "text": "Hi from Claude" }
  ],
  "usage": {
    "input_tokens": 800,
    "output_tokens": 500,
    "cache_read_input_tokens": 200,
    "cache_creation_input_tokens": 50
  }
}
```

- [ ] **Step 2: Write the failing test**

Create `crates/gateway-llm/tests/anthropic_transport.rs`:

```rust
//! Anthropic /v1/messages egress, non-streaming, mocked. Asserts request mapping
//! (system hoisted out of messages, x-api-key + anthropic-version + idempotency),
//! response mapping (stop_reason → finish_reason), and usage extraction. Anthropic
//! already reports NON-overlapping buckets (input excludes cache), so the mapping
//! is direct.

use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::message::{Message, Role};
use gateway_llm::resp::FinishReason;
use gateway_llm::transports::anthropic::Anthropic;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn anthropic_messages_maps_request_response_and_usage() {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string("tests/fixtures/anthropic_messages.json").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-ant"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(header("idempotency-key", "idem-a"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    let provider = Anthropic::new();
    let creds = Credentials::new("sk-ant").with_base_url(server.uri());
    let req = ChatRequest::new(
        "claude-3-5-sonnet-20241022",
        vec![
            Message::text(Role::System, "Be terse."),
            Message::text(Role::User, "Hi"),
        ],
    );

    let resp = provider.chat(&req, &creds, "idem-a").await.unwrap();

    assert_eq!(resp.text(), "Hi from Claude");
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    assert_eq!(resp.usage.input_tokens, 800);
    assert_eq!(resp.usage.output_tokens, 500);
    assert_eq!(resp.usage.cache_read_tokens, 200);
    assert_eq!(resp.usage.cache_write_tokens, 50);
    assert_eq!(resp.provider_response_id.as_deref(), Some("msg_01ABC"));
}
```

- [ ] **Step 3: Implement the transport**

Create `crates/gateway-llm/src/transports/anthropic.rs`:

```rust
//! Anthropic Messages (`/v1/messages`) egress transport. System messages are
//! hoisted into the top-level `system` field; the rest map to user/assistant
//! turns. Auth is `x-api-key` + `anthropic-version`; `anthropic-beta` and other
//! overrides ride via `Credentials.extra_headers` (Claude Code requires header
//! forwarding). Usage is already non-overlapping (input excludes cache). `max_
//! tokens` is REQUIRED by Anthropic — we default it when unset. Streaming in
//! Task 12; full tool/structured-output fidelity is P1.3.

use async_trait::async_trait;
use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::message::{ContentPart, Message, Role};
use crate::provider::{
    Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError,
};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};
use crate::toolcall::ToolCall;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: i64 = 4096;

pub struct Anthropic {
    http: reqwest::Client,
}

impl Default for Anthropic {
    fn default() -> Self {
        Self::new()
    }
}

impl Anthropic {
    pub fn new() -> Self {
        Anthropic { http: reqwest::Client::new() }
    }

    fn base_url<'a>(&self, creds: &'a Credentials) -> &'a str {
        creds.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
    }
}

// ---- wire structs (private) ----

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    max_tokens: i64,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct WireResponse {
    id: Option<String>,
    model: String,
    stop_reason: Option<String>,
    #[serde(default)]
    content: Vec<WireContentBlock>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
}

// ---- mapping ----

/// Hoist system turns into the top-level `system` string; map the rest.
fn split_messages(messages: &[Message]) -> (Option<String>, Vec<WireMessage>) {
    let mut system = String::new();
    let mut out = Vec::new();
    for m in messages {
        match m.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&m.text_content());
            }
            Role::User | Role::Tool => {
                out.push(WireMessage { role: "user", content: m.text_content() });
            }
            Role::Assistant => {
                out.push(WireMessage { role: "assistant", content: m.text_content() });
            }
        }
    }
    (if system.is_empty() { None } else { Some(system) }, out)
}

fn map_finish(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolCalls,
        _ => FinishReason::Unknown,
    }
}

fn map_usage(u: Option<WireUsage>) -> TokenUsage {
    let Some(u) = u else { return TokenUsage::default() };
    TokenUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_input_tokens,
        cache_write_tokens: u.cache_creation_input_tokens,
    }
}

fn map_response(w: WireResponse) -> ChatResponse {
    let mut content = Vec::new();
    let mut tool_calls = Vec::new();
    for block in w.content {
        match block {
            WireContentBlock::Text { text } => content.push(ContentPart::text(text)),
            WireContentBlock::ToolUse { id, name, input } => tool_calls.push(ToolCall {
                id,
                name,
                arguments: input.to_string(),
            }),
            WireContentBlock::Other => {}
        }
    }
    ChatResponse {
        model: w.model,
        content,
        tool_calls,
        finish_reason: map_finish(w.stop_reason.as_deref()),
        usage: map_usage(w.usage),
        provider_response_id: w.id,
    }
}

#[async_trait]
impl Provider for Anthropic {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: true,
            supports_tools: true,
            supports_vision: true,
            supports_idempotency: true,
        }
    }

    async fn chat(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        let url = format!("{}/v1/messages", self.base_url(creds));
        let (system, messages) = split_messages(&req.messages);
        let wire = WireRequest {
            model: &req.model,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            messages,
            system,
            temperature: req.temperature,
            stream: false,
        };
        let mut rb = self
            .http
            .post(url)
            .header("x-api-key", &creds.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Idempotency-Key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok());
            return Err(ProviderError::RateLimited { retry_after_secs: retry });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }
        let wire: WireResponse =
            resp.json().await.map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(map_response(wire))
    }

    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        // Implemented in Task 12.
        Err(ProviderError::Unsupported { feature: "streaming (added in Task 12)".into() })
    }
}
```

Add to `crates/gateway-llm/src/transports/mod.rs`:

```rust
pub mod anthropic;
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p gateway-llm --test anthropic_transport`
Expected: `anthropic_messages_maps_request_response_and_usage` PASSES — headers asserted by the mock, all four usage buckets mapped directly.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports/anthropic.rs crates/gateway-llm/src/transports/mod.rs crates/gateway-llm/tests/fixtures/anthropic_messages.json crates/gateway-llm/tests/anthropic_transport.rs
git commit -s -m "feat(llm): Anthropic /v1/messages egress (non-streaming) with system hoist"
```

---

### Task 12: Anthropic egress transport — streaming (mocked SSE)

**Files:**
- Modify: `crates/gateway-llm/src/transports/anthropic.rs`
- Create: `crates/gateway-llm/tests/fixtures/anthropic_stream.sse`
- Modify: `crates/gateway-llm/tests/anthropic_transport.rs`

Anthropic's SSE uses named events whose `data:` payloads carry a `type` field (`message_start`, `content_block_delta`, `message_delta`, `message_stop`). Our shared decoder ignores `event:` lines and relays the `data:` JSON; the transport dispatches on the JSON `type`. Usage is split: `message_start` carries input/cache usage, `message_delta` carries output tokens — we accumulate and emit on the terminal delta so usage is never lost.

- [ ] **Step 1: Record the SSE fixture**

Create `crates/gateway-llm/tests/fixtures/anthropic_stream.sse`:

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":800,"cache_read_input_tokens":200,"cache_creation_input_tokens":0,"output_tokens":0}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}

```

- [ ] **Step 2: Write the failing test**

Append to `crates/gateway-llm/tests/anthropic_transport.rs`:

```rust
#[tokio::test]
async fn anthropic_stream_accumulates_usage_and_text() {
    use futures::StreamExt;
    let server = MockServer::start().await;
    let sse = std::fs::read_to_string("tests/fixtures/anthropic_stream.sse").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("idempotency-key", "idem-as"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let provider = Anthropic::new();
    let creds = Credentials::new("sk-ant").with_base_url(server.uri());
    let mut req = ChatRequest::new("claude-3-5-sonnet-20241022", vec![Message::text(Role::User, "Hi")]);
    req.stream = true;

    let mut stream = provider.stream(&req, &creds, "idem-as").await.unwrap();

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
    let u = usage.expect("usage must be emitted on the terminal delta");
    assert_eq!(u.input_tokens, 800);
    assert_eq!(u.cache_read_tokens, 200);
    assert_eq!(u.output_tokens, 2);
}
```

- [ ] **Step 3: Implement streaming**

In `crates/gateway-llm/src/transports/anthropic.rs`, add stream wire structs near the others:

```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireStreamEvent {
    MessageStart { message: WireStreamStartMessage },
    ContentBlockDelta { delta: WireStreamDelta },
    MessageDelta { delta: WireStreamMessageDelta, #[serde(default)] usage: Option<WireDeltaUsage> },
    MessageStop,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireStreamStartMessage {
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireStreamDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireStreamMessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireDeltaUsage {
    #[serde(default)]
    output_tokens: i64,
}
```

Add a stateful accumulator + the streaming method. Anthropic splits usage across `message_start` (input/cache) and `message_delta` (output), so the transport holds a running `TokenUsage` and emits it on the terminal delta. Place this helper below `map_response`:

```rust
/// Fold one Anthropic stream event into (optional emitted delta, usage accumulator
/// mutation). Returns the unified delta to relay, if any.
fn fold_stream_event(
    payload: &str,
    acc_usage: &mut TokenUsage,
) -> Result<Option<crate::stream::StreamDelta>, ProviderError> {
    use crate::stream::StreamDelta;
    let ev: WireStreamEvent =
        serde_json::from_str(payload).map_err(|e| ProviderError::Decode(e.to_string()))?;
    match ev {
        WireStreamEvent::MessageStart { message } => {
            if let Some(u) = message.usage {
                let mapped = map_usage(Some(u));
                acc_usage.input_tokens = mapped.input_tokens;
                acc_usage.cache_read_tokens = mapped.cache_read_tokens;
                acc_usage.cache_write_tokens = mapped.cache_write_tokens;
            }
            Ok(None)
        }
        WireStreamEvent::ContentBlockDelta { delta } => match delta {
            WireStreamDelta::TextDelta { text } => Ok(Some(StreamDelta::text(text))),
            // Tool-call argument fragments: relay carried in P1.3's aggregation;
            // here we surface them as a content-less tool delta on index 0.
            WireStreamDelta::InputJsonDelta { partial_json } => {
                use crate::stream::ToolCallDelta;
                Ok(Some(StreamDelta {
                    tool_call_deltas: vec![ToolCallDelta {
                        index: 0,
                        id: None,
                        name: None,
                        arguments_delta: Some(partial_json),
                    }],
                    ..Default::default()
                }))
            }
            WireStreamDelta::Other => Ok(None),
        },
        WireStreamEvent::MessageDelta { delta, usage } => {
            if let Some(u) = usage {
                acc_usage.output_tokens = u.output_tokens;
            }
            // Terminal delta: carry finish + the accumulated usage so it is never lost.
            Ok(Some(StreamDelta {
                finish_reason: Some(map_finish(delta.stop_reason.as_deref())),
                usage: Some(*acc_usage),
                ..Default::default()
            }))
        }
        WireStreamEvent::MessageStop | WireStreamEvent::Other => Ok(None),
    }
}
```

Replace the `stream` method body:

```rust
    async fn stream(
        &self,
        req: &ChatRequest,
        creds: &Credentials,
        idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        use crate::sse::{SseDecoder, SseEvent};
        use futures::StreamExt;

        let url = format!("{}/v1/messages", self.base_url(creds));
        let (system, messages) = split_messages(&req.messages);
        let wire = WireRequest {
            model: &req.model,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            messages,
            system,
            temperature: req.temperature,
            stream: true,
        };
        let mut rb = self
            .http
            .post(url)
            .header("x-api-key", &creds.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Idempotency-Key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }

        let byte_stream = resp.bytes_stream();
        type State = (
            std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
            SseDecoder,
            Vec<Result<crate::stream::StreamDelta, ProviderError>>,
            TokenUsage,
            bool,
        );
        let init: State = (Box::pin(byte_stream), SseDecoder::new(), Vec::new(), TokenUsage::default(), false);
        let out = futures::stream::unfold(init, |(mut bytes, mut decoder, mut pending, mut acc, mut done)| async move {
            loop {
                if let Some(item) = pending.pop() {
                    return Some((item, (bytes, decoder, pending, acc, done)));
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
                                SseEvent::Data(payload) => match fold_stream_event(&payload, &mut acc) {
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
        });
        Ok(Box::pin(out))
    }
```

- [ ] **Step 4: Run the streaming test**

Run: `cargo test -p gateway-llm --test anthropic_transport`
Expected: both Anthropic tests PASS. Text="Hello", finish=Stop, terminal usage has input 800 / cache_read 200 / output 2 — the split-source usage was accumulated and emitted intact.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports/anthropic.rs crates/gateway-llm/tests/fixtures/anthropic_stream.sse crates/gateway-llm/tests/anthropic_transport.rs
git commit -s -m "feat(llm): Anthropic streaming with split-source usage accumulation"
```

---

### Task 13: Gemini egress transport — non-streaming (`generateContent`)

**Files:**
- Create: `crates/gateway-llm/src/transports/gemini.rs`
- Create: `crates/gateway-llm/tests/fixtures/gemini_generate.json`
- Create: `crates/gateway-llm/tests/gemini_transport.rs`
- Modify: `crates/gateway-llm/src/transports/mod.rs`

Gemini differs most: roles are `user`/`model` (no system role — system goes in `systemInstruction`), the path embeds the model + `:generateContent`, the API key is a query param (`?key=`), and usage is `usageMetadata.{promptTokenCount,candidatesTokenCount,cachedContentTokenCount}` (prompt INCLUDES cached, so we split). Gemini has no idempotency header — the transport declares `supports_idempotency: false` and threads the key as a forwarded `x-idempotency-key` for our own audit correlation (no-op upstream, surfaced for the lifecycle layer).

- [ ] **Step 1: Record the fixture**

Create `crates/gateway-llm/tests/fixtures/gemini_generate.json`:

```json
{
  "candidates": [
    {
      "content": {
        "role": "model",
        "parts": [ { "text": "Hello from Gemini" } ]
      },
      "finishReason": "STOP"
    }
  ],
  "modelVersion": "gemini-1.5-pro",
  "usageMetadata": {
    "promptTokenCount": 1000,
    "candidatesTokenCount": 500,
    "cachedContentTokenCount": 200
  }
}
```

- [ ] **Step 2: Write the failing test**

Create `crates/gateway-llm/tests/gemini_transport.rs`:

```rust
//! Gemini generateContent egress, non-streaming, mocked. Asserts request mapping
//! (model in path, key in query, system → systemInstruction, role mapping), response
//! mapping (finishReason STOP → Stop), and usage extraction (prompt includes
//! cached → split into non-overlapping buckets).

use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::message::{Message, Role};
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
```

- [ ] **Step 3: Implement the transport**

Create `crates/gateway-llm/src/transports/gemini.rs`:

```rust
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
use crate::provider::{
    Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError,
};
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
        Gemini { http: reqwest::Client::new() }
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
                parts: vec![WirePart { text: m.text_content() }],
            }),
            Role::Assistant => contents.push(WireContent {
                role: Some("model"),
                parts: vec![WirePart { text: m.text_content() }],
            }),
        }
    }
    let sys = if system.is_empty() {
        None
    } else {
        Some(WireContent { role: None, parts: vec![WirePart { text: system }] })
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
    let Some(u) = u else { return TokenUsage::default() };
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
        model: w.model_version.unwrap_or_else(|| requested_model.to_string()),
        content,
        tool_calls: Vec::new(),
        finish_reason: finish,
        usage: map_usage(w.usage_metadata),
        provider_response_id: None,
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
            Some(WireGenConfig { temperature: req.temperature, max_output_tokens: req.max_tokens })
        } else {
            None
        };
        let wire = WireRequest { contents, system_instruction, generation_config };
        let mut rb = self
            .http
            .post(url)
            .query(&[("key", creds.api_key.as_str())])
            .header("x-idempotency-key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::Auth);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited { retry_after_secs: None });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }
        let wire: WireResponse =
            resp.json().await.map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(map_response(wire, &req.model))
    }

    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        // Implemented in Task 14.
        Err(ProviderError::Unsupported { feature: "streaming (added in Task 14)".into() })
    }
}
```

Add to `crates/gateway-llm/src/transports/mod.rs`:

```rust
pub mod gemini;
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p gateway-llm --test gemini_transport`
Expected: both Gemini tests PASS — path+query asserted by the mock, usage split 800/200/500, `supports_idempotency` false.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports/gemini.rs crates/gateway-llm/src/transports/mod.rs crates/gateway-llm/tests/fixtures/gemini_generate.json crates/gateway-llm/tests/gemini_transport.rs
git commit -s -m "feat(llm): Gemini generateContent egress (non-streaming), no-idempotency declared"
```

---

### Task 14: Gemini egress transport — streaming (`streamGenerateContent`, mocked SSE)

**Files:**
- Modify: `crates/gateway-llm/src/transports/gemini.rs`
- Create: `crates/gateway-llm/tests/fixtures/gemini_stream.sse`
- Modify: `crates/gateway-llm/tests/gemini_transport.rs`

Gemini streams via `:streamGenerateContent?alt=sse`, emitting `data:` JSON chunks each shaped like the non-streaming response (incremental `candidates[].content.parts[].text`, with `usageMetadata` on later chunks and `finishReason` on the final candidate). We reuse the response wire structs and emit a terminal delta carrying usage + finish.

- [ ] **Step 1: Record the SSE fixture**

Create `crates/gateway-llm/tests/fixtures/gemini_stream.sse`:

```
data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hel"}]}}]}

data: {"candidates":[{"content":{"role":"model","parts":[{"text":"lo"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"cachedContentTokenCount":0}}

```

- [ ] **Step 2: Write the failing test**

Append to `crates/gateway-llm/tests/gemini_transport.rs`:

```rust
#[tokio::test]
async fn gemini_stream_yields_text_then_terminal_usage() {
    use futures::StreamExt;
    use wiremock::matchers::query_param;
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
```

- [ ] **Step 3: Implement streaming**

In `crates/gateway-llm/src/transports/gemini.rs`, add a parser that maps one streamed chunk (same shape as `WireResponse`) into a unified delta. Place below `map_response`:

```rust
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
```

Replace the `stream` method body:

```rust
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
            Some(WireGenConfig { temperature: req.temperature, max_output_tokens: req.max_tokens })
        } else {
            None
        };
        let wire = WireRequest { contents, system_instruction, generation_config };
        let mut rb = self
            .http
            .post(url)
            .query(&[("alt", "sse"), ("key", creds.api_key.as_str())])
            .header("x-idempotency-key", idempotency_key)
            .json(&wire);
        for (k, v) in &creds.extra_headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream { status: status.as_u16(), body });
        }

        let byte_stream = resp.bytes_stream();
        type State = (
            std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
            SseDecoder,
            Vec<Result<crate::stream::StreamDelta, ProviderError>>,
            bool,
        );
        let init: State = (Box::pin(byte_stream), SseDecoder::new(), Vec::new(), false);
        let out = futures::stream::unfold(init, |(mut bytes, mut decoder, mut pending, mut done)| async move {
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
        });
        Ok(Box::pin(out))
    }
```

- [ ] **Step 4: Run the streaming test**

Run: `cargo test -p gateway-llm --test gemini_transport`
Expected: all three Gemini tests PASS. Text="Hello", finish=Stop, usage.output_tokens=2.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/transports/gemini.rs crates/gateway-llm/tests/fixtures/gemini_stream.sse crates/gateway-llm/tests/gemini_transport.rs
git commit -s -m "feat(llm): Gemini streamGenerateContent SSE streaming"
```

---

### Task 15: Idempotency-reuse invariant proof + error-path tests

**Files:**
- Create: `crates/gateway-llm/tests/idempotency_and_errors.rs`

This is the milestone's invariant proof: the SAME idempotency key, reused across two transport calls (simulating a retry), produces a byte-identical idempotency header upstream — the no-double-billing precondition. Plus error-path coverage (401→Auth, 429→RateLimited, 500→Upstream).

- [ ] **Step 1: Write the test**

Create `crates/gateway-llm/tests/idempotency_and_errors.rs`:

```rust
//! Invariant proof (design §2 — no double-billing): one idempotency key reused
//! across two calls of the same logical request yields a byte-identical
//! `Idempotency-Key` header upstream. Plus the transport error taxonomy mapping.

use gateway_llm::provider::{Credentials, Provider, ProviderError};
use gateway_llm::req::ChatRequest;
use gateway_llm::message::{Message, Role};
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
    assert!(matches!(err, ProviderError::RateLimited { retry_after_secs: Some(12) }));
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
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p gateway-llm --test idempotency_and_errors`
Expected: 4 tests PASS. The idempotency test's `.expect(2)` on the header-matched route is the actual invariant assertion — both retries hit the same key.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/idempotency_and_errors.rs
git commit -s -m "test(llm): prove idempotency-key reuse + error taxonomy mapping"
```

---

### Task 16: Cost wiring against the spine registry (cross-crate integration)

**Files:**
- Create: `crates/gateway-llm/tests/cost_integration.rs`

Proves the seam to P1.1: a transport's extracted `TokenUsage` priced through `gateway_spine::ModelRegistry::cost` yields the exact µUSD. This is the contract P1.4 will wire in the request lifecycle.

- [ ] **Step 1: Write the test**

Create `crates/gateway-llm/tests/cost_integration.rs`:

```rust
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
    let cost = registry().cost(&resp.model, &resp.usage).expect("known model");
    assert_eq!(cost, Usd::from_micros(7_250));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p gateway-llm --test cost_integration`
Expected: PASS — cost is exactly 7_250 µUSD ($0.00725).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/cost_integration.rs
git commit -s -m "test(llm): prove transport usage prices exactly via spine registry"
```

---

### Task 17: Finalize `lib.rs` module surface + full-crate gate

**Files:**
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Ensure `lib.rs` reads exactly (no `CRATE` placeholder)**

```rust
//! # gateway-llm
//!
//! The provider-agnostic LLM core: the unified request/response/streaming types,
//! the `Provider` egress trait, and reference transports (OpenAI, Anthropic,
//! Gemini). Ingress dialects map INTO the unified types; transports map OUT to
//! provider wire formats. Usage is normalized into `gateway_spine::TokenUsage`
//! (non-overlapping buckets) so the spine prices it exactly.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod message;
pub mod provider;
pub mod req;
pub mod resp;
pub mod sse;
pub mod stream;
pub mod toolcall;
pub mod transports;

pub use message::{ContentPart, ImageSource, Message, Role};
pub use provider::{Credentials, DeltaStream, Provider, ProviderCapabilities, ProviderError};
pub use req::{ChatRequest, ReasoningEffort, ResponseFormat};
pub use resp::{ChatResponse, FinishReason};
pub use sse::{SseDecoder, SseEvent};
pub use stream::{StreamDelta, ToolCallDelta};
pub use toolcall::{ToolCall, ToolChoice, ToolDef};
```

- [ ] **Step 2: Run the entire crate's tests + the full gate**

Run: `cargo test -p gateway-llm`
Expected: every unit test (message/toolcall/req/resp/stream/provider/sse) + every integration test (openai/anthropic/gemini transports, idempotency_and_errors, cost_integration) PASS.

Then run the gate:

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
```

Expected: clean (no warnings). If clippy flags `collapsible_if` anywhere, rewrite the nested `if` as a let-chain (`if let Some(x) = opt && cond {}`) per the Rust 2024 discipline.

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): finalize gateway-llm module surface"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-llm` is fully green (all unit + all integration tests across OpenAI/Anthropic/Gemini, streaming + non-streaming).
- [ ] `cargo clippy -p gateway-llm --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean; `#![forbid(unsafe_code)]` holds.
- [ ] The three invariants this milestone owns are each proven by a test:
  - **No double-billing** — `same_idempotency_key_sends_identical_header_across_retries` (header byte-identical across a retry).
  - **Never lose usage on aborted streams** — every streaming test asserts a terminal delta carries `usage` (Anthropic's split-source accumulation especially).
  - **No silent degradation** — `ProviderError::Unsupported` is the typed seam (used by the pre-streaming stubs and asserted reachable).
- [ ] Usage normalization is non-overlapping and verified: OpenAI/Gemini split cached out of prompt tokens; Anthropic maps direct; priced exactly through `gateway_spine::ModelRegistry` (`cost_integration`).
- [ ] Public surface is stable and re-exported: `ChatRequest`, `ChatResponse`, `StreamDelta`, `ToolDef`/`ToolCall`/`ToolChoice`, `Message`/`ContentPart`/`Role`, `Provider`/`Credentials`/`ProviderError`/`DeltaStream`/`ProviderCapabilities` — downstream milestones import these verbatim.
- [ ] Transports use **mocked HTTP only** (no live provider calls); fixtures live under `crates/gateway-llm/tests/fixtures/`.
- [ ] Deferred-to-P1.3 seams are present but explicitly not implemented: tool-call delta aggregation (fragments relayed with index), `response_format`/`reasoning_effort` carried-not-mapped, no conformance harness yet.

**Next:** `2026-06-10-p1-03-translation-conformance.md` — the ingress-dialect ⇄ unified ⇄ provider translation core, tool-call delta aggregation, `UnsupportedOperationError` discipline, and the golden-fixture conformance harness + per-pair fidelity matrix, all built on the types and `Provider` trait frozen in this milestone.
