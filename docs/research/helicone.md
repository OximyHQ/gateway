# Competitive Intelligence: Helicone (AI Gateway + LLM Observability Platform)

Research date: 2026-06-10. Subject: Helicone (YC W23) — the Rust self-hosted AI Gateway (`Helicone/ai-gateway`) plus the Helicone observability/cloud platform (`Helicone/helicone`).

---

## 1. Executive Summary

Helicone started as a one-line-of-code LLM observability proxy and has evolved into a two-headed product:

1. **Helicone Cloud AI Gateway** (`https://ai-gateway.helicone.ai`) — the current flagship. OpenAI-compatible unified API to "100+ models" with provider routing, fallbacks, BYOK or prepaid credits with **0% markup** (explicitly positioned against OpenRouter's ~5.5%), prompt management executed inside the gateway, caching, custom rate limits, and observability built in. Lives inside the main `Helicone/helicone` monorepo (Apache-2.0, ~5.8K stars, actively developed as of May 2026).
2. **Self-hosted Rust AI Gateway** (`Helicone/ai-gateway`) — the "fastest, lightest" single-binary Rust gateway ("NGINX of LLMs"). ~600 stars, last release v0.2.0-beta.30 (July 2025), **last commit Nov 21, 2025 — which was a relicense from Apache-2.0 to GPL-3.0**. Effectively dormant since; the company's energy has clearly moved to the cloud gateway.

Strategic read: Helicone validated the Rust gateway technically (sub-1ms overhead, ~3K RPS, <100MB RAM, ~15-30MB binary) but pivoted monetization to a managed gateway + credits/passthrough-billing business on top of their observability platform. The GPL relicense of the Rust gateway is a defensive moat move and an opening for a permissively-licensed competitor.

---

## 2. Company / Project Facts

- YC W23. Open-source LLM observability platform; claims 2.1B+ requests / 2.6T+ tokens processed; customers from startups to Fortune 500.
- Main repo: `Helicone/helicone` — Apache-2.0, ~5,801 stars, pushed 2026-05-18, 107 open issues. Includes platform, cloud gateway code, `helicone-mcp`, cost package.
- Rust gateway repo: `Helicone/ai-gateway` — **GPL-3.0** (relicensed from Apache 2.0 on 2025-11-21, the repo's final commit to date), ~600 stars, 9 open issues, status "Public Beta", last release 2025-07-21.
- Languages: Rust (gateway, ~97% of ai-gateway repo); platform is TypeScript (Next.js dashboard, "jawn" Node backend, Cloudflare Workers proxy) over Postgres + ClickHouse + MinIO/S3 + Kafka (cloud).
- Other OSS: LLM Mapper, Helicone Prompts, `@helicone/mcp` npm package.

---

## 3. Self-Hosted Rust AI Gateway (`Helicone/ai-gateway`) — Full Feature Surface

### Core
- **Unified interface**: route to 100+ models across 20+ providers using OpenAI SDK syntax; transparent proxy — change only the base URL. Translation layer maps OpenAI-format requests to Anthropic, Google, Bedrock, Mistral, etc.
- **Single binary**: ~15-30MB, Rust + Tower middleware. Runs via Docker, Kubernetes, bare metal, `npx @helicone/ai-gateway` (npm-distributed binary), even as a subprocess. Cold start ~100ms.
- **Routers**: multiple named routers per deployment, each with its own balance/cache/rate-limit policy (e.g. `/router/prod`, `/router/dev` URL prefixes).

### Load balancing / routing strategies
- **Latency-based**: P2C (power-of-two-choices) + PeakEWMA over provider latency.
- **Model-based latency routing**, **weighted distribution** (explicit percentages per provider), and **cost-optimized** routing.
- Automatic failover/fallback chains on provider errors; health monitoring removes unhealthy providers.

### Rate limiting
- GCRA-based (smooth traffic shaping with burst tolerance) at global, per-router, and per-API-key levels; limits by request count, token usage, or dollar amount; Redis-backed distributed rate limiting (added via PR #182).

### Caching
- Exact-match request/response caching with Redis and S3 backends (and in-memory); claims cost/latency reduction "up to 95%"; bucketed cache (multiple stored responses per key) and seeds.

### Config & observability
- YAML config with sensible defaults; env vars for provider keys; validation endpoints; cloud UI wizard can generate config.
- Built-in Helicone observability integration plus **OpenTelemetry** (logs, metrics, traces).
- "Prompts" + dynamic routing + prompt templating landed in beta.29 (July 2025).

### Published performance numbers (their own k6 benchmarks vs mock provider, Fly.io)
- Sustained **3,000 RPS** on a single instance, 100% success (3 failures / 539,630 reqs), gateway overhead **<1ms** measured by distributed traces, P95 total 89ms (60ms mock provider latency), **<100MB memory**.
- Large-body test: 1,500 RPS, memory **<65MB**, ~60% CPU.
- Marketing claims elsewhere: ~1-5ms P95 overhead, "~10,000 requests/second", 8ms P50. Third-party roundups repeat "1-8ms overhead" and note LiteLLM's Python/GIL architecture degrades beyond ~500 RPS by comparison.

### Status / risk
- Public Beta, never reached 1.0. No commits since the GPL relicense (2025-11-21). Docs for the self-hosted config reference were removed from docs.helicone.ai (404s); self-hosted gateway docs now live only in the repo. Treat as de-prioritized/maintenance-mode.

---

## 4. Cloud AI Gateway (current flagship) — Full Feature Surface

### Endpoint & API surface
- Base URL `https://ai-gateway.helicone.ai`; OpenAI-compatible.
- REST endpoints: `POST /v1/chat/completions`, `POST /v1/responses` (OpenAI Responses API), `GET /v1/models`, `GET /v1/models/multimodal`; public model registry endpoint; OpenAPI spec published (`ai-gateway.openapi.json`).
- Concepts docs cover: reasoning models, prompt caching (provider-level cache_control), context editing, image generation, web search, Responses API — i.e., they normalize modern provider features across vendors.

### Provider routing (model-string DSL — notable design)
- `model: "gpt-4o-mini"` → auto-route to cheapest available provider.
- `model: "gpt-4o-mini/openai"` → pin provider (no failover).
- `model: "gpt-4o-mini/azure/<deploymentId>"` → pin a specific custom deployment (e.g., EU region for GDPR).
- `model: "gpt-4o-mini/azure,gpt-4o-mini/openai,gpt-4o-mini"` → explicit fallback chain.
- `model: "!openai,gpt-4o-mini"` → provider exclusion (multiple `!provider` allowed).
- Routing priority: 1) your BYOK keys → 2) Helicone-managed keys (credits) → 3) cost optimization w/ load balancing among equal-cost → 4) availability (skip providers with outages/rate limits).
- Auto-failover on 429, 401, 400 (context length), 408, 5xx.

