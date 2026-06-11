# Dimension Deep-Dive: Unified API Design in AI Gateways (June 2026)

Competitive-intelligence report for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

Researched: 2026-06-10. Sources: official docs (LiteLLM, OpenRouter, Portkey, Bifrost, Vercel, Cloudflare, Envoy AI Gateway, Kong, Helicone, TensorZero, OpenAI, Anthropic, DeepSeek), GitHub issues, third-party comparisons.

---

## 1. The landscape: there is no longer ONE "unified API" — there are four dialects

In 2024 "unified API" meant "OpenAI Chat Completions for everything." By mid-2026 the table stakes have tripled. A credible gateway must speak **four wire protocols**, in both directions:

| Dialect | Endpoint | Who demands it | 2026 status |
|---|---|---|---|
| OpenAI Chat Completions | `POST /v1/chat/completions` | Widest SDK/framework ecosystem | Legacy-but-universal; OpenAI says "Responses recommended for all new projects" but CC remains supported |
| OpenAI Responses API | `POST /v1/responses` | Codex CLI, OpenAI Agents SDK, anything using reasoning items/encrypted reasoning/built-in tools | The growth dialect. Assistants API dies Aug 26, 2026; Codex has discussed deprecating chat/completions support entirely |
| Anthropic Messages | `POST /v1/messages` (+ `/v1/messages/count_tokens`) | **Claude Code** (sets `ANTHROPIC_BASE_URL`), Anthropic SDKs, agent harnesses | The killer driver: every gateway added it in 2025–26 specifically to capture Claude Code traffic |
| Gemini generateContent | `POST /v1beta/models/{model}:generateContent` (+ `:streamGenerateContent`) | Gemini CLI, google-genai SDK | Least common; LiteLLM and Bifrost expose it; most others only passthrough |

Key strategic fact: **the protocols are converging from both sides.** Providers themselves now expose competitors' formats — Anthropic ships an OpenAI-SDK compatibility layer; Gemini has an OpenAI-compatible endpoint; DeepSeek exposes `https://api.deepseek.com/anthropic` (and maps `claude-opus*` → deepseek-v4-pro, `claude-sonnet/haiku*` → deepseek-v4-flash); Z.ai exposes `https://api.z.ai/api/anthropic` for GLM; all the Chinese labs (Qwen, Kimi, MiniMax) ship OpenAI-compat endpoints. The Anthropic Messages format has become a de-facto second standard because Claude Code reads `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` and is the highest-spend agent client in the market.

**Implication:** a gateway that only normalizes *into* OpenAI format is a 2024 design. The 2026 design is N×M: any client dialect in, any provider out, with documented per-pair fidelity.

---

## 2. Per-gateway survey of unified-API surface

