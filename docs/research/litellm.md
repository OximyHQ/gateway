# LiteLLM (BerriAI) — Competitive Intelligence Report

**Date:** 2026-06-10
**Category:** LLM Gateway / AI Gateway (OSS incumbent)
**Company:** BerriAI (YC W23, founders Ishaan Jaffer + Krrish Dholakia)
**Repo:** github.com/BerriAI/litellm — ~50,000+ stars, 8,800+ forks, 1,350+ releases
**Language:** Python (~86%) + TypeScript (~13%, admin UI). FastAPI proxy, Prisma ORM + PostgreSQL, optional Redis.
**License:** MIT for core; everything under `enterprise/` is under a separate commercial license (open-core). Separate `litellm-enterprise` PyPI package.

---

## 1. What it is

Two products in one repo:

1. **Python SDK** (`litellm.completion()`): unified client for 100+ LLM providers in OpenAI format, with a `Router` class for client-side load balancing, retries, fallbacks, caching.
2. **Proxy Server / "AI Gateway"** (`litellm[proxy]`): self-hosted FastAPI server exposing OpenAI-compatible endpoints in front of all providers, with virtual keys, teams/orgs, budgets, rate limits, spend tracking, guardrails, logging callbacks, an admin dashboard (Next.js UI at `/ui`), MCP gateway, and A2A agent gateway.

Positioning (current): "unified gateway for LLMs, agents, and MCP — one endpoint for 100+ models, A2A agents, and MCP tools." They explicitly market that you don't need a separate MCP/agent gateway.

---

## 2. Full feature surface