### Billing models
- **Credits / passthrough billing**: prepay credits, pay exact provider rates, **0% markup** (only Stripe processing fees ~2.9%+30¢ on deposits). Explicit competitive positioning vs OpenRouter's 5.5%.
- **BYOK**: add provider keys in dashboard; always tried first; credits act as overflow/fallback.

### Gateway-native prompt management
- Prompts stored/versioned in dashboard; executed by passing `prompt_id` + `inputs` in the chat completion body — gateway compiles template server-side and forwards. Deploy prompt changes with zero code deploys.
- Variables `{{hc:name:type}}` (string/number/boolean/custom, type-validated); variables work in system prompts, messages, **and tool schemas**.
- **Prompt partials**: `{{hcp:prompt_id:index:environment}}` — compose prompts from other prompts.
- Environments (production/staging/dev/custom), per-environment versions, commit histories, tags, instant rollback. Full REST CRUD API (`/v1/prompt-2025/...` — ~20 endpoints). Playground editor for iteration. (Older prompts system is explicitly "Legacy".)

### Caching (cloud)
- Header-driven: `Helicone-Cache-Enabled`, `Cache-Control: max-age=...` (default 7 days, max 365), `Helicone-Cache-Bucket-Max-Size` (up to 20 variants, random serve), `Helicone-Cache-Seed` (namespace isolation, e.g. per-user), `Helicone-Cache-Ignore-Keys` (exclude fields from cache key). Response headers `Helicone-Cache: HIT/MISS` + bucket index. Served from Cloudflare edge (300+ PoPs).

