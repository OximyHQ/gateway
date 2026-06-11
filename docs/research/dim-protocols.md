# Dimension Deep-Dive: Protocol & Modality Coverage in AI Gateways

**Date:** 2026-06-10
**Scope:** Streaming SSE fidelity, WebSocket/Realtime proxying, batch APIs, files APIs, fine-tuning, image gen, video gen (Veo/Sora), TTS/STT, rerank, embeddings, computer-use, structured outputs, logprobs, MCP — across LiteLLM, Portkey, Bifrost, OpenRouter, Kong AI Gateway, Envoy AI Gateway, Cloudflare AI Gateway, Vercel AI Gateway, Helicone, TensorZero.

---

## 1. Why this dimension matters

The "long tail" of modalities is the single biggest hidden differentiator between gateways. Every gateway nails `/chat/completions`. Where they fork is everything else: Realtime audio over WebSocket, batch/files/fine-tuning passthrough, video generation, rerank, and — most painfully — **streaming fidelity** (does the SSE byte stream that exits the gateway match what the provider sent, event-for-event?). Agent runtimes (Claude Code, Codex CLI, Gemini CLI, OpenAI Agents SDK) are now the most demanding clients: they consume native provider APIs (`/v1/messages`, `/v1/responses`, Gemini `generateContent`) with beta headers, extended thinking, and strict SSE event sequencing — and they break loudly when a gateway normalizes, reorders, or drops anything.

---

## 2. Per-gateway protocol/modality matrix

### 2.1 LiteLLM (Python, MIT core + enterprise) — the maximalist

Broadest modality surface of any gateway, by far. Documented supported endpoints:

- **Text:** `/chat/completions`, `/completions`, `/responses` (OpenAI Responses API with prompt templates), **native Anthropic `/v1/messages`**, **Bedrock `/converse` and `/invoke`**, Google `/generateContent`.
- **Embeddings** `/embeddings`; **Rerank** `/rerank` (Cohere request/response format); **Moderations**; **OCR**.
- **Images:** generations, `/images/edits`, `/images/variations` (beta).
- **Video:** first-class `/videos` endpoint — Sora via OpenAI video models, plus ModelsLab/Kling; **Veo via Google AI Studio passthrough**; built-in async polling, cost tracking, fallbacks ("switch Sora↔Kling↔Veo by changing one string").
- **Audio:** `/audio/transcriptions` (STT), `/audio/speech` (TTS).
- **Realtime:** `/realtime` WebSocket proxy for OpenAI/Azure realtime models, **WebRTC support** (gateway handles auth, audio streams direct to provider), realtime guardrails (pre/post-call hooks, voice transcription hooks, session termination policies, v1.82), configurable `logged_real_time_event_types`. Also **Vertex AI Live API WebSocket passthrough**.
- **Ops surfaces:** `/batches`, `/files` (provider files in OpenAI format, per-endpoint key routing), `/fine_tuning`, Assistants (deprecated, shutting down Aug 2026), `/evals`, code-interpreter containers, token counting.
- **Retrieval:** `/vector_stores` + search, `/rag/ingest`, `/rag/query` (all-in-one ingestion pipeline).
- **Agentic:** **MCP gateway (`/mcp`, 17 sub-endpoints)**, **A2A agent gateway (`/a2a`)**, Gemini managed agents (`/v1beta/agents`), Anthropic Skills API, `/memory` (user/team-scoped persistence), `/search` (14 variants).
- **Escape hatch:** 16 documented passthrough variants + config-defined custom passthrough endpoints (`/openai_passthrough` for assistants/threads/vector_stores; Vertex, Gemini, Cohere, Anthropic, Azure passthroughs) — passthroughs increasingly get cost tracking.
- **Structured outputs:** translates OpenAI `response_format`/`json_schema` to Anthropic `output_format` (auto-adds `structured-outputs-2025-11-13` beta header), Gemini `responseSchema`; optional client-side JSON-schema validation (`enable_json_schema_validation`).
- **Beta headers:** "Auto Sync Anthropic Beta Headers" feature; provider-specific beta-flag validation added after a public incident where Claude Code's beta headers were blindly forwarded to Bedrock/Azure/Vertex and caused request failures.

