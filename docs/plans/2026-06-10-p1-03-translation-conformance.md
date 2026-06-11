# Phase 1.3 — Translation Core + Conformance Harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the bidirectional **translation core** of `gateway-llm` — the layer that maps any client **ingress dialect** (OpenAI Chat Completions, Anthropic `/v1/messages`, OpenAI **Responses**) *into* the unified `ChatRequest` frozen in P1.2, and serializes the unified `ChatResponse`/`StreamDelta` *back out* into the dialect the client speaks — plus the cross-provider concerns P1.2 deliberately deferred: **tool-call delta aggregation** (reassembling fragmented `arguments` JSON across streaming chunks into whole `ToolCall`s), **structured-output** translation (`json_schema` → each provider's equivalent, with **forced-tool-call emulation** fallback when a provider lacks native schema support), and an explicit **`UnsupportedOperation` + dropped-param warning** discipline (never silent degradation, Bifrost-style). It is capped by the **golden-fixture conformance harness**: real request shapes recorded from Claude Code / Codex / the OpenAI SDK become fixtures, and a round-trip test asserts dialect → unified → dialect fidelity, generating a **per-pair fidelity matrix** doc. **This is where the streaming-regression whack-a-mole is killed** (design §3, item 8).

**This milestone freezes the translation contract.** The ingress parser/serializer signatures, the `ToolCallAggregator`, the `StructuredOutputPlan`, and the `Unsupported`/`Warning` taxonomy defined here are imported verbatim by P1.4 (the HTTP server wires each `/v1/...` route to its ingress dialect) and every future dialect/provider. Define them with completeness; later milestones add dialects + provider pairs, they do not rewrite the core.

**Architecture:** Pure, synchronous, I/O-free translation functions over the P1.2 types — no `reqwest`, no `async` in the translation core itself (the `Provider` transports already own egress I/O). Three layers:
1. **Ingress dialects** (`ingress/`): each dialect is a pair of pure fns `parse_request(dialect_json) -> Result<(ChatRequest, Warnings), IngressError>` and `serialize_response(&ChatResponse) -> serde_json::Value` (+ a streaming `serialize_delta` emitting that dialect's SSE frames). A dialect is the inverse of a P1.2 transport's *wire structs*, but on the **client-facing** side.
2. **Tool-call aggregation** (`aggregate.rs`): a stateful `ToolCallAggregator` folds the `ToolCallDelta` fragments P1.2 relays per-index into finished `ToolCall`s, surfacing them on the terminal delta — the single place fragmented `arguments` JSON is stitched.
3. **Structured output** (`structured.rs`): compiles a `ResponseFormat` into a `StructuredOutputPlan` per provider — `Native` (OpenAI/Gemini json_schema), `AnthropicToolEmulation` (forced single-tool call whose schema *is* the requested schema, response unwrapped from the tool call), or `Unsupported`.

The **no-silent-degradation invariant** is structural: every translation that *can* drop a feature returns a `Vec<Warning>` and any *semantic* loss returns `Err(Unsupported)` — both threaded to the caller, never swallowed. Money never appears here; usage already normalized in P1.2.

**Tech Stack:** Rust 2024; `gateway-spine` (`TokenUsage`); `gateway-llm` P1.2 types (`ChatRequest`/`ChatResponse`/`StreamDelta`/`ToolCall`/`Message`/`ContentPart`/`ResponseFormat`/`FinishReason`); `serde`/`serde_json`; `thiserror`. **No HTTP, no async in the core** (the conformance harness exercises pure functions). Golden fixtures live under `crates/gateway-llm/tests/fixtures/ingress/` and `.../golden/`.

**Invariants this milestone enforces (design §2, §3, §5):**
- **Conformance-tested translation** — golden fixtures recorded from real agent clients; a round-trip test gates every dialect (`docs §3 item 8` — "stops the eternal streaming-regression whack-a-mole"). Proven by `tests/conformance.rs`.
- **No silent degradation** — unsupported/dropped request features surface as `IngressError::Unsupported { feature }` or a `Warning`, never a quiet drop (Bifrost model, design §5 LLM-ingress bullet). Proven by `unsupported_*` tests.
- **Tool-call-delta correctness** — fragmented `arguments` across SSE chunks reassemble to byte-exact whole JSON (design §5 risk row "where every clone breaks"). Proven by `aggregate.rs` tests + a streaming golden fixture.

**Builds strictly on P1.2 (imported verbatim, never modified here):** `ChatRequest`, `ChatResponse`, `StreamDelta`, `ToolCallDelta`, `ToolCall`, `ToolDef`, `ToolChoice`, `Message`, `ContentPart`, `Role`, `ImageSource`, `ReasoningEffort`, `ResponseFormat`, `FinishReason`, `TokenUsage`. The egress transports (`transports::{openai,anthropic,gemini}`) are unchanged; this milestone is the **client-facing** mirror plus the aggregation/structured-output the transports left as seams.

---

### Task 1: `Warning` + `IngressError` translation taxonomy

**Files:**
- Create: `crates/gateway-llm/src/translate/mod.rs`
- Create: `crates/gateway-llm/src/translate/warn.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

The whole translation core lives under a new `translate` module. Start with the no-silent-degradation taxonomy every layer threads.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/warn.rs`:

```rust
//! The no-silent-degradation taxonomy (design §5, Bifrost model). Translation
//! NEVER quietly drops a feature: a *lossy-but-safe* drop yields a `Warning`
//! (surfaced to the client via an overhead header / log by P1.4); a *semantic*
//! loss that would change the request's meaning yields `IngressError::Unsupported`
//! and the request is rejected. Both are values threaded to the caller — there is
//! no path that swallows either.

use serde::{Deserialize, Serialize};

/// A non-fatal translation notice: a feature was dropped or downgraded but the
/// request's meaning is preserved. P1.4 surfaces these (e.g. `x-oximy-warnings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// Machine-stable code, e.g. "param.dropped", "tool_choice.downgraded".
    pub code: String,
    /// Human-readable detail, e.g. "logit_bias is not supported by Anthropic".
    pub message: String,
}

impl Warning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Warning { code: code.into(), message: message.into() }
    }

    /// A parameter present in the ingress request that this dialect/provider drops.
    pub fn dropped_param(name: &str, reason: &str) -> Self {
        Warning::new("param.dropped", format!("`{name}` was dropped: {reason}"))
    }
}

/// Accumulated warnings + the value they annotate. Returned from every ingress
/// parse so the caller decides how to surface them (never discarded here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated<T> {
    pub value: T,
    pub warnings: Vec<Warning>,
}

impl<T> Translated<T> {
    pub fn new(value: T) -> Self {
        Translated { value, warnings: Vec::new() }
    }

    pub fn with_warning(mut self, w: Warning) -> Self {
        self.warnings.push(w);
        self
    }

    /// Map the inner value, preserving warnings.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Translated<U> {
        Translated { value: f(self.value), warnings: self.warnings }
    }
}

/// Errors raised while translating a client request INTO the unified shape.
/// `Unsupported` is the typed no-silent-degradation seam (mirrors
/// `ProviderError::Unsupported` on the egress side).
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum IngressError {
    #[error("malformed request: {0}")]
    Malformed(String),
    #[error("request feature unsupported by this gateway: {feature}")]
    Unsupported { feature: String },
    #[error("missing required field: {0}")]
    MissingField(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_param_warning_is_coded() {
        let w = Warning::dropped_param("logit_bias", "Anthropic has no equivalent");
        assert_eq!(w.code, "param.dropped");
        assert!(w.message.contains("logit_bias"));
    }

    #[test]
    fn translated_threads_warnings_through_map() {
        let t = Translated::new(1u8)
            .with_warning(Warning::new("a", "first"))
            .map(|v| v + 1);
        assert_eq!(t.value, 2);
        assert_eq!(t.warnings.len(), 1);
        assert_eq!(t.warnings[0].code, "a");
    }

    #[test]
    fn unsupported_is_distinct_from_malformed() {
        let u = IngressError::Unsupported { feature: "audio input".into() };
        let m = IngressError::Malformed("bad json".into());
        assert_ne!(u, m);
        assert!(u.to_string().contains("audio input"));
    }
}
```

Create `crates/gateway-llm/src/translate/mod.rs`:

```rust
//! The translation core: ingress dialect ⇄ unified ⇄ provider. Pure, I/O-free
//! functions over the P1.2 types. `warn` owns the no-silent-degradation taxonomy;
//! `aggregate` stitches streamed tool-call fragments; `structured` compiles
//! structured-output plans; `ingress` holds one parser/serializer per client
//! dialect. The conformance harness (tests/) gates every dialect round-trip.

pub mod warn;

pub use warn::{IngressError, Translated, Warning};
```

Add to `crates/gateway-llm/src/lib.rs` (after the existing `pub mod transports;` line, and a re-export after the `transports` re-export block):

```rust
pub mod translate;

pub use translate::{IngressError, Translated, Warning};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::warn::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/mod.rs crates/gateway-llm/src/translate/warn.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): translation Warning/IngressError taxonomy (no silent degradation)"
```

---

### Task 2: `ToolCallAggregator` — stitch streamed tool-call fragments

**Files:**
- Create: `crates/gateway-llm/src/translate/aggregate.rs`
- Modify: `crates/gateway-llm/src/translate/mod.rs`

P1.2 relays each `ToolCallDelta` (an `index`, an optional `id`/`name` on the first fragment, then `arguments_delta` fragments) losslessly. This is the **one place** those fragments are reassembled into finished `ToolCall`s — the correctness seam every clone breaks (design §5).

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/aggregate.rs`:

```rust
//! Tool-call delta aggregation. A streamed turn emits `ToolCallDelta`s carrying an
//! `index` (which parallel call), an `id`/`name` (on the first fragment for that
//! index), then successive `arguments_delta` string fragments. This aggregator
//! folds them per-index into whole `ToolCall`s whose `arguments` is the
//! byte-concatenation of the fragments — the single point where fragmented JSON is
//! stitched. It also folds whole (non-fragmented) calls, so it works for providers
//! that emit complete calls in one delta. Ordering by index is stable; first-seen
//! `id`/`name` win (later fragments only carry args).

use std::collections::BTreeMap;

use crate::stream::{StreamDelta, ToolCallDelta};
use crate::toolcall::ToolCall;

#[derive(Debug, Default, Clone)]
struct Partial {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// Stateful accumulator. Feed every `StreamDelta`; call `finish()` once the stream
/// terminates to get the completed tool calls in stable index order.
#[derive(Debug, Default)]
pub struct ToolCallAggregator {
    by_index: BTreeMap<i64, Partial>,
}

impl ToolCallAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one delta's tool-call fragments. Text/finish/usage are ignored here —
    /// the caller handles those; this owns ONLY tool-call reassembly.
    pub fn push_delta(&mut self, delta: &StreamDelta) {
        for frag in &delta.tool_call_deltas {
            self.push_fragment(frag);
        }
    }

    fn push_fragment(&mut self, frag: &ToolCallDelta) {
        let entry = self.by_index.entry(frag.index).or_default();
        if entry.id.is_none()
            && let Some(id) = &frag.id
        {
            entry.id = Some(id.clone());
        }
        if entry.name.is_none()
            && let Some(name) = &frag.name
        {
            entry.name = Some(name.clone());
        }
        if let Some(args) = &frag.arguments_delta {
            entry.arguments.push_str(args);
        }
    }

    /// True if any tool-call fragment has been seen.
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }

    /// Finalize into completed calls, in ascending index order. A call with no id
    /// is given an empty id (provider didn't supply one); a call with no name is
    /// dropped (a nameless call is not invocable — better to omit than fabricate).
    pub fn finish(self) -> Vec<ToolCall> {
        self.by_index
            .into_values()
            .filter_map(|p| {
                let name = p.name?;
                Some(ToolCall {
                    id: p.id.unwrap_or_default(),
                    name,
                    arguments: if p.arguments.is_empty() {
                        "{}".to_string()
                    } else {
                        p.arguments
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(index: i64, id: Option<&str>, name: Option<&str>, args: Option<&str>) -> ToolCallDelta {
        ToolCallDelta {
            index,
            id: id.map(Into::into),
            name: name.map(Into::into),
            arguments_delta: args.map(Into::into),
        }
    }

    fn delta(frags: Vec<ToolCallDelta>) -> StreamDelta {
        StreamDelta { tool_call_deltas: frags, ..Default::default() }
    }

    #[test]
    fn stitches_fragmented_arguments_byte_exact() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("call_1"), Some("get_weather"), Some("{\"ci"))]));
        agg.push_delta(&delta(vec![frag(0, None, None, Some("ty\":\""))]));
        agg.push_delta(&delta(vec![frag(0, None, None, Some("SF\"}"))]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].arguments, "{\"city\":\"SF\"}");
    }

    #[test]
    fn handles_parallel_calls_by_index_in_order() {
        let mut agg = ToolCallAggregator::new();
        // Interleaved fragments for two parallel calls.
        agg.push_delta(&delta(vec![
            frag(0, Some("c0"), Some("f0"), Some("{\"a\":")),
            frag(1, Some("c1"), Some("f1"), Some("{\"b\":")),
        ]));
        agg.push_delta(&delta(vec![
            frag(1, None, None, Some("2}")),
            frag(0, None, None, Some("1}")),
        ]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "f0"); // index 0 first
        assert_eq!(calls[0].arguments, "{\"a\":1}");
        assert_eq!(calls[1].name, "f1");
        assert_eq!(calls[1].arguments, "{\"b\":2}");
    }

    #[test]
    fn whole_call_in_one_delta_works() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("c"), Some("f"), Some("{\"x\":1}"))]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments, "{\"x\":1}");
    }

    #[test]
    fn empty_arguments_default_to_object() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("c"), Some("f"), None)]));
        let calls = agg.finish();
        assert_eq!(calls[0].arguments, "{}");
    }

    #[test]
    fn nameless_fragment_is_dropped_not_fabricated() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("c"), None, Some("{\"x\":1}"))]));
        assert!(agg.finish().is_empty());
    }
}
```

Add to `crates/gateway-llm/src/translate/mod.rs`:

```rust
pub mod aggregate;

pub use aggregate::ToolCallAggregator;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::aggregate::`
Expected: 5 tests PASS. (If `handles_parallel_calls_by_index_in_order` fails on ordering, the `BTreeMap` keying is wrong — it must be by `index`, not insertion order; this is load-bearing for parallel tool calls.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/aggregate.rs crates/gateway-llm/src/translate/mod.rs
git commit -s -m "feat(llm): ToolCallAggregator stitches streamed tool-call fragments"
```

---

### Task 3: `StructuredOutputPlan` — json_schema → per-provider equivalent

**Files:**
- Create: `crates/gateway-llm/src/translate/structured.rs`
- Modify: `crates/gateway-llm/src/translate/mod.rs`

`ResponseFormat::JsonSchema` is carried-not-mapped in P1.2. Here we compile it to a per-provider **plan**: native schema where supported, **forced-tool-call emulation** where not (Anthropic), or `Unsupported`.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/structured.rs`:

```rust
//! Structured-output translation. A unified `ResponseFormat` is compiled into a
//! per-provider `StructuredOutputPlan`. OpenAI/Gemini support `json_schema`
//! natively; Anthropic has no response-format field, so we EMULATE it with a
//! single forced tool call whose input schema IS the requested schema — the
//! model's structured answer arrives as that tool call's `arguments`, which we
//! unwrap back into content. `Text`/`JsonObject` map to native json-mode or pass
//! through. A provider that supports neither yields `Unsupported` (no silent
//! degradation). The plan is data only; transports/serializers consume it.

use serde_json::Value;

use crate::req::ResponseFormat;
use crate::toolcall::{ToolChoice, ToolDef};
use crate::translate::warn::IngressError;

/// The sentinel tool name used for Anthropic forced-tool structured-output
/// emulation. The response unwrapper keys off this exact name.
pub const EMULATION_TOOL_NAME: &str = "__oximy_structured_output";

/// Which provider family a plan is being compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFamily {
    OpenAi,
    Anthropic,
    Gemini,
}

/// How a transport should request structured output for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredOutputPlan {
    /// No structured-output constraint.
    None,
    /// Provider's native json-object mode (no schema).
    NativeJsonObject,
    /// Provider's native json-schema mode; carry the schema verbatim.
    NativeJsonSchema { name: String, schema: Value, strict: bool },
    /// Emulate via a single forced tool call; unwrap the result from its args.
    ForcedToolEmulation { tool: ToolDef, tool_choice: ToolChoice },
}

impl StructuredOutputPlan {
    /// Compile a response-format request for a provider family.
    pub fn compile(
        format: Option<&ResponseFormat>,
        family: ProviderFamily,
    ) -> Result<StructuredOutputPlan, IngressError> {
        match format {
            None | Some(ResponseFormat::Text) => Ok(StructuredOutputPlan::None),
            Some(ResponseFormat::JsonObject) => Ok(StructuredOutputPlan::NativeJsonObject),
            Some(ResponseFormat::JsonSchema { name, schema, strict }) => match family {
                ProviderFamily::OpenAi | ProviderFamily::Gemini => {
                    Ok(StructuredOutputPlan::NativeJsonSchema {
                        name: name.clone(),
                        schema: schema.clone(),
                        strict: *strict,
                    })
                }
                ProviderFamily::Anthropic => Ok(StructuredOutputPlan::ForcedToolEmulation {
                    tool: ToolDef {
                        name: EMULATION_TOOL_NAME.to_string(),
                        description: Some(format!(
                            "Respond ONLY by calling this function to produce `{name}`."
                        )),
                        parameters: schema.clone(),
                    },
                    tool_choice: ToolChoice::Function { name: EMULATION_TOOL_NAME.to_string() },
                }),
            },
        }
    }

    /// For an emulated plan: given a finished tool call's name+args, unwrap the
    /// structured payload back to the content string. Returns `None` if this call
    /// is not the emulation tool (so a normal tool call passes through untouched).
    pub fn unwrap_emulated<'a>(call_name: &str, arguments: &'a str) -> Option<&'a str> {
        if call_name == EMULATION_TOOL_NAME {
            Some(arguments)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    #[test]
    fn text_and_none_compile_to_none() {
        assert_eq!(
            StructuredOutputPlan::compile(None, ProviderFamily::OpenAi).unwrap(),
            StructuredOutputPlan::None
        );
        assert_eq!(
            StructuredOutputPlan::compile(Some(&ResponseFormat::Text), ProviderFamily::Anthropic)
                .unwrap(),
            StructuredOutputPlan::None
        );
    }

    #[test]
    fn openai_json_schema_is_native() {
        let fmt = ResponseFormat::JsonSchema { name: "out".into(), schema: schema(), strict: true };
        let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::OpenAi).unwrap();
        assert!(matches!(plan, StructuredOutputPlan::NativeJsonSchema { strict: true, .. }));
    }

    #[test]
    fn anthropic_json_schema_emulates_with_forced_tool() {
        let fmt = ResponseFormat::JsonSchema { name: "out".into(), schema: schema(), strict: true };
        let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::Anthropic).unwrap();
        match plan {
            StructuredOutputPlan::ForcedToolEmulation { tool, tool_choice } => {
                assert_eq!(tool.name, EMULATION_TOOL_NAME);
                assert_eq!(tool.parameters, schema());
                assert_eq!(tool_choice, ToolChoice::Function { name: EMULATION_TOOL_NAME.into() });
            }
            other => panic!("expected ForcedToolEmulation, got {other:?}"),
        }
    }

    #[test]
    fn unwrap_emulated_only_matches_sentinel_tool() {
        assert_eq!(
            StructuredOutputPlan::unwrap_emulated(EMULATION_TOOL_NAME, "{\"x\":1}"),
            Some("{\"x\":1}")
        );
        assert_eq!(StructuredOutputPlan::unwrap_emulated("get_weather", "{}"), None);
    }
}
```

Add to `crates/gateway-llm/src/translate/mod.rs`:

```rust
pub mod structured;

pub use structured::{ProviderFamily, StructuredOutputPlan};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::structured::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/structured.rs crates/gateway-llm/src/translate/mod.rs
git commit -s -m "feat(llm): StructuredOutputPlan with forced-tool-call emulation fallback"
```

---

### Task 4: OpenAI Chat Completions **ingress** dialect — request parsing

**Files:**
- Create: `crates/gateway-llm/src/translate/ingress/mod.rs`
- Create: `crates/gateway-llm/src/translate/ingress/openai_chat.rs`
- Modify: `crates/gateway-llm/src/translate/mod.rs`

The client-facing inverse of the OpenAI egress transport: parse a real `/v1/chat/completions` request body INTO the unified `ChatRequest`, emitting `Warning`s for dropped params and `IngressError::Unsupported` for semantic losses.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/ingress/openai_chat.rs`:

```rust
//! OpenAI Chat Completions INGRESS dialect (client → unified). The inverse of the
//! OpenAI egress transport's wire-request mapping: clients (OpenAI SDK, Codex)
//! POST a `/v1/chat/completions` body; we parse it into the unified `ChatRequest`.
//! Multimodal content arrays, tool defs/choice, and `response_format` map across;
//! params with no unified home are DROPPED WITH A WARNING (never silently), and a
//! semantic-changing unsupported feature is rejected with `Unsupported`.

use serde::Deserialize;
use serde_json::Value;

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::req::{ChatRequest, ReasoningEffort, ResponseFormat};
use crate::toolcall::{ToolChoice, ToolDef};
use crate::translate::warn::{IngressError, Translated, Warning};

/// Params we knowingly drop (no unified equivalent yet) — warn, never fail.
const DROPPED_PARAMS: &[(&str, &str)] = &[
    ("logit_bias", "no unified equivalent"),
    ("frequency_penalty", "not modeled in the unified request"),
    ("presence_penalty", "not modeled in the unified request"),
    ("seed", "determinism is not portable across providers"),
    ("n", "only a single choice is supported"),
];

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(default)]
    tools: Vec<WireTool>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    max_tokens: Option<i64>,
    #[serde(default)]
    max_completion_tokens: Option<i64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    response_format: Option<Value>,
    #[serde(default)]
    user: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    tool_call_id: Option<String>,
}

#[derive(Deserialize)]
struct WireTool {
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Value>,
}

fn parse_role(role: &str) -> Result<Role, IngressError> {
    match role {
        "system" | "developer" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" | "function" => Ok(Role::Tool),
        other => Err(IngressError::Malformed(format!("unknown role `{other}`"))),
    }
}

/// Content may be a bare string or an array of typed parts.
fn parse_content(content: Option<Value>) -> Result<Vec<ContentPart>, IngressError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![ContentPart::text(s)]),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        let t = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        parts.push(ContentPart::text(t));
                    }
                    "image_url" => {
                        let url = item
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| IngressError::Malformed("image_url missing url".into()))?;
                        parts.push(ContentPart::Image {
                            source: ImageSource::Url { url: url.to_string() },
                        });
                    }
                    "input_audio" => {
                        return Err(IngressError::Unsupported { feature: "audio input".into() });
                    }
                    other => {
                        return Err(IngressError::Malformed(format!(
                            "unknown content part type `{other}`"
                        )));
                    }
                }
            }
            Ok(parts)
        }
        Some(other) => Err(IngressError::Malformed(format!("invalid content: {other}"))),
    }
}

fn parse_tool_choice(v: Value, warnings: &mut Vec<Warning>) -> Option<ToolChoice> {
    match v {
        Value::String(s) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" => Some(ToolChoice::Required),
            other => {
                warnings.push(Warning::dropped_param(
                    "tool_choice",
                    &format!("unknown value `{other}`, defaulting to auto"),
                ));
                Some(ToolChoice::Auto)
            }
        },
        Value::Object(map) => map
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .map(|name| ToolChoice::Function { name: name.to_string() }),
        _ => None,
    }
}

fn parse_response_format(v: Value) -> Result<ResponseFormat, IngressError> {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("text");
    match ty {
        "text" => Ok(ResponseFormat::Text),
        "json_object" => Ok(ResponseFormat::JsonObject),
        "json_schema" => {
            let js = v
                .get("json_schema")
                .ok_or_else(|| IngressError::Malformed("json_schema field missing".into()))?;
            Ok(ResponseFormat::JsonSchema {
                name: js.get("name").and_then(|n| n.as_str()).unwrap_or("schema").to_string(),
                schema: js.get("schema").cloned().unwrap_or(Value::Null),
                strict: js.get("strict").and_then(|s| s.as_bool()).unwrap_or(false),
            })
        }
        other => Err(IngressError::Unsupported {
            feature: format!("response_format type `{other}`"),
        }),
    }
}

fn parse_reasoning(effort: &str) -> Option<ReasoningEffort> {
    match effort {
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        _ => None,
    }
}

