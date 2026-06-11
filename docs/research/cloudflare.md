# Competitive Intelligence: Cloudflare AI Gateway

**Category:** Edge LLM gateway (proxy/control plane for AI traffic)
**Vendor:** Cloudflare (closed-source SaaS feature of the Cloudflare platform)
**Researched:** 2026-06-10

---

## 1. Positioning & Summary

Cloudflare markets AI Gateway as an "AI Application Control Plane" — a one-line-of-code proxy between your app and AI providers that adds observability (analytics, logs), reliability (caching, retries, fallback, timeouts), cost control (spend limits, unified billing, custom costs), routing (visual/JSON dynamic routing), and safety (Guardrails content moderation + DLP). It runs on Cloudflare's global edge network (300+ cities) and leans on existing Cloudflare primitives: Quicksilver (config KV distribution), Secrets Store (BYOK), Workers AI (Guardrails inference), Zero Trust DLP profiles, Logpush.

Strategic arc (2025→2026): from a passive observability proxy → a full control plane with its own unified REST API on `api.cloudflare.com`, credits-based unified billing across providers, dynamic routing flows, and dollar-denominated spend limits. It is converging on OpenRouter-style "one endpoint, any model" while keeping its infra-control-plane DNA.

**Not open source.** The gateway itself is proprietary Cloudflare edge infrastructure (no self-host option). The `cloudflare/ai` GitHub repo hosts SDK/tooling and issue tracking, not the gateway implementation. Cloudflare's edge stack is largely Rust/Workers-based, but no implementation language is published for AI Gateway specifically.

---

## 2. Full Feature Surface

### 2.1 Unified API / endpoints (the data plane)

- **New REST API (May 2026, flagship)** at `https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/...` — call ANY model (Cloudflare-hosted or third-party) with one Cloudflare API token. Four endpoints:
  - `/ai/run` — universal "envelope" endpoint (`model` + `input`), all modalities.
  - `/ai/v1/chat/completions` — OpenAI Chat Completions-compatible; point the OpenAI SDK's `baseURL` here.
  - `/ai/v1/responses` — OpenAI Responses API-compatible, explicitly aimed at **agentic workflows**.
  - `/ai/v1/messages` — Anthropic Messages API-compatible; point the Anthropic SDK here.