**LiteLLM weaknesses (from its own issue tracker):** SSE fidelity is its Achilles heel — output differs from upstream for `/v1/messages` and `/v1/responses` (#27442); Responses API streaming omits required event types (`response.created`, `response.output_item.added`) so strict clients reject deltas (#20975); usage lost when vLLM sends usage in trailing empty-choices chunk (#25389); tool_calls deltas dropped in mixed text+function streams (#17246); out-of-order `thinking_delta` via Bedrock crashing opencode (#3596); errors returned as SSE instead of JSON (#18756); stream cancellation loses usage chunk (#18887); silently dropped tool calls on `finish_reason: length`; anthropic-beta headers not forwarded to Bedrock (#15622) / Vertex (#15299); no first-class `gpt-realtime-whisper` realtime-transcription routing (#28535 — users bypass the gateway entirely); realtime transcription cost should be tracked per audio-minute not text tokens; browser WebSocket 1006 failures (#6825).

### 2.2 Portkey (TypeScript; fully Apache-2.0 OSS as of March 2026)

- Universal API across **1,600+ models**: chat, completions, embeddings, vision, **image generation**, **TTS + STT**, files, batches.
- **Realtime:** integrated WebSocket server proxying OpenAI Realtime API — full-duplex, with Portkey logging/cost on the session.
- **Fine-tuning:** unified fine-tuning API across providers (OpenAI, Azure, Bedrock, Fireworks…) — one of the few gateways treating fine-tuning as a first-class translated surface, not just passthrough.
- **Batch:** provider batch APIs **plus gateway-side "custom batching"** for providers without native batch — a genuinely distinctive feature.
- Files for fine-tune/batch workflows managed through the gateway.
- 50+ integrated guardrails run inline on the request path.
- Gaps: no video-generation endpoint; realtime limited to OpenAI (no Gemini Live); rerank not a first-class endpoint; MCP gateway is newer/less mature than its LLM side.

### 2.3 Bifrost / Maxim (Go, Apache-2.0) — the speed-first challenger

- `/v1`: models, chat, completions, **responses**, embeddings, **audio (speech + transcription)**, **images**, count-tokens, **batches, files, containers**, **MCP**.
- Drop-in **OpenAI- and Anthropic-compatible** (and Gemini) API surfaces; 20–23+ providers, 1,000+ models.
- Correctly returns `UnsupportedOperationError` when a modality isn't supported upstream (e.g., embeddings/speech/images on Anthropic) — explicit capability signaling rather than silent failure.
- MCP gateway with tool execution, "agent mode" and "code mode"; semantic caching; adaptive load balancing; cluster mode; virtual keys w/ budgets.
- **Perf claims:** ~11 µs added overhead at 5,000 RPS sustained; "50x faster than LiteLLM"; <100 µs overhead at 5k RPS (README).
- Gaps: no Realtime/WebSocket audio proxy; no video gen; no rerank endpoint; no fine-tuning surface.

### 2.4 OpenRouter (closed SaaS) — modality breadth on the hosted side

- Chat with multimodal **inputs: images, PDFs, audio, video files**; dedicated **STT transcription endpoint** (multiple providers); **image generation** models; **embeddings API** (recent — community confusion persisted into 2025 because it launched late); **`logprobs`/`top_logprobs` (0–20)** exposed as first-class params; structured outputs (JSON mode + strict schema) but **only on a subset of models** — users must check per-model support tables.
- No self-host option; no files/batches/fine-tuning surfaces; no realtime WebSocket proxy. Per-model feature support is the canonical UX answer ("check the models page"), which agents find awkward without a machine-readable capability API (they do expose model metadata via API).

### 2.5 Kong AI Gateway (Lua/Kong plugin, mixed OSS/enterprise)

- `route_type` taxonomy: as of **3.11+**, `llm/v1/chat`, `llm/v1/completions`, `llm/v1/responses`, `llm/v1/files`, `llm/v1/batches`, `llm/v1/assistants`, **`image/v1/images/generations`, `image/v1/images/edits`**, **`audio/v1/audio/speech`, `audio/v1/audio/transcriptions`, `audio/v1/audio/translations`**, plus `llm/v1/embeddings`.
- 3.10+ added native-provider format mode (`config.llm_format != openai`) — i.e., passthrough of Bedrock/Gemini native shapes.
- Strength: classic API-gateway policy machinery (authn, rate limiting, request transformation) applied per-route-type. Weakness: each modality is a plugin config exercise, not an out-of-the-box unified API; no realtime WebSocket story; no video gen; modality support trails OpenAI's release cadence by quarters (files/batches only landed in 3.11).

### 2.6 Envoy AI Gateway (Go/Envoy, CNCF, Apache-2.0)

- Supported endpoints (v0.5 docs): `/v1/chat/completions` (streaming, function calling, JSON schema), **native `/anthropic/v1/messages` incl. extended thinking**, `/v1/completions`, `/v1/embeddings`, `/v1/images/generations`, **`/v1/responses` (streaming + MCP tools + reasoning)**, **`/cohere/v2/rerank`**, `/v1/models`.
- **MCP Gateway** (v0.4+) with full OAuth and server multiplexing — strongest MCP-gateway security story of the infra-tier players.
- Honest per-provider support matrix (✅/⚠️ untested/🚧/❌): e.g., image gen is OpenAI-only; Anthropic Messages only on OpenAI/Anthropic/partially Azure; Bedrock embeddings still under development.
- Gaps: no audio (TTS/STT), no realtime, no batch/files/fine-tuning, no video. Kubernetes-native deployment only (Gateway API CRDs) — heavy for small teams.

### 2.7 Cloudflare AI Gateway (closed, edge SaaS)

- Unified API on api.cloudflare.com: any model from OpenAI/Anthropic/Google/Workers AI through one endpoint+auth (Aug 2025 refresh); dynamic routing; gateway-level retries.
- **Realtime WebSockets API: the broadest realtime coverage of any gateway — OpenAI Realtime API, Google Gemini Live API, plus Cartesia and ElevenLabs** speech models, speech-to-speech.
- Two WebSocket modes: realtime passthrough + a non-realtime WebSocket API for the unified API.
- **Unified Billing**: pay Cloudflare, no provider keys needed (5% fee on credits) — agents/orgs get one bill across providers.
- Caching, rate limiting, logs, guardrails at the edge. Gaps: no batch/files/fine-tuning surfaces; observability-first rather than full protocol translation; vendor lock to CF account model.

### 2.8 Vercel AI Gateway (closed SaaS, AI SDK-coupled)

- Capabilities: chat, **embeddings**, **image generation** (incl. multimodal output via `modalities: ['text','image']`, Gemini 2.5 Flash Image), OpenAI-compatible API.
- Tightly coupled to the AI SDK type system — great DX for JS/TS, thin for everything else. No realtime, no batch/files/fine-tuning, no audio endpoints, no rerank.

### 2.9 Helicone AI Gateway (Rust, OSS)

- Philosophy: **passthrough-first** — logs and routes but doesn't modify, so anything the provider supports works (including beta features) at "zero-latency overhead". 100+ providers without per-provider signup (PTB cloud gateway).
- **Realtime:** documented OpenAI Realtime API WebSocket monitoring — both text and audio modalities, configurable audio formats, cost monitoring per session.
- Tradeoff of passthrough purity: no unified translation for audio/image/etc. across heterogeneous providers — you get fidelity, not normalization.

### 2.10 TensorZero (Rust, Apache-2.0)

- Inference-focused: chat w/ tool use, **structured outputs (JSON) as typed "functions"**, **batch inference**, **embeddings**, multimodal inputs (base64 or remote files, any S3-compatible storage), caching.
- Distinctive: schema-first function abstraction + experimentation/optimization loop, <1 ms p99 gateway overhead claim.
- Gaps: no image/video/audio generation endpoints, no realtime, no files/fine-tuning passthrough, no MCP gateway.

---

## 3. Modality-by-modality state of the art

| Modality / surface | Best in class | Notes |
|---|---|---|
| SSE streaming fidelity | Helicone (passthrough), Bifrost | LiteLLM's normalization layer is the canonical cautionary tale (event-type omission, reordering, dropped tool deltas) |
| Native Anthropic `/v1/messages` | LiteLLM, Envoy AI Gateway, Bifrost | Now table stakes because Claude Code demands it w/ beta headers + extended thinking |
| OpenAI `/v1/responses` | LiteLLM, Envoy, Bifrost, Kong 3.11 | Hard part = exact SSE event-sequence reproduction; strict clients reject partial sequences |
| Realtime WebSocket (audio) | **Cloudflare** (OpenAI + Gemini Live + Cartesia + ElevenLabs), then LiteLLM (incl. WebRTC + realtime guardrails), Portkey, Helicone | Realtime **transcription** models and per-minute audio cost attribution remain unsolved everywhere |
| Batch APIs | LiteLLM, Portkey (provider batch **+ gateway-side custom batching**), Bifrost, TensorZero | Kong only since 3.11; cost attribution on async batch results is the hard part |
| Files APIs | LiteLLM (per-endpoint key routing), Portkey, Bifrost, Kong 3.11 | Mostly passthrough w/ auth + tracking |
| Fine-tuning | **Portkey (unified cross-provider API)**, LiteLLM (`/fine_tuning`) | Everyone else: nothing |
| Image gen/edit | Nearly universal (gen); edits = LiteLLM, Kong, OpenAI-only elsewhere | Multimodal *output* in chat (`modalities` param) only Vercel/OpenRouter |
| Video gen (Veo/Sora) | **LiteLLM only** (first-class `/videos` + async polling + cost + fallback across Sora/Kling/Veo/ModelsLab) | Biggest white space in the market |
| TTS / STT | LiteLLM, Portkey, Kong 3.11 (incl. translations), Bifrost, OpenRouter (STT endpoint) | Audio *translations* endpoint rare (Kong) |
| Rerank | LiteLLM (Cohere format), Envoy (`/cohere/v2/rerank`) | Most gateways skip it; RAG users scream |
| Embeddings | Universal, but OpenRouter only recently; token-array inputs still patchy (TensorZero discussion #4333) | |
| Logprobs | OpenRouter (first-class param), passthrough gateways | Translation gateways often drop/garble logprobs across providers |
| Structured outputs | LiteLLM (cross-provider translation OpenAI↔Anthropic `output_format`↔Gemini `responseSchema` + optional validation), TensorZero (schema-first) | Per-model support opacity is a universal complaint |
| Computer use / beta features | Passthrough gateways win by default | LiteLLM had a public incident: beta headers (computer-use, 1M-context) dropped or wrongly forwarded; Anthropic's Claude Code docs now explicitly require gateways to forward `anthropic-beta`/`anthropic-version` headers |
| MCP gateway | Envoy (OAuth + multiplexing), LiteLLM (17 sub-endpoints), Bifrost (agent/code mode) | The newest competitive front; converging with LLM gateways |
| A2A protocol | LiteLLM only (`/a2a`) | Early signal of agent-protocol expansion |
| Vector stores / RAG | LiteLLM only (`/vector_stores`, `/rag/*`) | Scope-creep question for a new gateway |

---

## 4. What users scream about (failure modes to design against)

1. **SSE non-fidelity is the #1 complaint class.** Missing event types, reordered thinking deltas, dropped tool-call deltas, usage chunks lost on cancellation or trailing chunks, errors emitted as SSE instead of JSON. Strict consumers (Responses API clients, opencode, Claude Code) hard-fail. Design rule: **byte/event-faithful proxy by default; transform only on explicit cross-protocol translation.**
2. **Header fidelity.** `anthropic-beta`, `anthropic-version` must pass through with per-provider validation (forward to Anthropic, strip/translate for Bedrock/Vertex). Computer-use, prompt caching, 1M context, structured outputs all ride beta headers.
3. **"Unsupported = silent failure."** Bifrost's explicit `UnsupportedOperationError` and Envoy's honest ✅/⚠️/🚧/❌ matrix are the right pattern; LiteLLM's "everything kind of works" generates ghost bugs.
4. **Realtime cost attribution.** Audio is priced per-minute, not per-token; no gateway tracks realtime transcription cost correctly today. WebRTC vs WebSocket vs browser-origin connections (1006 errors) all need first-class handling.
5. **Modality lag.** Providers ship new endpoints (Realtime transcription, Sora 2, video understanding) faster than translation gateways can model them — the only durable answer is a **cost-tracked passthrough escape hatch** plus rapid first-class promotion.
6. **Per-model capability opacity.** "Does model X via provider Y support structured outputs / logprobs / audio input through this gateway?" needs a queryable capability API, not docs tables.

---

## 5. Agent-experience (AX) observations

- Agent runtimes consume **native dialects** (Anthropic `/v1/messages`, OpenAI `/v1/responses`, Gemini `generateContent`), not the lowest-common-denominator chat API. A gateway that only speaks OpenAI-chat is invisible to Claude Code/Codex/Gemini CLI. Anthropic publishes explicit gateway-compatibility requirements for Claude Code.
- MCP gateway functionality (Envoy's OAuth+multiplexing, LiteLLM's `/mcp`, Bifrost's agent/code modes) is becoming the control surface through which agents discover and call tools — a new gateway should expose **its own admin/config plane as MCP tools** (none of the incumbents do this well; config is YAML/CRDs/dashboards).
- LiteLLM's A2A endpoint and Skills API passthrough signal the next protocol wave beyond MCP.
- Machine-readable capability discovery (per-model: modalities, logprobs, structured-output mode, max audio minutes) is the single highest-leverage AX feature missing from every incumbent.

---

## 6. Implications for a new OSS gateway (single binary, LLM+MCP)

**Table stakes:** chat/completions/embeddings/images/TTS/STT/responses + native Anthropic Messages, SSE streaming on all of them, batch+files passthrough, structured-output translation, rerank, models listing, MCP gateway.

**Steal these:** Cloudflare's multi-provider Realtime WebSocket coverage; LiteLLM's `/videos` abstraction with async polling + cost; Portkey's unified fine-tuning + gateway-side custom batching; Bifrost's explicit unsupported-operation errors and µs-class overhead; Envoy's per-provider honesty matrix and MCP OAuth; Helicone's passthrough-fidelity default; LiteLLM's beta-header auto-sync (done right, with per-provider validation).

**Win condition:** byte-faithful streaming + provider-dialect-native endpoints + cost-tracked passthrough for everything else + machine-readable capability API — i.e., never be the reason an agent's request breaks.

---

## Sources (primary)

- LiteLLM supported endpoints: https://docs.litellm.ai/docs/supported_endpoints ; realtime: https://docs.litellm.ai/docs/realtime ; videos: https://docs.litellm.ai/docs/videos ; Veo passthrough: https://docs.litellm.ai/docs/proxy/veo_video_generation ; beta-header incident: https://docs.litellm.ai/blog/claude-code-beta-headers-incident ; structured outputs: https://docs.litellm.ai/docs/completion/json_mode
- LiteLLM streaming-fidelity issues: BerriAI/litellm #27442, #20975, #25389, #17246, #18756, #18887, #25766, #15622, #15299, #28535, #6825; anomalyco/opencode #3596; google/adk-python #3181, #4482
- Portkey: https://github.com/Portkey-AI/gateway ; https://portkey.ai/features/ai-gateway ; realtime docs (gateway/features/realtime)
- Bifrost: https://docs.getbifrost.ai/overview ; https://github.com/maximhq/bifrost
- Envoy AI Gateway: https://aigateway.envoyproxy.io/docs/0.5/capabilities/llm-integrations/supported-endpoints/ ; release notes v0.4/v0.5
- Kong: https://developer.konghq.com/plugins/ai-proxy-advanced/ ; https://developer.konghq.com/ai-gateway/
- Cloudflare: https://developers.cloudflare.com/ai-gateway/usage/websockets-api/realtime-api/ ; unified billing; Aug-2025 refresh blog
- Vercel: https://vercel.com/docs/ai-gateway/capabilities (image-generation, embeddings)
- OpenRouter: docs for multimodal overview, audio, embeddings, parameters (logprobs)
- Helicone: https://github.com/Helicone/ai-gateway ; realtime blog
- TensorZero: https://github.com/tensorzero/tensorzero ; multimodal-inference docs; discussion #4333
- Claude Code gateway requirements: https://docs.anthropic.com/en/docs/claude-code/llm-gateway
