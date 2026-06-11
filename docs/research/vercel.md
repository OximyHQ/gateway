# Competitive Intelligence: Vercel AI Gateway

Category: Hosted LLM gateway (unified multi-provider inference API), deeply integrated with the Vercel AI SDK.
Researched: 2026-06-10. Primary sources: vercel.com/docs/ai-gateway (last_updated 2026-05/06), Vercel changelog, Vercel engineering blog, third-party benchmarks/reviews.

---

## 1. What it is

Vercel AI Gateway is a fully hosted, proprietary gateway at `https://ai-gateway.vercel.sh/v1` that fronts "hundreds of models" (200+ marketed) from ~45 providers behind one API key. It is the **default provider of the Vercel AI SDK** — passing a plain string model id like `anthropic/claude-opus-4.7` to `generateText()` routes through the gateway with zero config. Positioning: one key, unified API, automatic failover, spend monitoring, **zero markup on tokens** (including BYOK).

It is NOT self-hostable and NOT open source (the gateway service). The client SDK (`@ai-sdk/gateway`, part of vercel/ai) is open source (Apache-2.0, TypeScript). The gateway service itself is a Node.js/Next.js service running on Vercel's own Fluid compute with Redis state (per Vercel's "How AI Gateway runs on Fluid compute" blog).

There is no MCP gateway component — this is purely an LLM/inference gateway.

---

## 2. Feature surface (grouped)

### 2.1 Unified API / protocol compatibility
- **AI SDK v5 and v6** native integration (plain-string model ids; gateway is the AI SDK default provider; `globalThis.AI_SDK_DEFAULT_PROVIDER` override).
- **OpenAI Chat Completions** compatible endpoint (`/v1/chat/completions`) — works with the OpenAI Python/TS SDKs by changing `base_url`.
- **OpenAI Responses API** compatible endpoint.
- **Anthropic Messages API** compatible endpoint (this is what enables Claude Code passthrough via `ANTHROPIC_BASE_URL`).
- **OpenResponses** API (open variant of Responses).
- Model id convention: `creator/model-name` (e.g. `openai/gpt-5.5`, `xai/grok-4.3`); the same model can be served by multiple providers (e.g. Claude via anthropic, bedrock, vertex, claudeaws, azure).

### 2.2 Routing, reliability, failover
- Default routing: gateway **dynamically picks providers by recent uptime + latency**.
- `providerOptions.gateway.order` — explicit provider priority order.
- `providerOptions.gateway.only` — per-request provider allowlist (free).
- `providerOptions.gateway.sort` — rank providers by `cost`, `ttft` (latency), or `tps` (throughput): cost-based and perf-based routing as a one-liner.
- **Provider timeouts** — per-provider timeout to trigger fast failover when slow.
- **Model fallbacks** — model-level failover chains (try backup models when primary fails).
- Automatic retry to other providers of the same model on failure.
- **Team-wide provider allowlist** (dashboard setting; paid add-on $0.10/1k successful requests, Pro/Enterprise).
- **Automatic caching** (`caching: 'auto'`) — gateway injects provider-appropriate prompt-cache markers (Anthropic, MiniMax, etc.) so callers don't manage cache breakpoints; `supports_implicit_caching` exposed per endpoint.

### 2.3 Modalities / capabilities (all through one API)
- Text generation + streaming + tool use.
- **Reasoning** — normalized across OpenAI, Anthropic, Google, Vertex, Bedrock (per-provider `reasoningEffort`/`reasoningSummary`/`thinkingBudget` options).
- **Embeddings** (embedding model type in catalog).
- **Reranking** (rerank model type — for RAG re-scoring; multi-provider).
- **Image generation** (OpenAI GPT Image, Google Imagen, multimodal LLMs; image editing).
- **Video generation** (Google Veo 3.1, KlingAI motion control, Wan; text/image/video-to-video, resolution/duration/aspect/audio control) — unusual for a gateway.
- **Web search** — two modes: Perplexity Search bolted onto ANY model (provider-agnostic), or native provider search tools (Anthropic/OpenAI/Google). Billed per search call.
- **Service tiers** — OpenAI priority/flex processing passthrough for cost/speed tradeoff.