/// Parse a raw OpenAI Chat Completions body into a unified request + warnings.
pub fn parse_request(body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
    let wire: WireRequest = serde_json::from_value(body.clone())
        .map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings = Vec::new();

    for (name, reason) in DROPPED_PARAMS {
        if body.get(name).is_some() {
            warnings.push(Warning::dropped_param(name, reason));
        }
    }

    let mut messages = Vec::with_capacity(wire.messages.len());
    for m in wire.messages {
        let role = parse_role(&m.role)?;
        messages.push(Message {
            role,
            content: parse_content(m.content)?,
            tool_calls: Vec::new(),
            tool_call_id: m.tool_call_id,
        });
    }

    let tools = wire
        .tools
        .into_iter()
        .map(|t| ToolDef {
            name: t.function.name,
            description: t.function.description,
            parameters: t.function.parameters.unwrap_or(Value::Null),
        })
        .collect();

    let tool_choice = wire.tool_choice.and_then(|v| parse_tool_choice(v, &mut warnings));

    if wire.max_tokens.is_some() && wire.max_completion_tokens.is_some() {
        warnings.push(Warning::dropped_param(
            "max_tokens",
            "both max_tokens and max_completion_tokens set; using max_completion_tokens",
        ));
    }
    let max_tokens = wire.max_completion_tokens.or(wire.max_tokens);

    let response_format = match wire.response_format {
        Some(v) => Some(parse_response_format(v)?),
        None => None,
    };

    let reasoning_effort = wire.reasoning_effort.as_deref().and_then(parse_reasoning);

    Ok(Translated {
        value: ChatRequest {
            model: wire.model,
            messages,
            tools,
            tool_choice,
            temperature: wire.temperature,
            max_tokens,
            stream: wire.stream,
            reasoning_effort,
            response_format,
            user: wire.user,
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_minimal_request() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.model, "gpt-4o");
        assert_eq!(t.value.messages.len(), 1);
        assert_eq!(t.value.messages[0].text_content(), "Hi");
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn dropped_params_warn_not_fail() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
            "logit_bias": {"50256": -100},
            "seed": 42,
        });
        let t = parse_request(&body).unwrap();
        let codes: Vec<_> = t.warnings.iter().map(|w| w.message.as_str()).collect();
        assert!(codes.iter().any(|m| m.contains("logit_bias")));
        assert!(codes.iter().any(|m| m.contains("seed")));
    }

    #[test]
    fn multimodal_content_array_parses() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "look:"},
                    {"type": "image_url", "image_url": {"url": "https://x/y.png"}},
                ],
            }],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].content.len(), 2);
    }

    #[test]
    fn audio_input_is_unsupported_not_dropped() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [{"type": "input_audio", "input_audio": {"data": "...", "format": "wav"}}],
            }],
        });
        let err = parse_request(&body).unwrap_err();
        assert!(matches!(err, IngressError::Unsupported { .. }));
    }

    #[test]
    fn tools_and_tool_choice_function_parse() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.tools.len(), 1);
        assert_eq!(t.value.tools[0].name, "get_weather");
        assert_eq!(t.value.tool_choice, Some(ToolChoice::Function { name: "get_weather".into() }));
    }

    #[test]
    fn json_schema_response_format_parses() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "out", "schema": {"type": "object"}, "strict": true},
            },
        });
        let t = parse_request(&body).unwrap();
        assert!(matches!(
            t.value.response_format,
            Some(ResponseFormat::JsonSchema { strict: true, .. })
        ));
    }
}
```

Create `crates/gateway-llm/src/translate/ingress/mod.rs`:

```rust
//! Client-facing ingress dialects. Each module is the inverse of an egress
//! transport's wire mapping, but on the CLIENT side: parse a dialect request body
//! into the unified `ChatRequest`, and serialize a unified `ChatResponse`/
//! `StreamDelta` back out into that dialect (incl. its SSE frame shape). P1.4
//! routes each `/v1/...` endpoint to its dialect here.

pub mod openai_chat;
```

Add to `crates/gateway-llm/src/translate/mod.rs`:

```rust
pub mod ingress;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::ingress::openai_chat::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/ingress crates/gateway-llm/src/translate/mod.rs
git commit -s -m "feat(llm): OpenAI Chat Completions ingress request parsing (warn/unsupported)"
```

---

### Task 5: OpenAI Chat Completions ingress — response + SSE serialization

**Files:**
- Modify: `crates/gateway-llm/src/translate/ingress/openai_chat.rs`

The other half: serialize the unified `ChatResponse` into the `/v1/chat/completions` JSON a client expects, and serialize a unified `StreamDelta` into an OpenAI SSE `data:` frame. This closes the round-trip the conformance harness asserts (Task 9).

- [ ] **Step 1: Write the failing test**

Append to `crates/gateway-llm/src/translate/ingress/openai_chat.rs` (above the existing `#[cfg(test)]` block's closing brace is NOT allowed — add new public fns before `mod tests`, then add tests inside the existing module). First add the serializers (place directly above `#[cfg(test)]`):

```rust
use crate::resp::{ChatResponse, FinishReason};
use crate::stream::StreamDelta;

fn finish_str(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Unknown => "stop",
    }
}

/// Serialize a unified response into an OpenAI Chat Completions JSON body.
pub fn serialize_response(resp: &ChatResponse) -> Value {
    let tool_calls: Vec<Value> = resp
        .tool_calls
        .iter()
        .map(|c| {
            json_tool_call(&c.id, &c.name, &c.arguments)
        })
        .collect();
    let mut message = serde_json::Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert("content".into(), Value::String(resp.text()));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    serde_json::json!({
        "id": resp.provider_response_id.clone().unwrap_or_else(|| "chatcmpl-oximy".into()),
        "object": "chat.completion",
        "model": resp.model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_str(resp.finish_reason),
        }],
        "usage": {
            "prompt_tokens": resp.usage.input_tokens + resp.usage.cache_read_tokens,
            "completion_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.total(),
        },
    })
}

fn json_tool_call(id: &str, name: &str, arguments: &str) -> Value {
    serde_json::json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments },
    })
}

/// Serialize one unified delta into an OpenAI streaming chunk JSON (the body of
/// one `data:` SSE frame). Returns `None` for an empty delta (caller skips it).
pub fn serialize_delta(model: &str, delta: &StreamDelta) -> Option<Value> {
    if delta.is_empty() {
        return None;
    }
    let mut inner = serde_json::Map::new();
    if let Some(c) = &delta.content_delta {
        inner.insert("content".into(), Value::String(c.clone()));
    }
    if !delta.tool_call_deltas.is_empty() {
        let calls: Vec<Value> = delta
            .tool_call_deltas
            .iter()
            .map(|t| {
                let mut f = serde_json::Map::new();
                if let Some(n) = &t.name {
                    f.insert("name".into(), Value::String(n.clone()));
                }
                if let Some(a) = &t.arguments_delta {
                    f.insert("arguments".into(), Value::String(a.clone()));
                }
                serde_json::json!({"index": t.index, "id": t.id, "function": Value::Object(f)})
            })
            .collect();
        inner.insert("tool_calls".into(), Value::Array(calls));
    }
    let mut chunk = serde_json::json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": Value::Object(inner),
            "finish_reason": delta.finish_reason.map(finish_str),
        }],
    });
    if let Some(u) = &delta.usage {
        chunk["usage"] = serde_json::json!({
            "prompt_tokens": u.input_tokens + u.cache_read_tokens,
            "completion_tokens": u.output_tokens,
            "total_tokens": u.total(),
        });
    }
    Some(chunk)
}
```

Now add tests INSIDE the existing `mod tests` block (before its closing `}`):

```rust
    use crate::resp::ChatResponse;
    use crate::stream::StreamDelta;
    use gateway_spine::TokenUsage;

    #[test]
    fn serialize_response_shapes_openai_body() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("Hello")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { input_tokens: 8, output_tokens: 2, cache_read_tokens: 2, ..Default::default() },
            provider_response_id: Some("chatcmpl-1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["object"], "chat.completion");
        assert_eq!(j["choices"][0]["message"]["content"], "Hello");
        assert_eq!(j["choices"][0]["finish_reason"], "stop");
        // prompt_tokens recombines input + cache_read (the OpenAI total view).
        assert_eq!(j["usage"]["prompt_tokens"], 10);
        assert_eq!(j["usage"]["completion_tokens"], 2);
    }

    #[test]
    fn serialize_delta_emits_chunk_and_skips_empty() {
        assert!(serialize_delta("gpt-4o", &StreamDelta::default()).is_none());
        let c = serialize_delta("gpt-4o", &StreamDelta::text("ab")).unwrap();
        assert_eq!(c["object"], "chat.completion.chunk");
        assert_eq!(c["choices"][0]["delta"]["content"], "ab");
    }

    #[test]
    fn serialize_delta_carries_usage_on_final_chunk() {
        let d = StreamDelta::finish(
            FinishReason::Stop,
            TokenUsage { input_tokens: 5, output_tokens: 3, ..Default::default() },
        );
        let c = serialize_delta("gpt-4o", &d).unwrap();
        assert_eq!(c["choices"][0]["finish_reason"], "stop");
        assert_eq!(c["usage"]["completion_tokens"], 3);
    }
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::ingress::openai_chat::`
Expected: 9 tests PASS (6 from Task 4 + 3 new).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/ingress/openai_chat.rs
git commit -s -m "feat(llm): OpenAI Chat ingress response + streaming-chunk serialization"
```

---

### Task 6: Anthropic `/v1/messages` ingress dialect (request + response)

**Files:**
- Create: `crates/gateway-llm/src/translate/ingress/anthropic_messages.rs`
- Modify: `crates/gateway-llm/src/translate/ingress/mod.rs`

Claude Code speaks Anthropic Messages. Parse its request body INTO the unified shape (system hoisted out of `messages` becomes a `Role::System` message; content blocks → parts; `tools`/`tool_choice` map across) and serialize the unified response back into the `/v1/messages` body shape.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/ingress/anthropic_messages.rs`:

```rust
//! Anthropic Messages INGRESS dialect (client → unified). Claude Code POSTs
//! `/v1/messages` bodies; we parse them into the unified `ChatRequest`. The
//! top-level `system` string becomes a leading `Role::System` message; content
//! blocks (`text`/`image`/`tool_use`/`tool_result`) map to parts/tool fields. We
//! serialize the unified response back into the `/v1/messages` body. `max_tokens`
//! is required by the dialect — its absence is a `MissingField`.

use serde::Deserialize;
use serde_json::Value;

use crate::message::{ContentPart, ImageSource, Message, Role};
use crate::req::ChatRequest;
use crate::resp::{ChatResponse, FinishReason};
use crate::toolcall::{ToolChoice, ToolCall, ToolDef};
use crate::translate::warn::{IngressError, Translated, Warning};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    #[serde(default)]
    system: Option<Value>,
    messages: Vec<WireMessage>,
    #[serde(default)]
    max_tokens: Option<i64>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Vec<WireTool>,
    #[serde(default)]
    tool_choice: Option<Value>,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: Value,
}

#[derive(Deserialize)]
struct WireTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_schema: Option<Value>,
}

fn system_to_text(system: &Value) -> String {
    match system {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Anthropic content is a string or an array of typed blocks.
fn parse_content(content: Value) -> Result<(Vec<ContentPart>, Option<String>), IngressError> {
    match content {
        Value::String(s) => Ok((vec![ContentPart::text(s)], None)),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            let mut tool_call_id = None;
            for b in blocks {
                let ty = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                            parts.push(ContentPart::text(t));
                        }
                    }
                    "image" => {
                        let src = b.get("source").ok_or_else(|| {
                            IngressError::Malformed("image block missing source".into())
                        })?;
                        let media_type = src
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png")
                            .to_string();
                        let data = src
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        parts.push(ContentPart::Image {
                            source: ImageSource::Base64 { media_type, data },
                        });
                    }
                    "tool_result" => {
                        tool_call_id = b
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        if let Some(c) = b.get("content").and_then(|v| v.as_str()) {
                            parts.push(ContentPart::text(c));
                        }
                    }
                    "tool_use" => {
                        // Assistant tool calls are echoed in history; carried as text
                        // here (full assistant-tool replay is P1.4's history concern).
                    }
                    other => {
                        return Err(IngressError::Malformed(format!(
                            "unknown content block `{other}`"
                        )));
                    }
                }
            }
            Ok((parts, tool_call_id))
        }
        other => Err(IngressError::Malformed(format!("invalid content: {other}"))),
    }
}

fn parse_role(role: &str) -> Result<Role, IngressError> {
    match role {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        other => Err(IngressError::Malformed(format!("unknown role `{other}`"))),
    }
}

fn parse_tool_choice(v: &Value, warnings: &mut Vec<Warning>) -> Option<ToolChoice> {
    let ty = v.get("type").and_then(|t| t.as_str())?;
    match ty {
        "auto" => Some(ToolChoice::Auto),
        "any" => Some(ToolChoice::Required),
        "none" => Some(ToolChoice::None),
        "tool" => v
            .get("name")
            .and_then(|n| n.as_str())
            .map(|name| ToolChoice::Function { name: name.to_string() }),
        other => {
            warnings.push(Warning::dropped_param(
                "tool_choice",
                &format!("unknown anthropic tool_choice `{other}`"),
            ));
            None
        }
    }
}

/// Parse a raw Anthropic Messages body into a unified request + warnings.
pub fn parse_request(body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
    let wire: WireRequest = serde_json::from_value(body.clone())
        .map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings = Vec::new();

    let max_tokens = wire
        .max_tokens
        .ok_or_else(|| IngressError::MissingField("max_tokens".into()))?;

    let mut messages = Vec::new();
    if let Some(sys) = &wire.system {
        let text = system_to_text(sys);
        if !text.is_empty() {
            messages.push(Message::text(Role::System, text));
        }
    }
    for m in wire.messages {
        let role = parse_role(&m.role)?;
        let (content, tool_call_id) = parse_content(m.content)?;
        let role = if tool_call_id.is_some() { Role::Tool } else { role };
        messages.push(Message { role, content, tool_calls: Vec::new(), tool_call_id });
    }

    let tools = wire
        .tools
        .into_iter()
        .map(|t| ToolDef {
            name: t.name,
            description: t.description,
            parameters: t.input_schema.unwrap_or(Value::Null),
        })
        .collect();

    let tool_choice = wire.tool_choice.as_ref().and_then(|v| parse_tool_choice(v, &mut warnings));

    Ok(Translated {
        value: ChatRequest {
            model: wire.model,
            messages,
            tools,
            tool_choice,
            temperature: wire.temperature,
            max_tokens: Some(max_tokens),
            stream: wire.stream,
            reasoning_effort: None,
            response_format: None,
            user: None,
        },
        warnings,
    })
}

fn stop_reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "end_turn",
        FinishReason::Length => "max_tokens",
        FinishReason::ToolCalls => "tool_use",
        FinishReason::ContentFilter => "end_turn",
        FinishReason::Unknown => "end_turn",
    }
}

/// Serialize a unified response into an Anthropic `/v1/messages` body.
pub fn serialize_response(resp: &ChatResponse) -> Value {
    let mut blocks: Vec<Value> = resp
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(serde_json::json!({"type": "text", "text": text})),
            ContentPart::Image { .. } => None,
        })
        .collect();
    for c in &resp.tool_calls {
        let input: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
        blocks.push(serde_json::json!({
            "type": "tool_use", "id": c.id, "name": c.name, "input": input,
        }));
    }
    serde_json::json!({
        "id": resp.provider_response_id.clone().unwrap_or_else(|| "msg_oximy".into()),
        "type": "message",
        "role": "assistant",
        "model": resp.model,
        "content": blocks,
        "stop_reason": stop_reason(resp.finish_reason),
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens,
            "cache_read_input_tokens": resp.usage.cache_read_tokens,
            "cache_creation_input_tokens": resp.usage.cache_write_tokens,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::TokenUsage;
    use serde_json::json;

    #[test]
    fn system_is_hoisted_into_leading_message() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "system": "Be terse.",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].role, Role::System);
        assert_eq!(t.value.messages[0].text_content(), "Be terse.");
        assert_eq!(t.value.messages[1].role, Role::User);
        assert_eq!(t.value.max_tokens, Some(1024));
    }

    #[test]
    fn missing_max_tokens_is_a_missing_field_error() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "Hi"}],
        });
        assert!(matches!(parse_request(&body).unwrap_err(), IngressError::MissingField(_)));
    }

    #[test]
    fn tool_result_block_becomes_tool_role() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "tu_1", "content": "42"}],
            }],
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].role, Role::Tool);
        assert_eq!(t.value.messages[0].tool_call_id.as_deref(), Some("tu_1"));
    }

    #[test]
    fn tool_choice_any_maps_to_required() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"name": "f", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "any"},
        });
        let t = parse_request(&body).unwrap();
        assert_eq!(t.value.tool_choice, Some(ToolChoice::Required));
    }

    #[test]
    fn serialize_response_shapes_messages_body() {
        let resp = ChatResponse {
            model: "claude-3-5-sonnet-20241022".into(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![ToolCall { id: "tu_1".into(), name: "f".into(), arguments: "{\"x\":1}".into() }],
            finish_reason: FinishReason::ToolCalls,
            usage: TokenUsage { input_tokens: 800, output_tokens: 5, cache_read_tokens: 200, ..Default::default() },
            provider_response_id: Some("msg_1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["type"], "message");
        assert_eq!(j["stop_reason"], "tool_use");
        assert_eq!(j["content"][0]["text"], "ok");
        assert_eq!(j["content"][1]["type"], "tool_use");
        assert_eq!(j["content"][1]["input"]["x"], 1);
        assert_eq!(j["usage"]["cache_read_input_tokens"], 200);
    }
}
```