- Model naming: `author/model` (e.g. `openai/gpt-4.1`, `anthropic/claude-sonnet-4`) for third-party; `@cf/author/model` for Workers AI.
- Auth: a single Cloudflare API token with `AI Gateway` permission; provider keys come from BYOK store or unified-billing credits.
- `cf-aig-gateway-id` header attaches requests to a named gateway (applies that gateway's caching/limits/logging config). Required for Workers AI, optional otherwise. **Auto-creation**: using `default` as gateway ID provisions a gateway automatically with zero prior setup (Mar 2026).
- **Provider-native passthrough** (the older `gateway.ai.cloudflare.com/v1/{account}/{gateway}/{provider}` URLs): proxy to ~24 providers in their native API formats — Amazon Bedrock, Anthropic, Azure OpenAI, Baseten, Cartesia, Cerebras, Cohere, Deepgram, DeepSeek, ElevenLabs, Fal AI, Google AI Studio, Google Vertex AI, Groq, HuggingFace, Ideogram, Mistral, OpenAI, OpenRouter, Parallel, Perplexity, Replicate, xAI, Workers AI.
- **Universal Endpoint (now deprecated)**: JSON array of provider steps in one request body = declarative fallback chain; response header `cf-aig-step` tells which step served the request. Superseded by the OpenAI-compat endpoint + dynamic routing.
- **WebSockets APIs**:
  - *Realtime API* (Mar 2025): proxy provider-native realtime/multimodal WebSocket APIs (OpenAI Realtime, Google Gemini Live) — speech-to-speech, low latency.
  - *Non-realtime WebSockets*: swap `https://` for `wss://` on the universal endpoint for persistent connections to ANY provider (even ones without native WS support), avoiding repeated handshakes.
- **Translation layer**: automatic request/response normalization across providers so you can switch models without rewriting integration code.
- **Workers binding**: `env.AI.run()` (works for both `@cf/` and third-party models, with per-call gateway options: cache TTL, metadata, log collection), `env.AI.gateway(name)` instance with `getUrl()`, `getLog()`, `patchLog()` (attach feedback/score/metadata to a log), plus `env.AI.aiGatewayLogId`.

### 2.2 Reliability

- **Caching**: exact-match only (SHA-256 over provider + endpoint + model + auth headers + full request body). TTL 60s–1 month. Per-request headers: `cf-aig-skip-cache`, `cf-aig-cache-ttl`, `cf-aig-cache-key` (custom key override); `cf-aig-cache-status: HIT|MISS` response header. Claim: up to 90% latency reduction on hits. Caveats: cache is *volatile* (concurrent identical requests may both miss); text+image responses only; semantic caching announced as planned but still not shipped.
- **Retries**: gateway-level automatic retries on upstream failure (Apr 2026) — configurable `maxAttempts` (≤5), `retryDelay`, `backoff` (constant/linear/exponential); also per-request via `cf-aig-max-attempts`.
- **Request timeouts**: per-provider/step `requestTimeout` (ms); if the first chunk doesn't arrive in time, fall over to next step.
- **Model/provider fallback**: ordered fallback chains (Universal Endpoint array historically; dynamic routing flows going forward).

### 2.3 Traffic control & cost

- **Rate limiting**: per-gateway request-count limits, fixed or sliding window, configured via dashboard or API (`rate_limiting_interval/limit/technique`); 429 on exceed. **Gateway-scoped only — no per-API-key / per-user rate limits.**
- **Spend limits** (Jun 2026, open beta): dollar-denominated budgets that track cumulative spend and **block requests when exceeded**; scoped by model, provider, or custom metadata; daily/weekly/monthly windows; account-level caps on unified-billing spend too.
- **Dynamic routing** (Sep 2025): named, **versioned** route flows built in a visual editor or JSON. Node types: Start/End, Conditional (expressions over body/headers/metadata), Percentage split (A/B, gradual rollout), Model call, Rate-limit node (quota → fallback branch), Budget-limit node (cost quota → fallback branch). Routes are invoked by putting the route name in the `model` field (e.g. `model: "dynamic/support"`). Draft → deploy with instant rollback. Use cases: paid vs free user segmentation, A/B tests, cost-tiered fallbacks. Requires gateway auth + BYOK.
- **Unified Billing** (beta): buy Cloudflare credits, spend them at OpenAI, Anthropic, Google AI Studio, Vertex, xAI, Groq without per-provider accounts. **5% fee on credit purchase; provider list-price passthrough with no per-token markup.** Auto top-up. Zero-Data-Retention routing option for OpenAI/Anthropic. Workers AI billed separately.
- **Custom costs**: override per-model pricing with negotiated rates so analytics/cost numbers match your contracts.

### 2.4 Security & governance

- **Authenticated Gateway**: token-based auth (`cf-aig-authorization`) so only holders of a gateway token can use your gateway URL.
- **BYOK via Secrets Store** (Aug 2025): provider keys stored centrally with two-level AES-encrypted hierarchy and RBAC; referenced at request time so keys never travel in plaintext headers and are removed from app code.
- **Guardrails** (Feb 2025): inline prompt + response moderation powered by **Llama Guard 3 8B on Workers AI**; per-category flag-or-block (hate, violence, sexual content, criminal planning, self-harm, etc.); uniform across all providers. Latency cost ~**500 ms per evaluated request**; billed as Workers AI inference tokens.
- **DLP ("Firewall")** (Sep 2025): scans prompts/responses for PII, financial, health identifiers; 2 predefined profiles free, full profile library + custom profiles with a Zero Trust subscription; block or alert; DLP actions recorded in logs (policy IDs, matched entries).
- Adjacent (Cloudflare One, not AI Gateway proper): **MCP server portals** — Access-gated portal aggregating MCP servers with per-server OAuth; "Firewall for AI"; enterprise MCP reference architecture.

### 2.5 Observability

- **Analytics dashboard**: requests, tokens, cost, errors, latency per provider/model; dedicated AI sidebar in the Cloudflare dashboard (Feb 2026 refresh); queryable via GraphQL Analytics API.
- **Logging**: per-request log = prompt, response, provider, model, timestamp, status, token usage, cost, duration, plus DLP/guardrail actions. Plan-based storage caps (Workers Free: 100k logs total; Workers Paid: 10M per gateway); auto-delete-oldest option; filterable bulk delete; DELETE API. Per-request opt-out (`cf-aig-collect-log`) and **metadata-only mode** (`cf-aig-collect-log-payload: false`, Mar 2026) for privacy: keeps tokens/cost/model, drops payloads.
- **Logpush**: stream logs out to external storage — Workers Paid only; 10M requests/month included, then $0.05/million.
- **Custom metadata**: tag requests (user ID, team, plan, etc.) → drives analytics slicing, dynamic routing conditions, and spend-limit scoping.
- **Evaluations** (open beta): build datasets from filtered logs; human-feedback evaluator (`patchLog` score/feedback API); compare cost/speed/accuracy across models — more evaluator types promised.

### 2.6 Agent/AX surface (how agents use it)

- `/ai/v1/responses` endpoint explicitly targets agentic workflows; OpenAI/Anthropic SDK compat means agent frameworks work by changing `baseURL` only.
- **Cloudflare Agents SDK** integration: agents built on Workers/Durable Objects route model calls and **MCP tool calls** through AI Gateway automatically — "every action an AI agent takes through the MCP server is proxied by AI Gateway" (logging/caching/limits applied, one dashboard).
- **Cloudflare API MCP server**: the entire Cloudflare API (~2,500 endpoints, including AI Gateway CRUD/config) exposed to agents via just two tools, `search()` + `execute()` — agents can create/configure gateways conversationally.
- Full config API parity: gateways, rate limits, logging, dynamic routes manageable via REST API and **Terraform** (`cloudflare_ai_gateway` resource; Terraform provider auto-generated from OpenAPI). Wrangler/dashboards optional.
- Docs are agent-readable (Cloudflare publishes llms.txt / markdown variants of docs site).
- Zero-setup `default` gateway = agent can start proxying with no provisioning step.

---

## 3. Pricing & Licensing

- Core gateway (analytics, caching, rate limiting, proxying) **free on all plans**; no per-request gateway fee.
- Log storage: 100k logs (Free) / 10M per gateway (Workers Paid). Logpush: paid plan only, $0.05/M past 10M/mo.
- Guardrails: pay Workers AI token rates for Llama Guard evaluations.
- DLP: basic free (2 profiles); full library needs Zero Trust subscription.
- Unified Billing: +5% on credit purchases, list-price token passthrough.
- Enterprise: custom.
- License: proprietary, SaaS-only, no self-hosting.

---

## 4. Published performance claims

- Cache hits "reduce latency by up to 90%" and eliminate provider cost.
- Guardrails add ~500 ms per evaluated request (own docs).
- Third-party comparison (Respan) estimates 10–50 ms proxy overhead; Cloudflare publishes **no official latency-overhead or throughput numbers** for the proxy path itself — it leans on "300+ city edge network" branding.

---

## 5. Weaknesses & complaints (verified)

1. **Token/cost tracking gaps**: GitHub issues — streaming Azure OpenAI responses produce no token/cost data (usage in final SSE chunk ignored, cloudflare/ai#470); Anthropic `usage` object sometimes not parsed (tokens/cost logged as missing). For a product whose pitch is cost observability, this is a credibility wound.
2. **Exact-match caching only** — semantic caching "planned" since 2024, still unshipped; cache is volatile (concurrent misses) and limited to text/image.
3. **Rate limiting is gateway-global** — no per-key, per-user, per-team limits; granular control requires building dynamic-routing flows around metadata.
4. **No virtual keys / per-consumer credentials** — auth is one gateway token or Cloudflare API token; no built-in per-developer key issuance with budgets (contrast LiteLLM/Portkey).
5. **Log retention caps + stop-logging behavior**: hitting plan cap silently stops new logs unless auto-delete is on; shallow "deep observability" (no token-level tracing/spans; third-party reviewers call this out).
6. **No self-host / on-prem** — non-starter for VPC-isolated or data-residency-constrained buyers; all traffic transits Cloudflare.
7. **Guardrails latency** (~500 ms) is heavy for inline moderation; single moderation model (Llama Guard 3 8B), no pluggable judge.
8. **Config surface fragmentation**: three generations of data-plane APIs coexist (deprecated Universal Endpoint, provider-native URLs, new REST API) with different auth and URL schemes — migration confusion in community threads; Terraform support lagged the product (GH issue for missing resources).
9. **Unified billing 5% fee** + closed beta gating of newer features (spend limits beta, unified billing beta) — feature maturity churn.
10. **Vendor coupling**: best experience assumes Workers, Secrets Store, Zero Trust, Workers AI; standalone use loses DLP depth, bindings, and Guardrails economics.
11. **MCP gateway is a different product**: MCP portals/controls live in Cloudflare One (Access/Zero Trust), not AI Gateway — no single unified LLM+MCP gateway surface.

---

## 6. What to steal / counter (for a new OSS gateway)

- **Steal**: dollar-denominated spend limits that *block* (scoped by model/provider/metadata); versioned visual+JSON routing flows with draft/deploy/rollback invoked via the `model` field; `default` zero-setup gateway; metadata-only logging mode for privacy; `patchLog` feedback API feeding evaluations from production logs; per-request behavior headers (`cf-aig-*`); fallback-step response header; non-realtime WebSocket wrapper for any provider; whole-config-surface-as-MCP (search/execute pattern).
- **Counter**: be open-source + single-binary self-hostable (their structural gap); per-key/per-user rate limits & budgets; correct streaming token accounting from day one; semantic caching (they never shipped it); unify LLM + MCP gateway in one control plane (they split it across two products); no fee on BYO billing.

---

## 7. Key sources

- https://developers.cloudflare.com/ai-gateway/ (overview, features, providers, caching, rate limiting, dynamic routing, guardrails, unified billing, logging, evaluations, pricing, REST API, WebSockets, worker bindings)
- https://developers.cloudflare.com/changelog/product/ai-gateway/ (full changelog 2025–2026)
- https://blog.cloudflare.com/ai-gateway-aug-2025-refresh/ (unified endpoint, BYOK/Secrets Store, unified billing, dynamic routing, DLP)
- https://blog.cloudflare.com/ai-gateway-spend-limits/ (spend limits)
- https://blog.cloudflare.com/guardrails-in-ai-gateway/ (Llama Guard 3 8B)
- https://github.com/cloudflare/ai/issues/470 and cloudflare/cloudflare-docs#20536 (token/cost tracking bugs)
- https://developers.cloudflare.com/api/terraform/resources/ai_gateway (Terraform)
- https://blog.cloudflare.com/enterprise-mcp/ + Cloudflare One MCP portals docs (MCP story)
- https://www.respan.ai/market-map/compare/cloudflare-ai-gateway-vs-openrouter (third-party latency estimate, observability critique)