### 2.4 Auth & governance
- **API keys** — dashboard-created, never expire, per-key **spending budgets**; keys are deactivated when their creator leaves the team (a real operational footgun; docs tell you to use OIDC instead).
- **OIDC tokens** — automatic `VERCEL_OIDC_TOKEN` on Vercel deployments; zero secret management, auto-rotating, per-project.
- **BYOK** — team-level provider credentials (all projects) AND **request-scoped BYOK** (pass provider keys inline in `providerOptions.gateway.byok`); automatic fallback from your credentials to Vercel's system credentials on failure; zero fee on BYOK traffic.
- **Zero Data Retention (ZDR)** — gateway itself is ZDR by default (deletes prompts/responses after completion); optional provider-level enforcement: per-request `zeroDataRetention: true` (free, routes only to providers with verified ZDR agreements — covers Anthropic, OpenAI, Google, more) or team-wide ZDR ($0.10/1k requests, Pro/Enterprise).
- **Disallow prompt training** — per-request enforcement that providers don't train on your prompts.
- Budgets at team level (credits) and per-API-key.

### 2.5 Observability & reporting
- Dashboard Observability tab (team + project level): requests by model, TTFT, token counts, spend by model/time, full request traces.
- **Custom Reporting API** (`GET /v1/report`): aggregated spend grouped by `day/hour/user/model/tag/provider/credential_type/zero_data_retention/api_key_name`; filter by user, model, provider, byok-vs-system, ZDR flag, tags (any/all match). Requests carry `user`, `tags`, quota-entity IDs. Paid add-on: $0.075/1k attribute writes + $5/1k report queries. Not available on Hobby/Pro-trial.
- **Usage API**: `GET /v1/credits` (balance + lifetime spend), `GET /v1/generation?id=` (per-generation cost, TTFT, generation time, native/cached/reasoning token counts, finish reason, provider used, BYOK flag, billable web-search calls). Generation IDs returned on every response (`id` field, injected into first stream chunk, and `providerMetadata.gateway.generationId`). The generation endpoint deliberately **mirrors the OpenRouter schema**.
- **Live provider metrics in the catalog**: `GET /v1/models/{creator}/{model}/endpoints` returns per-provider uptime (15m/1h/1d), p50/p95 throughput (tps), p50/p95 TTFT latency, pricing, supported parameters — machine-readable, no auth required.

### 2.6 Model catalog / discovery (machine-readable)
- `GET /v1/models` — **no auth required**, OpenAI models-API format, includes context window, max tokens, type (`language|embedding|reranking|image|video`), capability tags (`reasoning`, `tool-use`, `vision`, `file-input`), full pricing including tiered pricing and cache read/write rates.
- AI SDK `gateway.getAvailableModels()` equivalent.
- Public model browser at vercel.com/ai-gateway/models with per-provider pricing comparison.

### 2.7 Ecosystem / integrations
- **Coding agents**: first-class docs for routing Claude Code (via `ANTHROPIC_BASE_URL`), OpenAI Codex (config.toml provider profile), OpenCode (native `/connect` integration with auto model discovery), Blackbox, Cline, Roo Code, Conductor, Crush, Grok Build, Superset. Pitch: one dashboard for agent spend, 200+ models in agents' pickers, fallbacks.
- **GitHub Actions**: `vercel/ai-action@v2` for AI in CI (PR review, release summaries).
- Framework integrations (LangChain, etc.) + **app attribution** (identify which app generated traffic).
- Python supported via OpenAI-compatible endpoint (no first-party Python SDK).

---

## 3. Pricing / licensing

- **Proprietary, hosted-only.** No self-host, no open-source server.
- Free tier: $5/month included credits (starts on first request), all models. Paid: pay-as-you-go credits, **zero markup** — provider list price passthrough; BYOK free.
- Add-on surcharges (off by default): Custom Reporting writes $0.075/1k + queries $5/1k; team-wide provider allowlist $0.10/1k requests; team-wide ZDR $0.10/1k requests (per-request variants of allowlist/ZDR are free).
- Auto top-up; user pays payment-processing fees (~3.2% card fee noted by third parties — the real monetization alongside ecosystem pull into Vercel hosting).
- `/v1/report` gated off Hobby and Pro-trial plans.