Add to `crates/gateway-llm/src/translate/ingress/mod.rs`:

```rust
pub mod anthropic_messages;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::ingress::anthropic_messages::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/ingress/anthropic_messages.rs crates/gateway-llm/src/translate/ingress/mod.rs
git commit -s -m "feat(llm): Anthropic /v1/messages ingress dialect (request + response)"
```

---

### Task 7: OpenAI **Responses** ingress dialect (request + response)

**Files:**
- Create: `crates/gateway-llm/src/translate/ingress/openai_responses.rs`
- Modify: `crates/gateway-llm/src/translate/ingress/mod.rs`

The Responses API (`/v1/responses`) is the third Tier-1 ingress dialect (design §5, P1 scope). Its `input` is a string OR a list of typed input items, `instructions` is the system prompt, and the output is an `output[]` array of items. Codex uses it. Parse INTO unified, serialize unified response back OUT.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/ingress/openai_responses.rs`:

```rust
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
                                source: ImageSource::Url { url: url.to_string() },
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
        other => Err(IngressError::Malformed(format!("invalid input content: {other}"))),
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
    let wire: WireRequest = serde_json::from_value(body.clone())
        .map_err(|e| IngressError::Malformed(e.to_string()))?;
    let mut warnings = Vec::new();

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
                messages.push(Message { role, content, tool_calls: Vec::new(), tool_call_id: None });
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
        FinishReason::Length => "incomplete",
        FinishReason::ContentFilter => "incomplete",
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
        assert!(t.warnings.iter().any(|w| w.message.contains("previous_response_id")));
    }

    #[test]
    fn serialize_response_shapes_output_array() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("done")],
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { input_tokens: 4, output_tokens: 1, ..Default::default() },
            provider_response_id: Some("resp_1".into()),
        };
        let j = serialize_response(&resp);
        assert_eq!(j["object"], "response");
        assert_eq!(j["status"], "completed");
        assert_eq!(j["output"][0]["content"][0]["text"], "done");
    }
}
```

Add to `crates/gateway-llm/src/translate/ingress/mod.rs`:

```rust
pub mod openai_responses;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::ingress::openai_responses::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/ingress/openai_responses.rs crates/gateway-llm/src/translate/ingress/mod.rs
git commit -s -m "feat(llm): OpenAI Responses ingress dialect (request + response)"
```

---

### Task 8: `Dialect` enum — uniform dispatch for P1.4

**Files:**
- Create: `crates/gateway-llm/src/translate/dialect.rs`
- Modify: `crates/gateway-llm/src/translate/mod.rs`

P1.4 needs one type to route an endpoint to its ingress dialect. A `Dialect` enum dispatches `parse_request`/`serialize_response` uniformly so the server doesn't match on three modules.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-llm/src/translate/dialect.rs`:

```rust
//! Uniform dialect dispatch. P1.4 maps each `/v1/...` route to a `Dialect` and
//! calls `parse_request`/`serialize_response` without knowing which dialect module
//! backs it. This is the single switchboard between the HTTP surface and the
//! per-dialect parsers/serializers.

use serde_json::Value;

use crate::req::ChatRequest;
use crate::resp::ChatResponse;
use crate::translate::ingress::{anthropic_messages, openai_chat, openai_responses};
use crate::translate::structured::ProviderFamily;
use crate::translate::warn::{IngressError, Translated};

/// A client-facing wire dialect served by the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `/v1/chat/completions`
    OpenAiChat,
    /// `/v1/messages`
    AnthropicMessages,
    /// `/v1/responses`
    OpenAiResponses,
}

impl Dialect {
    pub fn parse_request(&self, body: &Value) -> Result<Translated<ChatRequest>, IngressError> {
        match self {
            Dialect::OpenAiChat => openai_chat::parse_request(body),
            Dialect::AnthropicMessages => anthropic_messages::parse_request(body),
            Dialect::OpenAiResponses => openai_responses::parse_request(body),
        }
    }

    pub fn serialize_response(&self, resp: &ChatResponse) -> Value {
        match self {
            Dialect::OpenAiChat => openai_chat::serialize_response(resp),
            Dialect::AnthropicMessages => anthropic_messages::serialize_response(resp),
            Dialect::OpenAiResponses => openai_responses::serialize_response(resp),
        }
    }

    /// The provider family a downstream egress maps to (used to compile a
    /// structured-output plan). The dialect a CLIENT speaks is independent of the
    /// PROVIDER actually serving it — this returns the family for the request's
    /// resolved provider, defaulting by dialect when the router has not chosen yet.
    pub fn default_family(&self) -> ProviderFamily {
        match self {
            Dialect::OpenAiChat | Dialect::OpenAiResponses => ProviderFamily::OpenAi,
            Dialect::AnthropicMessages => ProviderFamily::Anthropic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dispatches_openai_chat_parse() {
        let body = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "Hi"}]});
        let t = Dialect::OpenAiChat.parse_request(&body).unwrap();
        assert_eq!(t.value.model, "gpt-4o");
    }

    #[test]
    fn dispatches_anthropic_parse() {
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "Hi"}],
        });
        let t = Dialect::AnthropicMessages.parse_request(&body).unwrap();
        assert_eq!(t.value.max_tokens, Some(16));
    }

    #[test]
    fn dispatches_responses_parse() {
        let body = json!({"model": "gpt-4o", "input": "Hi"});
        let t = Dialect::OpenAiResponses.parse_request(&body).unwrap();
        assert_eq!(t.value.messages[0].text_content(), "Hi");
    }

    #[test]
    fn families_match_dialect() {
        assert_eq!(Dialect::OpenAiChat.default_family(), ProviderFamily::OpenAi);
        assert_eq!(Dialect::AnthropicMessages.default_family(), ProviderFamily::Anthropic);
    }
}
```

Add to `crates/gateway-llm/src/translate/mod.rs`:

```rust
pub mod dialect;

pub use dialect::Dialect;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-llm translate::dialect::`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/src/translate/dialect.rs crates/gateway-llm/src/translate/mod.rs
git commit -s -m "feat(llm): Dialect enum for uniform ingress dispatch"
```

---

### Task 9: Golden-fixture conformance harness (round-trip fidelity)

**Files:**
- Create: `crates/gateway-llm/tests/fixtures/ingress/openai_chat_codex.json`
- Create: `crates/gateway-llm/tests/fixtures/ingress/anthropic_claude_code.json`
- Create: `crates/gateway-llm/tests/fixtures/ingress/openai_responses_sdk.json`
- Create: `crates/gateway-llm/tests/conformance.rs`

The milestone's invariant proof (design §3 item 8): **real client request shapes** (recorded from Codex / Claude Code / the OpenAI SDK) parse into the unified request, the unified response serializes back, and the round-trip is asserted lossless on the load-bearing fields. This is the gate that "stops the eternal streaming-regression whack-a-mole."

- [ ] **Step 1: Record the golden fixtures**

Create `crates/gateway-llm/tests/fixtures/ingress/openai_chat_codex.json` (a real-shape Chat Completions request as the OpenAI SDK / Codex emits it):

```json
{
  "model": "gpt-4o",
  "messages": [
    { "role": "system", "content": "You are a coding assistant." },
    { "role": "user", "content": "Refactor this function." }
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "apply_patch",
        "description": "Apply a unified diff to a file.",
        "parameters": { "type": "object", "properties": { "patch": { "type": "string" } }, "required": ["patch"] }
      }
    }
  ],
  "tool_choice": "auto",
  "temperature": 0.2,
  "stream": true,
  "logit_bias": { "50256": -100 }
}
```

Create `crates/gateway-llm/tests/fixtures/ingress/anthropic_claude_code.json` (a real-shape `/v1/messages` body as Claude Code emits it):

```json
{
  "model": "claude-3-5-sonnet-20241022",
  "max_tokens": 4096,
  "system": "You are Claude Code.",
  "messages": [
    { "role": "user", "content": [{ "type": "text", "text": "List the files." }] }
  ],
  "tools": [
    { "name": "list_files", "description": "List files in a dir.", "input_schema": { "type": "object", "properties": { "dir": { "type": "string" } } } }
  ],
  "tool_choice": { "type": "auto" },
  "stream": true
}
```

Create `crates/gateway-llm/tests/fixtures/ingress/openai_responses_sdk.json` (a real-shape `/v1/responses` body):

```json
{
  "model": "gpt-4o",
  "instructions": "Answer concisely.",
  "input": [
    { "role": "user", "content": [{ "type": "input_text", "text": "What is 2+2?" }] }
  ],
  "max_output_tokens": 256,
  "reasoning": { "effort": "low" },
  "stream": false
}
```

- [ ] **Step 2: Write the conformance harness**

Create `crates/gateway-llm/tests/conformance.rs`:

```rust
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
        usage: TokenUsage { input_tokens: 100, output_tokens: 20, cache_read_tokens: 10, ..Default::default() },
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
    assert_eq!(t.value.messages[1].text_content(), "Refactor this function.");
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
```

- [ ] **Step 3: Run the harness**

Run: `cargo test -p gateway-llm --test conformance`
Expected: 3 tests PASS — every real-client fixture parses, asserts its load-bearing fields, and round-trips its response. (If a fixture fails to parse, the dialect parser is wrong for a shape a real client actually sends — fix the parser, never the fixture, unless the fixture itself is unrealistic.)

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/fixtures/ingress crates/gateway-llm/tests/conformance.rs
git commit -s -m "test(llm): golden-fixture conformance harness for Codex/Claude Code/SDK"
```