### Rate limits & spend controls
- `Helicone-RateLimit-Policy` header: custom limits by requests, **cost**, or custom-property segment (per user / per property), independent of provider tiers.

### Gateway integrations (first-party docs)
- Claude Agent SDK (via MCP), OpenAI Agents SDK, OpenAI Codex, Claude Code, LangChain, LangGraph, LlamaIndex, DSPy, LiteLLM, Vercel AI SDK, Semantic Kernel, n8n, Zapier, PostHog, Langfuse (yes — they document sending gateway traffic to a competitor's tracing).

---

## 5. Observability Platform — Full Feature Surface

### Integration models
- **Proxy** (one-line base-URL change; Cloudflare Workers edge, claims ~0 added latency, 99.99% availability docs) or **async logging** (Manual Logger SDKs for TS/Python/Go/cURL, OpenLLMetry/OTel async, so Helicone is out of the critical path). They document the tradeoff explicitly (proxy-vs-async page).
- Direct integrations: OpenAI, Azure OpenAI, Anthropic, Gemini/Vertex, Bedrock, Groq, xAI, Llama, NVIDIA NIM/Dynamo, Ollama, OpenRouter, Together, Perplexity, Mistral, DeepSeek, Anyscale, Deepinfra, Hyperbolic, Novita, Nebius, Instructor, LiteLLM callbacks, Vercel AI SDK, CrewAI, Dify, PostHog, Xcode. OpenAI Realtime API + Responses API supported (claimed first observability platform to support Realtime).
- Beyond LLM calls: **vector DB tracing** and **tool-call tracing** via logger SDK/cURL (custom span types).

### Core features
- **Requests log**: full request/response bodies, cost, latency, TTFT, errors, streaming support; redesigned request drawer.
- **Sessions**: `Helicone-Session-Id` / `Helicone-Session-Path` (hierarchical `/parent/child` paths) / `Helicone-Session-Name` headers group LLM calls + vector DB + tool calls into agent-trace trees; session metrics, duration distributions by path, session replay cookbook.
- **Custom properties**: arbitrary key-values per request (headers) for segmentation (user, feature, environment); upsert property post-hoc via API.
- **User metrics**: per-user cost/request analytics.
- **Scores & feedback**: numeric scores + thumbs feedback per request/session via API; score distributions; eval REST API (`/v1/evals` create/query). LLM-as-judge evaluators + Ragas/promptfoo/OpenPipe integrations. (Evals/Experiments are comparatively shallow — the spreadsheet "Experiments" UI exists but reviewers consistently rank Helicone below LangSmith/Langfuse/Braintrust for eval depth.)
- **Datasets**: curate request data into datasets (fine-tuning export, OpenPipe integration).
- **Alerts** (Slack/email) and **Reports** (scheduled summaries).
- **Webhooks**: push events on request/score conditions, with local-testing tooling.
- **HQL (Helicone Query Language)**: direct ClickHouse SQL over `request_response_rmt` (timestamps, model, status, cost, tokens, custom properties); 300K row limit, 30s timeout, 100 q/min; REST endpoints `/v1/helicone-sql/execute|schema|download` + saved queries. Still gated to "selected workspaces".
- **LLM security**: prompt-injection/threat detection (header-enabled, Meta security models), moderations (OpenAI moderation pre-check), key vault (store provider keys, issue proxy keys), omit-logs options for privacy.
- **Cost tracking**: open-source cost package covering 300+ models; public LLM API pricing calculator (helicone.ai/llm-cost) — significant SEO asset.
- **Gateway/dispatch utilities**: retries with exponential backoff (header-enabled), fallbacks, load balancing at proxy layer.

### REST API surface (platform)
- Requests query (Postgres point + ClickHouse analytical), sessions query/metrics/feedback, users metrics, properties query, prompts 2025 CRUD, evals, dashboard scores, webhooks CRUD, trace log ingestion, feedback/scores. Swagger + OpenAPI published. Docs ship an `llms.txt` index (agent-readable docs).

---

## 6. Agent-Experience (AX) Surface — how agents use Helicone

- **`llms.txt`** machine-readable docs index at docs.helicone.ai/llms.txt; every docs page available as `.md`.
- **Helicone MCP server** (`@helicone/mcp`, in main repo): tools `query_requests` and `query_sessions` (full filter grammar: model/provider/status/time/cost/latency/custom properties, AND/OR combinators, pagination, optional bodies) — lets Claude Desktop/Cursor/Claude Code/Codex debug production LLM traffic conversationally. Plus `use_ai_gateway` tool so an agent can *make* LLM calls through the gateway (100+ models) from inside MCP.
- **Claude Agent SDK integration is MCP-first**, not base-URL-first: register `@helicone/mcp` as an MCP server; the agent self-directs routing, querying its own past requests, and analyzing its own performance/cost.
- Gateway works as drop-in for agent frameworks (OpenAI Agents SDK, Claude Code, Codex CLI, LangGraph, CrewAI) by base-URL swap; sessions headers give agent-step tracing.
- No agent-facing *control plane* (no CLI/MCP to create routers, change routing policy, manage keys, set rate limits — all of that is dashboard/REST only). Config-as-code exists only in the dormant self-hosted gateway (YAML).
- GitHub Actions cookbook for CI prompt testing.

---

## 7. Pricing

- **Hobby (free)**: 10K requests/mo, 1 seat, 1 org, 7-day retention, 10 logs/min, community support.
- **Pro $79/mo**: usage-based beyond 10K reqs + 1GB storage, unlimited seats, 1-month retention, 1K logs/min, alerts/reports/HQL, chat+email support. (Older $20/seat plan retired.)
- **Team $799/mo**: 5 orgs, 3-month retention, 15K logs/min, SOC-2 + HIPAA, private Slack.
- **Enterprise (custom)**: unlimited orgs, forever retention, 30K logs/min, SAML SSO, on-prem deployment, dedicated engineer, MSAs.
- Discounts: startups 50% off year 1, students free, nonprofits variable, $100/yr credit for OSS projects.
- **Gateway monetization**: 0% markup on credits (Stripe fees only) — gateway is a loss-leader/wedge for the paid observability platform; observability ingestion is the metered product.

## 8. Licensing / OSS posture

- Platform (`Helicone/helicone`): **Apache-2.0**, self-hostable (docker compose `helicone-compose.sh`, Kubernetes/Helm, all-in-one container; stack = Postgres + ClickHouse + MinIO; previously required Supabase — rewritten in 2024-25 to simplify).
- Rust gateway (`Helicone/ai-gateway`): **GPL-3.0 since Nov 2025** (was Apache-2.0). Dormant.
- Cloud-only or gated: HQL (selected workspaces), credits/passthrough billing, managed routing infra, SOC2/HIPAA attestations on paid tiers.

## 9. Weaknesses & Complaints (gathered)

1. **Self-hosted Rust gateway abandoned-in-place**: no commits since Nov 2025, perpetual beta, docs pulled from main docs site, relicensed GPL-3.0 — community trust risk and an adoption blocker for commercial embedding.
2. **Eval/experimentation depth**: consistently ranked below LangSmith/Langfuse/Braintrust for agent tracing depth and evaluation; positioned as "cost visibility" tool more than debugging/eval platform.
3. **Proxy-in-critical-path concern**: recurring HN/reddit question — proxy gives one-line setup but inserts a third party into request path (they mitigate with Cloudflare Workers + async logging option, but skepticism persists).
4. **Self-hosting the platform is rough**: GitHub issues on docker-compose/yarn build failures, ClickHouse migration-runner SyntaxErrors, worker→jawn auth failures ("Network connection lost"), all-in-one container exit 127; historical Supabase dependency bloat.
5. **Gateway translation gaps**: e.g. OpenAI `response_format` not translated to Anthropic structured output (passed through and ignored) — silent failure (issue #5639); retry logic ignores `Retry-After` and treats 429 like 5xx (issue #5672).
6. **Retention stinginess**: 7 days free / 1 month Pro / 3 months Team — short vs competitors; "forever" only on Enterprise.
7. **HQL gated** to selected workspaces; query limits (300K rows / 30s).
8. **Pricing jump**: $79 → $799 cliff between Pro and Team; SOC-2/HIPAA locked to $799+.
9. **Prompt-management churn**: full rewrite ("prompt 2025" API, old system labeled Legacy) — migration burden for early adopters.
10. **Cloud gateway lock-in tension**: best features (credits, registry-driven routing, prompt execution) are cloud-only; the OSS story for the *gateway* (vs observability) is now weak.
11. Changelog appears stale (no public entries after Nov 2025) despite active repo — weak public release-notes hygiene.

## 10. Published performance claims (for benchmark comparison)

- Gateway overhead <1ms (traced) / ~1-8ms claimed elsewhere; P95 <5ms overhead.
- 3,000 RPS sustained single instance (k6, mock provider), 1,500 RPS large-body; "~10K RPS" marketing claim.
- Memory ~64MB (<100MB under load); binary ~15-30MB; cold start ~100ms.
- Proxy platform: Cloudflare Workers edge, claimed 99.99% availability, "negligible" added latency.
- Caching: "up to 95%" cost/latency reduction claim.

## 11. Ideas worth stealing / implications for a new OSS gateway

- **Model-string routing DSL** (`model/provider`, `model/provider/deployment`, comma fallback chains, `!provider` exclusions) — zero-SDK, works from any OpenAI client, very agent-friendly.
- **Prompt execution inside the gateway** (`prompt_id` + `inputs` in the request body, partials, environments) — decouples prompt deploys from code deploys.
- **0% markup credits + BYOK-first priority routing** as a wedge against OpenRouter.
- **Header-based feature flags** (cache, rate-limit policy, retries, sessions, properties, security) — no SDK needed; everything controllable per-request by an agent.
- **MCP server over observability data + a `use_ai_gateway` MCP tool** — agents both consume the gateway and introspect their own traffic.
- **llms.txt + .md docs for every page**.
- **GCRA rate limiting with $-denominated limits**; cache buckets/seeds/ignore-keys.
- **Open cost model package (300+ models) + public pricing calculator** as both correctness infrastructure and SEO.
- Their gaps to exploit: permissive license single binary (theirs is now GPL + dormant), MCP *gateway* (they have none — only an MCP server for data), agent-driven control plane (CLI/MCP to manage routers/keys/policies), deeper evals, simple self-host.

---

### Key sources
- https://github.com/Helicone/ai-gateway (README, benchmarks/README.md, releases, LICENSE)
- https://github.com/Helicone/helicone
- https://docs.helicone.ai/llms.txt (full docs map), /gateway/overview, /gateway/provider-routing, /features/advanced-usage/caching, /features/advanced-usage/prompts/overview, /features/hql, /features/sessions, /integrations/tools/mcp, /gateway/integrations/claude-agent-sdk, /references/open-source, /getting-started/platform-overview
- https://www.helicone.ai/pricing, /changelog, /blog/introducing-ai-gateway, /blog/ptb-gateway-launch, /blog/self-hosting-journey, /credits
- HN: Show HN thread (news.ycombinator.com/item?id=42806254)
- GitHub issues: Helicone/helicone #5639 (Anthropic structured output), #5672 (Retry-After), #4583/#3549/#2965 (self-host docker), ai-gateway PR #182 (Redis rate limiting)
- Third-party comparisons: klymentiev.com/blog/llm-gateway-guide, techsy.io, dev.to gateway benchmarks at 5,000 RPS, zuplo.com buyers guide