## 4. Performance

- Vercel claims: single-digit-millisecond gateway round-trip for most customers (~10ms control plane, "<20ms" marketed); anycast + PoPs + private backbone; billions of tokens/day; Fluid compute in-function concurrency (only 7.5% of runtime hours were active CPU).
- Independent benchmark (dev.to, vs native Anthropic SDK): ~200ms added overhead on small prompts (15-20% slower); negligible at 120K-token contexts; **gateway's own rate-limit tier absorbed every 120K-token request with zero 429s** (vs 4-minute waits on native Tier-1); BUT p99 TTFT spikes to 4.5x p50 (5.6s small-prompt, 6.7s large-context tails) vs native ~1.0-1.1x.

## 5. Weaknesses / complaints (third-party + inferred)

- Closed, hosted-only: no self-host path, vendor lock-in into Vercel billing/dashboard; less neutral than LiteLLM/Portkey for non-Vercel shops.
- p99 tail-latency unpredictability (up to 4.5x p50) vs direct SDK.
- ~200ms proxy overhead measurable on small/chatty workloads.
- Feature asymmetry: some capabilities (e.g. web search wiring, image/video `experimental_generate*`) work best/only via the TypeScript AI SDK; OpenAI-compatible endpoint is second-class for them ("a TypeScript SDK feature dressed up as a gateway feature" — folding-sky).
- "No fees" marketing vs fine print: ZDR/allowlist/reporting surcharges + ~3.2% card processing fee = trust gap.
- API keys die when their creator leaves the team; OIDC alternative only works on Vercel deployments.
- No guardrails/content-policy engine, no prompt management, no semantic caching, no A/B or conditional routing rules engine (routing is order/only/sort + fallbacks).
- No MCP gateway / tool-governance surface at all.
- No per-request virtual keys / multi-tenant key issuance for end users (budgets are per-API-key/team, not programmatic key minting).
- Custom reporting and allowlist/ZDR gated to Pro/Enterprise; Hobby can't query reports.
- Card-only payment; no crypto/invoice for small tiers.
- TrueFoundry review (competitor; conflates Vercel platform limits) cites function timeout ceilings and cold starts as agent-hosting constraints — applies to Vercel hosting more than the gateway itself.

## 6. Agent-experience (AX) notes

- Strongest AX story of the hosted gateways: unauthenticated machine-readable model catalog with pricing + live per-provider uptime/p95 latency/throughput; agents can do cost/perf-aware model selection without scraping.
- Coding-agent onboarding is two env vars (Claude Code: `ANTHROPIC_BASE_URL` + key) — protocol emulation (Anthropic Messages + OpenAI Responses) is the adoption wedge for agents, not SDKs.
- Generation IDs injected into the first stream chunk + `/v1/generation` lookup = agents can self-audit their own cost/latency immediately after each call.
- `/v1/credits` lets an agent check remaining budget programmatically; per-key spending budgets bound agent blast radius.
- Request-scoped BYOK + `tags`/`user` attribution let an orchestrator multiplex many downstream tenants/agents over one gateway.
- Gaps: no CLI, no IaC/API for gateway admin (keys, allowlists, BYOK config are dashboard-only), no MCP control plane, no webhooks/eventing.

## 7. What to steal / table stakes (for a new OSS gateway)

Steal: no-auth `/v1/models` with pricing+live health metrics; `sort: cost|ttft|tps` routing; generation-id-in-first-chunk + generation lookup API; `caching: 'auto'` provider-aware cache marker injection; request-scoped BYOK; per-request ZDR routing constraint; coding-agent onboarding docs as a first-class product surface; OpenRouter-schema compatibility for usage endpoints.
Beat it on: self-hosting/OSS, admin-plane API/CLI/MCP, guardrails, semantic caching, multi-tenant virtual keys, MCP gateway, tail latency.