### 2.1 Unified API endpoints (translation layer)
- `/v1/chat/completions` (~90 providers)
- `/v1/responses` (OpenAI Responses API, incl. MCP tool calling via `server_url: "litellm_proxy"`)
- `/v1/messages` (Anthropic-format endpoint usable against ~25 providers — i.e., Claude Code can point at LiteLLM and hit non-Anthropic models)
- `/v1/completions` (legacy text)
- `/v1/embeddings` (~30 providers)
- `/v1/images/generations` + edits (~15 providers)
- `/v1/audio/transcriptions`, `/v1/audio/speech` (~10 providers)
- `/v1/realtime` (WebSocket realtime API — GA'd v1.85.0, May 2026; benchmark median 59ms)
- `/v1/rerank` (Cohere-format rerank across providers)
- `/v1/batches` + `/v1/files` (OpenAI, Azure, Vertex, Bedrock, vLLM)
- `/v1/fine_tuning` (passthrough-style for supported providers)
- `/v1/assistants` (OpenAI/Azure assistants)
- `/v1/moderations`
- Vector stores / search APIs (added v1.79.0)
- `/a2a` (agent-to-agent protocol invocation)
- MCP endpoint (`/mcp`) — see §2.6
- Full Swagger/OpenAPI docs endpoint for the entire surface

### 2.2 Files & Batches (detail)
- File ops: `POST /v1/files`, retrieve content, delete; batch ops: create, retrieve, list, cancel.
- Providers: OpenAI, Azure, Vertex, Bedrock, vLLM.
- **Multi-account routing trick:** model credentials are *encoded into file/batch IDs* so follow-up calls route to the right provider account without env vars ("Managed Files," beta — "LiteLLM Managed Files with Batches").
- Batch **cost tracking is Enterprise-only**: logs batch creation, then aggregates usage from the output file when the batch completes.

### 2.3 Pass-through endpoints (native provider APIs through the gateway)
Mount native provider SDKs at `/{provider}/...` — no translation, native request/response, but with LiteLLM key auth, cost tracking, logging, and access control:
- Anthropic SDK, Vertex AI, Google AI Studio SDK, Bedrock (boto3), Azure, OpenAI, Cohere SDK, Mistral, vLLM, AssemblyAI, **Cursor Cloud Agents**, **Langfuse SDK** (pass Langfuse traffic through the gateway).
- Admins can define **custom pass-through routes** to arbitrary upstream APIs; "Passthrough Managed IDs" feature.
- Provider errors forwarded with original codes.

### 2.4 Auth, governance, multi-tenancy
- **Hierarchy:** Organizations → Teams → Users (internal) → Virtual Keys → End-users (customers, via `x-litellm-end-user-id`/`user` param).
- **Virtual keys:** master key generates keys via `/key/generate`; per-key `max_budget`, `budget_duration` (resets in s/m/h/d), `tpm_limit`, `rpm_limit`, model allowlists, key expiry/duration, metadata, aliases, temporary budget increases via `/key/update`.
- Budgets and rate limits at every level: org, team, team-member, user, key, end-user (customer), and per-model-per-key.
- **Tag-based routing & budgets** (route/spend by request tags).
- JWT auth (enterprise), OIDC; SSO for admin UI (Okta, Azure AD, Google WS, generic OIDC/SAML) — enterprise.
- Role-based access: proxy admin, org admin, team admin, internal user, viewer roles.
- Audit logs (enterprise) — exportable to S3/GCS/Azure Blob in Parquet with retention config.
- Secret managers: AWS Secrets Manager / KMS, Google Secret Manager, Azure Key Vault, Hashicorp Vault.
- IP allowlists, secret redaction in logs, custom branding (enterprise).
- Multi-pod budget accuracy work (v1.84.0) — distributed budget enforcement across replicas.

### 2.5 Routing & reliability (Router)
- Strategies: simple-shuffle (default, weight/RPM-based), latency-based, usage-based (lowest TPM), least-busy, cost-based, custom (`CustomRoutingStrategyBase` plugin).
- Retries with exponential backoff; cooldowns (`allowed_fails`, `cooldown_time`); deployment priority `order`; weighted failover within a model group before cross-group fallbacks (v1.86.0).
- Fallbacks: general, context-window-exceeded fallbacks, content-policy fallbacks.
- Pre-call checks: context-window validation, EU-region-only filtering (data residency), capacity exclusion.
- Timeouts per deployment; traffic mirroring (silent A/B); routing groups (different strategy per model); `caching_groups` for cross-model cache hits.
- Redis for cross-instance TPM/RPM accounting and cache.
- Load balancing multiple deployments under one `model_name` with `rpm`/`tpm` weights.

### 2.6 MCP Gateway
- Add MCP servers via UI or config.yaml; transports: **streamable HTTP, SSE, stdio**.
- Fixed single endpoint for all MCP tools; tools namespaced `server_name/tool` (SEP-986 compliant); MCP aliases; access-group name namespacing (v1.85.0); full rework of MCP access-group authorization with stateful/stateless routing (v1.88 RC, June 2026).
- Auth to upstream MCP servers: API key header, bearer, basic, custom header, **OAuth 2.0 PKCE (interactive) + client_credentials (M2M) with automatic token management**, **RFC 8693 token exchange (on-behalf-of)**, `delegate_auth_to_upstream` PKCE passthrough, **AWS SigV4** (Bedrock AgentCore), static headers, client-header forwarding.
- Access control by key / team / **org-level MCP server + toolset permissions** (v1.85.0); tool-level allowlists; **semantic tool filtering** (context-aware tool selection).
- **MCP cost tracking** per tool invocation, attributable to end-user budgets.
- Guardrails applied to MCP calls; health checks; MCP protocol version 2025-11-25 (as of v1.80.18); UI for OAuth-protected tool calls (v1.87.0).
- MCP tools usable from `/v1/responses` and auto-executed from `/chat/completions` (`require_approval: "never"`).
- Recommended split deployment: internal LLM-routing instance + internet-facing MCP-serving instance.

### 2.7 A2A Agent Gateway + Agent Hub
- Register agents (name + invocation URL) and invoke via A2A protocol through the gateway.
- Providers: native A2A, LangGraph, Vertex AI Agent Engine, Azure AI Foundry, Bedrock AgentCore, Pydantic AI.
- Logging, load balancing, streaming, **iteration budgets** for agents; per-agent permissions on virtual keys; cost attribution per agent; nested-call trace grouping via `X-LiteLLM-Trace-Id` / `X-LiteLLM-Agent-Id`.
- **Agent Hub / AI Hub** (v1.80.0): publish models + agents as an internal discoverable catalog ("make models and agents public for developers to discover").

### 2.8 Caching & cost features
- Response caching: in-memory, Redis, Redis-semantic (semantic caching), S3; per-key/per-request cache controls; TTLs.
- Provider prompt-caching support surfaced (OpenAI/Anthropic cache_control) with cache-aware cost tracking.
- Cost tracking on every request (token-level, includes cache discounts), per key/team/user/org/end-user/tag/model; spend logs table; `/spend` reporting endpoints; daily spend rollups; custom pricing for any model (`input_cost_per_token` etc.); **provider margins / fee markup** feature (resell with margin); pricing calculator endpoint.
- Budget alerts via Slack/webhooks; OpenMeter integration for usage-based billing; Lago/billing patterns documented.
- Prompt compression + memory API (v1.83.14) — store user preferences/memories across sessions.
- Prompt management integrations (Langfuse prompts, etc.) — serve prompts by ID from the gateway.

### 2.9 Guardrails
- Integrations: AWS Bedrock Guardrails (load-balanced across accounts), Presidio (PII masking, configurable entities), Lakera, Aporia, Guardrails AI, Azure Text Moderation / content safety, Cato Networks, generic "Guardrail API" interface, custom Python guardrails, **realtime-API guardrails** (v1.82.0).
- Modes: `pre_call`, `during_call` (parallel), `post_call`, `logging_only` (mask in logs only).
- Applied per-key, per-team (enterprise; non-overridable), per-model, or default-on; dynamic runtime params; system-message skip option; prompt-injection detection; secret redaction.

### 2.10 Observability / logging
- Callbacks to: Langfuse, OpenTelemetry (any OTLP collector; typed OTEL spans with metadata in v1.88), Datadog (logs + LLM Observability), Prometheus metrics (enterprise), Langsmith, Arize, Langtrace, Galileo, Lunary, MLflow, Helicone, Athina, Deepeval, Sentry (failures), Azure Sentinel, OpenMeter, plus raw sinks: S3, GCS, Azure Blob, AWS SQS, GCP Pub/Sub, generic custom API endpoint, custom async Python callbacks.
- `StandardLoggingPayload` (model, messages, response, tokens, cost, latency, metadata); message/response redaction (`turn_off_message_logging`, also per-request); per-key/per-team conditional logging; per-request callback disable header (enterprise); `x-litellm-call-id` for distributed tracing; Slack alerting (failures, slow responses, budget); claimed negligible perf impact for GCS/LangSmith logging.

### 2.11 Admin UI (dashboard)
- Login at `/ui` with master key or SSO; invite links for users.
- Manage: virtual keys, teams, orgs, users, budgets, rate limits, models (add/edit **without restart**, stored in DB via `store_model_in_db`), credentials, MCP servers, agents, guardrails, pass-through routes, router settings.
- Usage analytics: spend per key/team/model/user, activity graphs, logs viewer with request/response content, latency, cost attribution.
- Test playground (chat with models through the proxy); AI Hub catalog; projects management (v1.82.0); model hub page for developers; custom branding (enterprise).

### 2.12 Config & deployment model
- `config.yaml`: `model_list` (model_name + litellm_params + model_info), `router_settings`, `litellm_settings` (callbacks, drop_params, retries), `general_settings` (master_key, DB, alerting). Credentials via `os.environ/VAR` references; reusable `credential_list`; wildcard `model_name: "*"` and `provider/*` wildcard models.
- Runs as: pip package + `litellm --config`, Docker image (`-stable` tags get 12-hour load tests), Helm chart, Terraform/HCL bits in repo; DB-backed config optional (`store_model_in_db`) for UI-driven everything.
- Requires PostgreSQL for keys/spend (Prisma); Redis recommended for multi-instance; guidance: workers = CPU count; published infra sizing (Postgres 4–8 cores for 1–2K RPS; 16+ cores for 5K+).
- "LiteLLM Cloud" hosted option exists for enterprise; primarily self-hosted.

### 2.13 Agent-facing control surfaces (AX)
- **Everything in the UI is also an API**: management REST API (`/key/*`, `/team/*`, `/user/*`, `/organization/*`, `/model/*`, `/budget/*`, `/spend/*`, `/customer/*`) with Swagger — fully scriptable by agents.
- **`litellm-proxy` management CLI** (`uv tool install 'litellm[proxy]'`): models (list/add/update/delete), credentials CRUD, keys (generate with budget/duration, list, info, delete), users, chat completions from CLI, and generic raw HTTP command against any proxy endpoint. Auth via `LITELLM_PROXY_API_KEY`/`LITELLM_PROXY_URL` env vars or experimental browser SSO login.
- MCP gateway makes LiteLLM itself a tool source for agents; `/v1/messages` endpoint makes it a drop-in backend for Claude Code; A2A makes it an agent registry/router. Iteration budgets + per-agent spend = agent governance primitives.
- No first-class "manage LiteLLM itself via MCP" control plane found (agents manage it via REST/CLI, not via an admin MCP server) — a gap.

---

## 3. Published performance numbers

- Marketing claim: **"8ms P95 latency at 1k RPS"** (4 instances, 4 vCPU/8GB each... note: their own benchmark page shows P95 150ms total latency / 2ms median overhead at 1,170 RPS on 4 instances; 2-instance: 12ms median overhead, 1,035 RPS, P99 1,200ms).
- Realtime API: median 59ms, P95 67ms, 1,207 RPS.
- Stress-test claims (2026 blog series): 1,000 QPS no failures; up to 5,000 QPS on a single 4-CPU/8GB instance after a ~30% overhead-reduction sprint; "sub-millisecond proxy overhead" blog; ~90% TTFT reduction in streaming path (v1.87.0); ~30% cheaper per-chunk streaming (v1.88 RC).
- Their own comparison: beats Portkey on P95 (150 vs 230ms) and P99 (240 vs 500ms).
- Plan to keep Python for orchestration and offload hot path to a **sidecar** for performance.

## 4. Known criticisms & weaknesses

- **Performance ceiling (the big one):** independent load tests and user reports say the gateway degrades around **300–500 RPS** (P99 jumping to 90+ seconds); GitHub issue #21046 reports 1.7–4x throughput loss vs direct vLLM access on a 4vCPU box; Python GIL is the architectural critique competitors (Bifrost: "11µs overhead in Go") build their pitch on.
- **Resource hunger:** 350–400MB RAM baseline reported "excessive for a proxy"; 3+ second import/cold-start because `__init__.py` has 1,200+ lines importing every provider SDK.
- **Code quality reputation:** infamous July 2025 HN thread "LiteLLM is the worst code I have ever read in my life"; recurring complaints about bugginess, breaking changes, and enormous surface area; very high release cadence (1,350+ releases) cuts both ways.
- **Security record (2026):** PyPI **supply-chain compromise** — versions 1.82.7/1.82.8 backdoored via a poisoned Trivy CI dependency (TeamPCP, March 2026), credential-stealing payload on import; **CVE-2026-42271** command injection (CVSS 8.7) exploited in the wild and added to CISA KEV; **CVE-2026-42208** SQL injection exploited within 36 hours of disclosure. Major trust dent for a component that holds all provider keys.
- **Open-core friction:** SSO, audit logs, JWT auth, Prometheus metrics, per-team guardrail enforcement, batch cost tracking, per-request callback disable, etc. are paywalled ($250/mo Enterprise Basic; ~$30k/yr Premium).
- **Operational complexity:** needs Postgres + Redis + multiple instances + worker tuning to hit published numbers; config.yaml sprawl; DB-vs-yaml config duality.
- **Translation-layer correctness:** long tail of provider-mapping bugs/param drift inherent to maintaining 100+ adapters in Python.
- TCO critiques from vendors (TrueFoundry, Inworld) — "free software, you own the ops burden."

## 5. Pricing/licensing summary

- OSS core: MIT, free, self-hosted.
- Enterprise Basic: ~$250/month (SSO, audit logs, JWT, Prometheus, guardrail enforcement, Slack support).
- Enterprise Premium: ~$30,000/year (scale/compliance, custom SLAs, dedicated support).
- LiteLLM Cloud (hosted) available for enterprise customers.

## 6. Strategic takeaways for a new Rust/Go single-binary gateway

1. **Their moat is breadth** (100+ providers, every endpoint type incl. realtime/batch/files/passthrough) and ecosystem default-status — match the OpenAI + Anthropic `/v1/messages` + Responses surface first, not all 100 providers.
2. **Their soft underbelly is performance + footprint + security trust**: a single static binary with µs overhead, tiny RSS, instant cold start, and a clean supply chain directly attacks the 2026 narrative around LiteLLM.
3. **MCP gateway is now table stakes** and they're iterating fast (OAuth/OBO/SigV4/access groups/semantic tool filtering) — steal the auth matrix and per-tool cost tracking ideas.
4. **Pass-through endpoints with cost tracking** are an underrated differentiator (lets Claude Code/Cursor/native SDK traffic flow through governance) — replicate.
5. **Open-core resentment is real**: shipping SSO/audit logs/metrics free is an easy wedge.
6. **AX gap:** they have a REST API + CLI but no admin-MCP control plane; an agent-first gateway that agents can configure via MCP would leapfrog them.