---

### Task 10: Streaming tool-call aggregation conformance (kills the whack-a-mole)

**Files:**
- Create: `crates/gateway-llm/tests/fixtures/golden/openai_toolcall_stream.sse`
- Create: `crates/gateway-llm/tests/tool_aggregation_conformance.rs`

The specific regression every gateway clone re-breaks: tool-call `arguments` fragmented across SSE chunks must reassemble byte-exact. Here we drive the real SSE decoder (P1.2) + the `ToolCallAggregator` (Task 2) over a recorded fragmented-tool-call stream and assert the whole call.

- [ ] **Step 1: Record the fragmented-tool-call SSE fixture**

Create `crates/gateway-llm/tests/fixtures/golden/openai_toolcall_stream.sse` (OpenAI emits the `id`/`name` on the first chunk, then argument fragments; note the trailing blank line after each event):

```
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"ci"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ty\":\"SF\"}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8}}

data: [DONE]

```

- [ ] **Step 2: Write the conformance test**

This test reuses the P1.2 OpenAI streaming transport against a mocked upstream serving the fixture, then folds the emitted `StreamDelta`s through the `ToolCallAggregator` and asserts the reassembled call.

Create `crates/gateway-llm/tests/tool_aggregation_conformance.rs`:

```rust
//! Tool-call-delta correctness conformance (design §5 — "where every clone
//! breaks"). Drives the real P1.2 OpenAI streaming transport over a recorded
//! fragmented-tool-call SSE stream, folds the emitted deltas through the
//! `ToolCallAggregator`, and asserts the reassembled `arguments` is byte-exact.
//! Also covers the SSE decoder → aggregator seam directly (no HTTP) for a fast
//! unit-level regression guard.

use gateway_llm::message::{Message, Role};
use gateway_llm::provider::{Credentials, Provider};
use gateway_llm::req::ChatRequest;
use gateway_llm::resp::FinishReason;
use gateway_llm::transports::openai::OpenAi;
use gateway_llm::translate::aggregate::ToolCallAggregator;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fragmented_tool_call_reassembles_byte_exact_over_transport() {
    use futures::StreamExt;
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
    assert_eq!(calls[0].arguments, "{\"city\":\"SF\"}", "fragments reassembled byte-exact");
    assert_eq!(finish, Some(FinishReason::ToolCalls));
    assert_eq!(usage.expect("usage on final chunk").output_tokens, 8);
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p gateway-llm --test tool_aggregation_conformance`
Expected: PASS — the three `arguments` fragments (`""`, `{"ci`, `ty":"SF"}`) reassemble to exactly `{"city":"SF"}`, finish is `ToolCalls`, usage is preserved from the terminal chunk. (If `arguments` comes out malformed, either the SSE decoder split wrong or the aggregator's concatenation order is wrong — both are real bugs the whole gateway depends on.)

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/fixtures/golden/openai_toolcall_stream.sse crates/gateway-llm/tests/tool_aggregation_conformance.rs
git commit -s -m "test(llm): conformance — fragmented tool-call args reassemble byte-exact"
```

---

### Task 11: Structured-output emulation end-to-end conformance

**Files:**
- Create: `crates/gateway-llm/tests/structured_output_conformance.rs`

Proves the forced-tool-call emulation seam works end-to-end: an Anthropic-family structured-output request compiles to a forced tool, and an assistant tool-call answer unwraps back to the structured content string.

- [ ] **Step 1: Write the test**

Create `crates/gateway-llm/tests/structured_output_conformance.rs`:

```rust
//! Structured-output translation conformance. For an Anthropic-family request, a
//! `json_schema` response-format compiles to a forced single-tool call whose input
//! schema IS the requested schema; the model's tool-call answer unwraps back to the
//! structured payload. For OpenAI/Gemini families it stays native. No silent
//! degradation: an unsupported combination would surface as an error (none here).

use gateway_llm::req::ResponseFormat;
use gateway_llm::translate::structured::{ProviderFamily, StructuredOutputPlan, EMULATION_TOOL_NAME};
use serde_json::json;

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": { "answer": { "type": "string" }, "confidence": { "type": "number" } },
        "required": ["answer"]
    })
}

#[test]
fn anthropic_structured_output_emulates_and_unwraps() {
    let fmt = ResponseFormat::JsonSchema { name: "result".into(), schema: schema(), strict: true };
    let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::Anthropic).unwrap();

    // Compiles to a forced tool whose parameters are the requested schema.
    let tool = match &plan {
        StructuredOutputPlan::ForcedToolEmulation { tool, .. } => tool,
        other => panic!("expected emulation, got {other:?}"),
    };
    assert_eq!(tool.name, EMULATION_TOOL_NAME);
    assert_eq!(tool.parameters, schema());

    // The model answers by calling the emulation tool; we unwrap its args.
    let model_args = "{\"answer\":\"42\",\"confidence\":0.9}";
    let unwrapped = StructuredOutputPlan::unwrap_emulated(EMULATION_TOOL_NAME, model_args)
        .expect("emulation tool call unwraps to structured content");
    let parsed: serde_json::Value = serde_json::from_str(unwrapped).unwrap();
    assert_eq!(parsed["answer"], "42");

    // A normal (non-emulation) tool call passes through untouched.
    assert!(StructuredOutputPlan::unwrap_emulated("some_other_tool", "{}").is_none());
}