### LiteLLM (BerriAI) — the maximal-surface reference
- **Dialects in:** `/v1/chat/completions`, `/v1/responses`, `/v1/messages` (Anthropic spec, "works with ALL LiteLLM providers"), `/gemini/v1beta/models/{m}:generateContent` (Google AI Studio format; explicitly Gemini-CLI compatible — Vertex backend NOT supported for this route due to URL format incompatibility).
- **Translation depth:** publishes an actual **parameter-mapping document** for `/v1/messages` → OpenAI Responses API: `max_tokens`→`max_output_tokens`; Anthropic messages expanded into Responses input items (tool_use/tool_result become top-level items); system → `instructions` (multi-block joined by newlines); `tool_choice` `"any"`→`{"type":"required"}`; `thinking.budget_tokens` → effort buckets (high/medium/low/minimal), summary forced to "detailed"; web-search tool → `web_search_preview`. **Documented lossiness:** `stop_sequences` dropped silently, `top_k` unsupported, cache_control/citations not in the mapping tables.
- **Thinking support on /v1/messages:** full `thinking` object with budget_tokens + summary styles; response content blocks include `thinking` and `redacted_thinking`.
- **Passthrough:** dedicated passthrough routes per provider (`/anthropic/*`, `/gemini/*`, `/vertex_ai/*`, etc.) covering ALL native endpoints incl. streaming, plus user-defined custom passthrough endpoints — explicitly framed as "give devs Anthropic endpoints without the raw key." Caveat: spend tracking historically broken for `anthropic_messages` and passthrough call types (issue #24204).
- **Other endpoints:** embeddings, images, audio (STT/TTS), batch, files, moderations, rerank, realtime — broadest endpoint catalog of any gateway.
- **Fidelity reputation:** the cautionary tale. Long trail of translation regressions: streaming adapter dropping tool_use input arguments for non-Anthropic models (regression across v1.82.x); Anthropic→OpenAI conversion dropping `reasoning_content`, breaking multi-turn with reasoning models; Claude Code's `input_text` blocks silently dropped in the OpenAI passthrough path; OpenAI-proxy streaming for Claude emitting text-deltas with unregistered IDs that break Vercel AI SDK 6.x multi-step tool calls; tool names >64 chars passed through unmodified and rejected by OpenAI; tool_use ordering broken when context compaction merges assistant turns.

### OpenRouter — the hosted benchmark for API ergonomics
- **Dialects in:** Chat Completions (primary), **Responses API (beta, deliberately stateless** — "no conversation state persisted"; supports reasoning w/ configurable effort + encrypted reasoning chains, parallel tool calling, web search with citation annotations), and **`/api/v1/messages`** (Anthropic format: text, images, PDFs, tools, extended thinking). Claude Code works by pointing `ANTHROPIC_BASE_URL=https://openrouter.ai/api` — no local proxy.
- **Catalog as API:** `GET /api/v1/models` returns normalized machine-readable metadata per model — ID, **pricing in USD/token**, context length, **`supported_parameters` array**, modality — cached at edge; plus `GET /endpoints` listing per-model provider endpoints. This is the best-in-class machine-readable capability surface; agents can program against it.
- **Normalization stance:** normalizes a *superset* of params (e.g. unified `reasoning` object with `effort`/`max_tokens`), forwards unknown provider-specific params where safe, normalized finish_reason and error schema across providers.

### Portkey — three-format universal API, broad endpoint catalog
- Explicitly markets that all three formats (Chat Completions, Responses, Messages) work **against all 200+ models**, with automatic cross-translation; switch providers via `@provider/model` string.
- Endpoint catalog: images (gen/edit/variations), audio (STT/TTS), fine-tuning, batch, files, moderations, Assistants, legacy completions. Publishes a **provider compatibility matrix** because "not all providers support every endpoint or modality" — honest fidelity-matrix precedent worth copying.

### Bifrost (Maxim) — provider-compatible gateway pattern, Go, performance leader
- One OpenAI-compatible API over 1,000+ models / 25+ providers, **plus provider-specific compatible endpoints** (Anthropic, Gemini, Cohere, Bedrock…) so existing provider SDKs work with zero code change ("drop-in: change only the base URL in your OpenAI, Anthropic, Bedrock, or Google SDK").
- Endpoints: chat, classic text completions (with **fallback emulation** when provider lacks it), `/v1/embeddings`, image gen/edit, TTS/STT, batch, files, responses.
- Documents raw-request rewriting (e.g. `max_completion_tokens` → Anthropic `max_tokens`).
- Perf claim: 11 µs overhead/request at 5k RPS sustained; "50x faster than LiteLLM"; cluster mode.

### Vercel AI Gateway — hosted, AI-SDK-first, added compat endpoints later
- Base `https://ai-gateway.vercel.sh`: OpenAI-compatible `/v1` (hundreds of models via `creator/model` slug) + **Anthropic-compatible `/v1/messages` AND `/v1/messages/count_tokens`** — streaming, tool calls, extended thinking, structured outputs, file attachments. Auth via API key or **OIDC token** (deploy-context auth — interesting for agents). Primary interface is still the AI SDK (TypeScript), with the HTTP compat endpoints as escape hatches.

### Cloudflare AI Gateway — proxy-of-record + growing unified layer
- Four surfaces: `POST /ai/run` (universal endpoint, all models/modalities), `/ai/v1/chat/completions` (OpenAI compat), `/ai/v1/responses` (Responses compat), plus **per-provider native routes** (`.../anthropic/v1/messages`, OpenAI, Google AI Studio, Bedrock, Azure, Workers AI…). Older "Universal Endpoint" (request-array-with-fallbacks) now deprecated in favor of the unified API — instructive evolution.
- **Realtime WebSockets API**: persistent connections proxying OpenAI Realtime, Gemini Live, Cartesia, ElevenLabs — the only gateway with first-class realtime/speech-to-speech proxying.

### Envoy AI Gateway — K8s-native, strongest cross-provider translation engineering
- Endpoints: chat completions, completions, `/v1/embeddings` (extended to Bedrock Titan + Gemini), **native Anthropic `/v1/messages` servable on OpenAI-compatible backends**, audio transcription/translation, image generation, Responses API (OpenAI + Azure variants).
- Translators as first-class components: "Anthropic Messages → AWS Bedrock Converse translator"; "Anthropic-to-OpenAI translation now handles reasoning blocks and images end-to-end"; **single `reasoning_effort` knob unified across Anthropic, OpenAI, and Gemini** (v0.6).
- **Provider-agnostic prompt caching:** unified `cache_control` API translated to provider-specific directives across direct Anthropic, Vertex Claude, Bedrock Claude — explicitly noting the three APIs differ in "directive shape, response shape, TTL options, and even when caching is invoked."
- Also ships an **MCP Gateway** (OAuth, server multiplexing, tools/list filtered by authorization rules) — the closest existing analog to the LLM+MCP combined gateway concept. CRDs at v1beta1.

### Kong AI Gateway — route_type config model + native-format passthrough
- `config.route_type`: `llm/v1/chat`, `llm/v1/completions`, `llm/v1/responses`, `llm/v1/assistants`, `llm/v1/files`, `llm/v1/batches`, embeddings, audio, image, realtime — request/response transformed to OpenAI format by default.
- `config.llm_format != openai` ⇒ **native passthrough with analytics/cost still computed** (v3.10+): the pattern of "observe without translating." Ships how-to guides specifically for routing Claude Code traffic to OpenAI/Vertex/Bedrock backends.

### Helicone AI Gateway — minimal-surface, Rust, OpenAI-format-only
- One OpenAI-compatible API over 100+ models; "NGINX of LLMs"; 30MB binary, ~100ms cold start, p95 <5ms vs "60ms+ Python gateways". Deliberately does NOT chase multi-dialect ingress — differentiates on speed + observability instead. Shows the minimal viable unified API still has a market.

### TensorZero — the "superset-native" outlier
- Rust gateway whose **native API is its own schema** (functions/variants, structured "inferences", episodes), with an OpenAI-compatible `/openai/v1/chat/completions` adapter on top. Returns `thought` and unknown provider-specific content blocks via `tensorzero_extra_content` extension fields rather than dropping them; `thinking_budget_tokens` as a first-class inference param; schema enforcement on inputs/outputs; tool use, JSON structured outputs, batch, embeddings, multimodal (images/files), caching. GitOps config. The most principled answer to "what do you do with content the target dialect can't express" — escape-hatch extension blocks, never silent drops.

### LLMGateway.io — smaller OSS player; native `/v1/messages` over any catalog model; 40+ providers; one-line base-URL change.

---

## 3. Translation fidelity: where unification actually breaks

The fidelity matrix is the real product. Specific failure modes observed in the wild (mostly LiteLLM's issue tracker, the largest corpus):

1. **Streaming tool calls** — hardest case. Argument deltas dropped (empty `input` in tool_use blocks); text-delta IDs emitted without text-start events breaking AI SDK 6.x; tool_use/tool_result ordering invalidated when histories are compacted/merged.
2. **Reasoning/thinking round-trips** — Anthropic→OpenAI conversion dropped `reasoning_content`, breaking multi-turn with reasoning models (the model needs its prior reasoning items back). Responses API reasoning items vs Anthropic `thinking`/`redacted_thinking` blocks vs Gemini thought parts have no clean bijection; LiteLLM buckets budget_tokens→effort; Envoy unifies on one `reasoning_effort` knob. Encrypted/signed reasoning blocks (OpenAI encrypted reasoning, Anthropic signatures) cannot be re-fabricated by a gateway — must be passed through opaquely.
3. **Prompt caching params** — Anthropic `cache_control` vs OpenAI implicit/automatic caching vs Bedrock Converse cachePoint differ in directive shape, response usage fields, TTLs. Envoy AI Gateway is the only one with a documented provider-agnostic `cache_control` translation; most others silently drop it on cross-provider routes (a *cost* bug, not just a fidelity bug).
4. **Silent parameter drops** — LiteLLM documents dropping `stop_sequences` and `top_k` silently on the Messages→Responses path. Silent drops are the most-complained-about behavior; agents can't react to what they can't see.
5. **Validation asymmetries** — OpenAI's 64-char tool-name limit rejects requests that are valid Anthropic; client-specific content block types (Claude Code sends `input_text`) get dropped instead of normalized.
6. **Structured output** — Anthropic has no native `response_format`; gateways emulate JSON-schema output over Messages via forced tool-call (LiteLLM has a dedicated "Structured Output /v1/messages" doc); streaming+response_format+tools combinations have been repeatedly buggy.
7. **Images/files** — content-part shape differences (Anthropic `source.base64` vs OpenAI `image_url` vs Gemini `inline_data`); Anthropic-adapter image translation needed explicit fixes; PDFs/file attachments only recently handled (OpenRouter Messages supports PDFs; Vercel Messages supports file attachments).
8. **Usage/cost accounting across dialects** — token field names and cache-hit accounting differ; spend tracking for passthrough/messages call types lagged actual support in LiteLLM.

**Lesson:** translation must be a *specified, conformance-tested* subsystem (golden round-trip fixtures per client×provider pair, esp. Claude Code, Codex, Gemini CLI, Vercel AI SDK, OpenAI Agents SDK), with a published fidelity matrix per parameter — and an explicit policy for inexpressible content: passthrough-opaque, extension-field (TensorZero `tensorzero_extra_content`), warn-header, or hard-error. Never silent.

---

## 4. Beyond chat: full endpoint surface expected in 2026

| Endpoint class | Who has it | Notes |
|---|---|---|
| Embeddings `/v1/embeddings` | LiteLLM, Bifrost, Envoy (incl. Bedrock Titan, Gemini), Kong, TensorZero | Table stakes |
| Rerank | LiteLLM (`/v1/rerank`, Cohere-shape) | Rare; differentiator for RAG users |
| Images gen/edit/variations | LiteLLM, Portkey, Bifrost, Envoy, Kong, Cloudflare | OpenAI Images shape is the lingua franca |
| Audio STT/TTS | LiteLLM, Portkey, Bifrost, Envoy (transcribe/translate), Kong | |
| Realtime (WebSocket) | **Cloudflare only** (OpenAI Realtime, Gemini Live, Cartesia, ElevenLabs); Kong lists realtime route_type | Big white space for OSS gateways |
| Batch + Files | LiteLLM, Portkey, Bifrost, Kong (`llm/v1/batches`, `llm/v1/files`) | Needed for cost-sensitive pipelines |
| Moderations | LiteLLM, Portkey | |
| count_tokens | Vercel, OpenRouter, LiteLLM (Anthropic-shape) | Agents use this constantly for context budgeting |
| Model catalog API | OpenRouter (gold standard: pricing + supported_parameters + context), LiteLLM `/model/info`, others weaker | |
| MCP gateway | Envoy AI Gateway (OAuth + multiplexing + authz-filtered tools/list); LiteLLM has MCP gateway too | The convergence point with your product thesis |

---

## 5. What a SUPERSET unified API looks like in 2026 (synthesis)

1. **Four ingress dialects, all first-class:** `/v1/chat/completions`, `/v1/responses` (stateless by default, OpenRouter-style, with optional state), `/v1/messages` (+count_tokens), `/v1beta/models/{m}:generateContent`. Each works against ANY backend model.
2. **N×M translation core with a published fidelity matrix** — per parameter, per pair: lossless / mapped / emulated / dropped-with-warning. Conformance fixtures from real agent clients (Claude Code, Codex CLI, Gemini CLI, AI SDK).
3. **Unified capability params** that normalize the genuinely divergent stuff: one `reasoning` knob (effort + budget_tokens + include-in-output), one `cache_control` translated per provider, one structured-output contract (native where supported, tool-call emulation where not), one web-search/built-in-tool abstraction.
4. **Opaque-preservation policy:** signed/encrypted reasoning blocks, provider-specific content blocks, and unknown params ride through in extension fields (`x_*` / `extra_content`) instead of being dropped.
5. **Native passthrough as a peer mode, not an afterthought:** per-provider routes covering ALL native endpoints, with metering/logging/cost still applied (Kong `llm_format`, LiteLLM passthrough). This is how you support day-0 provider features before translation catches up.
6. **Full modality endpoint set:** embeddings, rerank, images, audio, batch, files, moderations, count_tokens — plus realtime WebSocket proxying (almost nobody OSS has it).
7. **Machine-readable catalog:** `/v1/models` with pricing, context, modalities, `supported_parameters`, per-endpoint provider list (OpenRouter shape) — this is the agent-discoverability layer.
8. **Normalized errors, finish_reasons, and usage** (incl. cache-read/write tokens) across all dialects so cost accounting is dialect-independent.
9. **MCP gateway sharing the same auth/policy/observability plane** as the LLM gateway (Envoy is the only one doing this today).

## 6. Agent-experience (AX) observations

- The #1 agent integration pattern is **env-var base-URL swap**: `ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN` (Claude Code), `OPENAI_BASE_URL` (Codex et al.), Gemini CLI passthrough routes. Gateways win agent traffic by speaking each agent's *native* dialect perfectly, not by asking agents to adopt a new one — entire OSS proxy ecosystems (claude-code-router, y-router, claude-code-adapter) exist solely to patch gateways that don't.
- Agents need machine-readable capability discovery: OpenRouter's `/models` (pricing + supported_parameters, edge-cached) is what agents actually program against; most gateways have nothing comparable.
- Stateless Responses API (OpenRouter) is the agent-friendly choice — agents manage their own history; server-side state is a liability for replay/audit.
- `count_tokens` endpoints, normalized error schemas, and explicit dropped-param warnings are the difference between an agent that can self-correct and one that silently degrades.
- Envoy's MCP gateway (OAuth, multiplexing, authz-filtered tools/list) is the only shipped example of MCP and LLM traffic governed by one plane — directly validates the combined-gateway thesis.

## 7. Notable performance claims (context for this dimension)

- Bifrost: 11 µs added overhead at 5k RPS sustained; "50x faster than LiteLLM"; <100 µs claims at 5k RPS.
- Helicone: Rust, 30MB binary, ~100ms cold start, p95 <5ms gateway overhead vs "60ms+ for Python gateways."
- Translation layers are where latency and correctness costs concentrate; passthrough modes exist partly as a perf escape hatch.

## 8. Sources (primary)

- Portkey Universal API: https://portkey.ai/docs/product/ai-gateway/universal-api
- LiteLLM /v1/messages: https://docs.litellm.ai/docs/anthropic_unified/ ; mapping doc: https://docs.litellm.ai/docs/anthropic_unified/messages_to_responses_mapping ; generateContent: https://docs.litellm.ai/docs/generateContent ; passthrough: https://docs.litellm.ai/docs/pass_through/anthropic_completion
- OpenRouter Responses beta: https://openrouter.ai/docs/api/reference/responses/overview ; models API: https://openrouter.ai/docs/api/api-reference/models/get-models ; Claude Code: https://openrouter.ai/docs/cookbook/coding-agents/claude-code-integration
- Bifrost providers: https://docs.getbifrost.ai/providers/supported-providers/overview ; repo: https://github.com/maximhq/bifrost
- Vercel Anthropic compat: https://vercel.com/docs/ai-gateway/anthropic-compat ; OpenAI compat: https://vercel.com/docs/ai-gateway/openai-compat
- Cloudflare unified API: https://developers.cloudflare.com/ai-gateway/usage/chat-completion/ ; REST API changelog: https://developers.cloudflare.com/changelog/post/2026-05-21-rest-api/
- Envoy AI Gateway release notes: https://aigateway.envoyproxy.io/release-notes/ ; prompt caching: https://aigateway.envoyproxy.io/docs/capabilities/llm-integrations/prompt-caching/
- Kong AI Proxy: https://developer.konghq.com/plugins/ai-proxy/
- Helicone gateway: https://github.com/Helicone/ai-gateway
- TensorZero: https://www.tensorzero.com/docs/gateway/api-reference/inference-openai-compatible ; https://github.com/tensorzero/tensorzero
- OpenAI migration: https://platform.openai.com/docs/guides/migrate-to-responses ; deprecations: https://developers.openai.com/api/docs/deprecations
- Anthropic OpenAI SDK compat: https://platform.claude.com/docs/en/api/openai-sdk
- DeepSeek Anthropic API: https://api-docs.deepseek.com/guides/anthropic_api
- LiteLLM fidelity issues: BerriAI/litellm #25321, #27946, #23841, #26529, #17904, #24204, #22946, #10435
- TrueFoundry provider-agnostic caching analysis: https://www.truefoundry.com/blog/provider-agnostic-prompt-caching-llm-gateway