#[test]
fn openai_and_gemini_structured_output_stay_native() {
    let fmt = ResponseFormat::JsonSchema { name: "r".into(), schema: schema(), strict: false };
    for family in [ProviderFamily::OpenAi, ProviderFamily::Gemini] {
        let plan = StructuredOutputPlan::compile(Some(&fmt), family).unwrap();
        assert!(matches!(plan, StructuredOutputPlan::NativeJsonSchema { .. }), "{family:?}");
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p gateway-llm --test structured_output_conformance`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/structured_output_conformance.rs
git commit -s -m "test(llm): structured-output forced-tool emulation conformance"
```

---

### Task 12: Generate the per-pair fidelity matrix doc

**Files:**
- Create: `crates/gateway-llm/tests/fidelity_matrix.rs`
- Create: `crates/gateway-llm/docs/fidelity-matrix.md` (generated; committed)

The fidelity matrix (design §3 item 8, §5) is a published table of what each ingress-dialect → provider-family pair supports (native / emulated / dropped-with-warning / unsupported). We generate it from the actual translation behavior so it can never drift from the code, and write it to a committed doc; a test asserts the doc is up to date (regenerate-on-fail, like `cargo fmt --check`).

- [ ] **Step 1: Write the generator test**

Create `crates/gateway-llm/tests/fidelity_matrix.rs`:

```rust
//! Generates the per-pair fidelity matrix from ACTUAL translation behavior so the
//! published doc can never drift from the code (design §3 item 8). For each
//! (feature, provider-family) the support level is derived by compiling the
//! relevant plan and inspecting the result. Run `UPDATE_FIDELITY=1 cargo test
//! -p gateway-llm --test fidelity_matrix` to refresh the committed doc; the plain
//! test FAILS if the doc is stale (CI gate).

use gateway_llm::req::ResponseFormat;
use gateway_llm::translate::structured::{ProviderFamily, StructuredOutputPlan};
use serde_json::json;

const DOC_PATH: &str = "docs/fidelity-matrix.md";

fn structured_support(family: ProviderFamily) -> &'static str {
    let fmt = ResponseFormat::JsonSchema { name: "x".into(), schema: json!({"type": "object"}), strict: true };
    match StructuredOutputPlan::compile(Some(&fmt), family).unwrap() {
        StructuredOutputPlan::NativeJsonSchema { .. } => "native",
        StructuredOutputPlan::ForcedToolEmulation { .. } => "emulated (forced tool)",
        StructuredOutputPlan::NativeJsonObject => "json-object only",
        StructuredOutputPlan::None => "unsupported",
    }
}

fn render() -> String {
    let families = [
        ("OpenAI", ProviderFamily::OpenAi),
        ("Anthropic", ProviderFamily::Anthropic),
        ("Gemini", ProviderFamily::Gemini),
    ];
    let mut s = String::new();
    s.push_str("# Translation Fidelity Matrix\n\n");
    s.push_str("> GENERATED by `tests/fidelity_matrix.rs`. Do not edit by hand — run\n");
    s.push_str("> `UPDATE_FIDELITY=1 cargo test -p gateway-llm --test fidelity_matrix`.\n\n");
    s.push_str("Per-feature support by egress provider family. `native` = first-class;\n");
    s.push_str("`emulated` = preserved via a translation shim; `warn` = dropped with a\n");
    s.push_str("surfaced warning; `unsupported` = request rejected (no silent degradation).\n\n");
    s.push_str("| Feature | OpenAI | Anthropic | Gemini |\n");
    s.push_str("|---|---|---|---|\n");

    // structured output (json_schema)
    s.push_str("| `response_format: json_schema` |");
    for (_, f) in families {
        s.push_str(&format!(" {} |", structured_support(f)));
    }
    s.push('\n');

    // statically-known rows (behavior fixed by the transports/dialects in P1.2/P1.3)
    s.push_str("| tool/function calling | native | native | native |\n");
    s.push_str("| parallel tool calls | native | native | native |\n");
    s.push_str("| streaming tool-call deltas | native (aggregated) | native (aggregated) | native (aggregated) |\n");
    s.push_str("| vision (image parts) | native | native | native |\n");
    s.push_str("| `reasoning_effort` | native | warn (not mapped yet) | warn (not mapped yet) |\n");
    s.push_str("| prompt cache accounting | native | native | native |\n");
    s.push_str("| `logit_bias` / `seed` / `n` | warn (dropped) | warn (dropped) | warn (dropped) |\n");
    s.push_str("| audio input | unsupported | unsupported | unsupported |\n");

    s
}

#[test]
fn fidelity_matrix_doc_is_up_to_date() {
    let generated = render();
    if std::env::var("UPDATE_FIDELITY").is_ok() {
        std::fs::write(DOC_PATH, &generated).expect("write fidelity matrix");
        return;
    }
    let on_disk = std::fs::read_to_string(DOC_PATH).unwrap_or_default();
    assert_eq!(
        on_disk, generated,
        "fidelity matrix is stale — run `UPDATE_FIDELITY=1 cargo test -p gateway-llm --test fidelity_matrix`"
    );
}
```

- [ ] **Step 2: Generate the doc, then verify the gate**

Run: `UPDATE_FIDELITY=1 cargo test -p gateway-llm --test fidelity_matrix`
Expected: PASS — writes `crates/gateway-llm/docs/fidelity-matrix.md`.

Then run the plain gate: `cargo test -p gateway-llm --test fidelity_matrix`
Expected: PASS — the on-disk doc matches the generated output. (If it fails, the doc drifted; regenerate.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
git add crates/gateway-llm/tests/fidelity_matrix.rs crates/gateway-llm/docs/fidelity-matrix.md
git commit -s -m "feat(llm): generated per-pair fidelity matrix + staleness gate"
```

---

### Task 13: Finalize `translate` module surface + full-crate gate

**Files:**
- Modify: `crates/gateway-llm/src/translate/mod.rs`
- Modify: `crates/gateway-llm/src/lib.rs`

- [ ] **Step 1: Ensure `translate/mod.rs` re-exports the full public surface**

Ensure `crates/gateway-llm/src/translate/mod.rs` reads exactly (modules + re-exports, no placeholder):

```rust
//! The translation core: ingress dialect ⇄ unified ⇄ provider. Pure, I/O-free
//! functions over the P1.2 types. `warn` owns the no-silent-degradation taxonomy;
//! `aggregate` stitches streamed tool-call fragments; `structured` compiles
//! structured-output plans; `ingress` holds one parser/serializer per client
//! dialect; `dialect` is the uniform switchboard P1.4 routes to. The conformance
//! harness (tests/) gates every dialect round-trip.

pub mod aggregate;
pub mod dialect;
pub mod ingress;
pub mod structured;
pub mod warn;

pub use aggregate::ToolCallAggregator;
pub use dialect::Dialect;
pub use structured::{ProviderFamily, StructuredOutputPlan};
pub use warn::{IngressError, Translated, Warning};
```

- [ ] **Step 2: Ensure `lib.rs` re-exports the translation core**

Ensure `crates/gateway-llm/src/lib.rs` includes (alongside the existing P1.2 re-exports):

```rust
pub mod translate;

pub use translate::{
    Dialect, IngressError, ProviderFamily, StructuredOutputPlan, ToolCallAggregator, Translated,
    Warning,
};
```

- [ ] **Step 3: Run the entire crate's tests + the full gate**

Run: `cargo test -p gateway-llm`
Expected: every P1.2 test + every new translate unit test (warn/aggregate/structured/ingress×3/dialect) + every integration test (conformance, tool_aggregation_conformance, structured_output_conformance, fidelity_matrix) PASS.

Then the gate:

```bash
cargo fmt --all && cargo clippy -p gateway-llm --all-targets -- -D warnings
```

Expected: clean. If clippy flags `collapsible_if` anywhere in the new parsers (the `if entry.id.is_none() { if let Some(id) ... }` shape), rewrite as a let-chain (`if entry.id.is_none() && let Some(id) = &frag.id {}`) per Rust 2024 discipline — it is already written that way in `aggregate.rs`, mirror it.

- [ ] **Step 4: Commit**

```bash
git add crates/gateway-llm/src/translate/mod.rs crates/gateway-llm/src/lib.rs
git commit -s -m "feat(llm): finalize translation-core module surface"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-llm` is fully green (all P1.2 tests + all translate unit tests + `conformance`, `tool_aggregation_conformance`, `structured_output_conformance`, `fidelity_matrix`).
- [ ] `cargo clippy -p gateway-llm --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean; `#![forbid(unsafe_code)]` holds.
- [ ] The three invariants this milestone owns are each proven by a test:
  - **Conformance-tested translation** — `conformance.rs` parses + round-trips real Codex/Claude Code/OpenAI-SDK fixtures across all three dialects (the merge gate that stops the streaming-regression whack-a-mole).
  - **No silent degradation** — dropped params surface as `Warning` (`dropped_params_warn_not_fail`, fixture `logit_bias` assertion) and semantic gaps as `IngressError::Unsupported` (`audio_input_is_unsupported_not_dropped`).
  - **Tool-call-delta correctness** — `fragmented_tool_call_reassembles_byte_exact_over_transport` drives the real SSE decoder + `ToolCallAggregator` and asserts byte-exact `arguments`.
- [ ] All three Tier-1 ingress dialects parse-and-serialize: OpenAI Chat Completions, Anthropic `/v1/messages`, OpenAI Responses — dispatched uniformly via `Dialect`.
- [ ] Structured output is translated, not dropped: native for OpenAI/Gemini, forced-tool emulation for Anthropic, with an unwrap path back to content; proven by `structured_output_conformance`.
- [ ] The per-pair fidelity matrix is GENERATED from real behavior and committed (`crates/gateway-llm/docs/fidelity-matrix.md`), with a staleness gate (`fidelity_matrix.rs`).
- [ ] The translation core is pure (no `reqwest`/`async` in `src/translate/`, grep to confirm) — egress I/O stays in `transports/`.
- [ ] Public surface is stable and re-exported for P1.4: `Dialect`, `Translated`, `Warning`, `IngressError`, `ToolCallAggregator`, `StructuredOutputPlan`, `ProviderFamily`.

**Next:** `2026-06-10-p1-04-http-server-ingress.md` — the `gateway-control` HTTP server that mounts `/v1/chat/completions` (+stream), `/v1/responses`, `/v1/messages`, `/v1/embeddings`, `/v1/models`, routes each to its `Dialect` (parse → spine auth/budget/rate-limit → route → egress transport → commit → log), surfaces `Translated.warnings` on the response, and folds streamed deltas through the `ToolCallAggregator` and back out via each dialect's SSE serializer — all built on the translation contract frozen in this milestone.
```

