# Requesty — Competitive Intelligence Report

**Category:** Managed LLM gateway / AI router (SaaS) with an MCP gateway
**Researched:** 2026-06-10
**Primary sources:** requesty.ai (homepage, pricing, enterprise, observability, llms.txt), docs.requesty.ai (feature docs, integrations), github.com/requestyai, TrueFoundry & respan.ai comparisons, DataCamp tutorial, Requesty blog (funding, smart routing).

---

## 1. Company & Positioning

- **What it is:** A fully managed, closed-source AI gateway. Drop-in OpenAI-compatible endpoint (`https://router.requesty.ai/v1`, EU: `https://router.eu.requesty.ai/v1`) in front of **400+ models across 20+ providers** (homepage now says 400+; older docs/marketing say 150/160/300+ — catalog has grown over time).
- **Funding:** $3M seed led by 20VC, with Tapestry VC, Insiders Ventures, Tiny Supercomputer. CEO/co-founder: Thibault Jaigu.
- **Positioning:** "the Cloudflare for AI"; markets itself explicitly as the **enterprise/EU OpenRouter alternative** — GDPR, EU data residency (Frankfurt), contractual SLAs, governance. Dev-community wedge was coding agents (Cline, Roo Code, Claude Code users chasing cheaper/multi-model access).
- **History note:** "Requesty" name previously attached to a conversation-analytics product (visible in stale G2 reviews about analyzing customer messages) — the company pivoted to the gateway. Not to be confused with **Requestly** (BrowserStack's open-source API client).

## 2. Pricing & Licensing

- **Closed source.** No self-hosting, no on-prem, no open core. Managed cloud only (US Virginia, EU Frankfurt, APAC Singapore regions).
- **Pay-as-you-go: flat 5% markup on base model cost.** No subscription, no seats, no minimum spend. A $10/1M-token model costs $10.50 via Requesty.
- **Everything included on every plan:** routing policies, caching, fallbacks, spend limits, EU residency, observability, MCP gateway, BYOK, email support.
- **Enterprise (custom pricing):** SSO (Okta, Azure AD, Google Workspace via SAML/OIDC), full RBAC + audit logs, model approval whitelists/custom policies, team structures with granular spend controls, guardrails/PII at enterprise scale, **service accounts for CI/CD**, per-user MCP credentials, dedicated support + custom SLAs.
- No free tier advertised (credits-based onboarding historically).

## 3. Full Feature Surface

### 3.1 Routing & Reliability
- **Fallback policies:** Created in the dashboard ("Routing Policies"), invoked as `model="policy/<name>"`. Sequential failover on timeout/rate-limit/error; 0–10 retries per model with exponential backoff (500ms–4s) + jitter; **nested policies** (a chain can reference another policy); transparent to the app; **failed attempts are not billed**.
- **Load-balancing policies:** weighted distribution across models/providers.
- **Latency-based routing:** route to the currently fastest provider.
- **Smart Routing ("smarter-than-human model picking"):** an in-house ~65M-parameter transformer (distilled from ~50k annotated examples) classifies each prompt (code, chat, SQL, creative, reasoning…) in ~20–100ms (~50ms typical) and forwards to the optimal model. Claimed up to 80% cost savings. (The dedicated docs page now 404s; marketed via /solution/smart-routing — may be repackaged into policies.)
- **Failover speed claim:** provider outage → next-best option in **<20ms** (marketing; blog elsewhere says <50ms by design).
- **BYOK:** OpenAI, Anthropic, Google AI Studio, xAI only (no Vertex yet); **max one key per provider**; per-policy choice of Requesty keys vs own keys vs hybrid (e.g., Requesty primary, own key fallback).
- **EU routing:** dedicated EU endpoint, zero cross-border transfer claim.

### 3.2 Cost Controls & Governance
- **Spend limits / API limits:** per user, team, or API key; budget caps; rate limits; policies cascade org → group → key ("5-layer policy engine" per comparisons).
- **Approved Models:** org-wide whitelist with filters (provider, region, data policy, capabilities, release date), bulk actions, presets ("EU Only", "US Only"). Resolution: API-key list > union of group lists > org list; default-open until first approval, then default-deny. `/v1/models` returns only permitted models, so tools like Claude Code/Copilot automatically reflect restrictions — clever enforcement-via-discovery.
- **RBAC:** Owner / Admin / Member / Viewer; groups/teams with cascading budgets; Enterprise gets full RBAC + audit logs.
- **Guardrails:** bidirectional (request + response) detection/masking of PII (SSN, emails, phones, names), credentials/secrets (API keys, DB creds, service-account keys), financial data (cards, bank accounts). Toggle switches per guardrail type in console; org-wide, instant, covers all keys/models/endpoints incl. streaming. **No documented prompt-injection defense or custom guardrail rules.**
- Zero-data-retention option; SOC 2 Type II; GDPR; ISO 27001 "in progress (Q2 2026)"; DPA on request.

### 3.3 Model Capabilities (passthrough features)
Streaming (SSE), structured outputs, reasoning-token support, image generation, image understanding, PDF input, web search augmentation, embeddings, function/tool calling.

### 3.4 Caching
"Auto caching" / optimized prompt caching — claims 40–60% (sometimes "up to 80%" with smart routing) token-cost reduction; cache status surfaced in request logs. Comparisons mention "semantic caching."

### 3.5 Observability & Analytics
- Automatic request logging (model, tokens, latency, cost, cache status) — **no SDK needed**.
- Cost tracking in real time by model, team, user, API key; usage analytics; error rates; per-model/per-provider latency (TTFT, total, P50/P95/P99).
- **Request metadata:** `extra_body.requesty = {tags[], user_id, trace_id, extra{...}}` for cost attribution, user-journey analysis, workflow visualization.
- **Session reconstruction:** infers sessions from message-history overlap — groups multi-turn agent flows into conversations with **zero instrumentation**. Feeds conversation-length/topic analytics.
- Gaps: no documented alerting (comparisons claim "dynamic alerts" but docs don't), no documented log export, retention policy, or Datadog/OTEL forwarding.

### 3.6 MCP Gateway
- Unified MCP intermediary for coding agents (Claude Code, Cursor, Roo Code, any MCP client) at `https://router.requesty.ai/v1/mcp` with the **same Requesty API key** as LLM traffic.
- Dashboard flow: enable gateway → register servers from **templates (GitHub, Notion, Linear, Asana)** or custom JSON (URL, protocol, auth headers) → **explore + whitelist individual tools per server** → connect client.
- **Claude Code auto-discovers MCP servers via the Requesty key — zero extra config.**
- Auth: org-wide shared credentials (Standard) or per-user credentials (Enterprise, admin-toggleable). AES-256 at rest, TLS 1.3, org-level tenant isolation.
- Analytics: request volume, per-server latency, success rates, tool-usage frequency, per-user activity (Enterprise).
- **Limitations: HTTP-only (streamable-http + SSE); no stdio servers yet.** MCP usage billed under normal quota, no separate charge.

### 3.7 API Surface
- `/v1/chat/completions`, `/v1/embeddings`, `/v1/models` (permission-filtered), `/v1/mcp`. OpenAI-schema; adds a **`cost` (USD) field on the usage object** of every response.
- **Anthropic-compatible endpoint** (used by Claude Code via `ANTHROPIC_BASE_URL=https://router.requesty.ai`).
- OpenAPI spec published (`docs.requesty.ai/api-reference/openapi.json`).
- **Notable gap: no documented management/admin API** — policies, guardrails, approved models, keys, groups are all configured in the dashboard console, not via API/IaC. Big opening for an API-first/agent-first competitor.

### 3.8 Integrations & SDKs
- **Coding agents:** Claude Code, Cline, Roo Code, own VS Code extension, OpenClaw, Anthropic Agent SDKs; chat UIs: LibreChat, OpenWebUI.
- **Frameworks:** OpenAI SDK, LangChain, PydanticAI, Vercel AI SDK (official `ai-sdk-requesty` provider), Haystack, LlamaIndex TS, raw requests/axios; n8n node.
- **Claude Code analytics wrapper:** auto-tags sessions with git branch, repo, developer username, CC version via `X-Requesty-Branch`/`X-Requesty-Repo` headers, **stripped before forwarding to the provider** → per-branch/per-repo/per-dev cost reporting. Best-in-class coding-agent attribution.
- GitHub org (`requestyai`): only integrations/forks open-sourced (TS); also a "pi" AI-agent toolkit (coding agent CLI + unified LLM API + TUI/web UI) and a hermes-agent — early signs they're building their own agent harness. Core gateway not on GitHub.

### 3.9 Agent/AX-Specific Surfaces
- `requesty.ai/llms.txt`, `docs llms.txt` + `llms-full.txt`, OpenAPI JSON — deliberately machine-readable docs.
- **Open Data Research Catalog** (CC BY 4.0, with its own llms.txt "index for AI agents"): published gateway telemetry datasets — provider latency leaderboards, throughput density, TTFT vs total latency, cache hit rate by provider, finish_reason mix, tool-call token share, reasoning-token share, policy-vs-direct eventual success rates, provider error-code distributions. Unique credibility/SEO asset built from gateway exhaust.
- Public **LLM provider status** page and **LLM cost calculator** tools.

## 4. Published Performance Numbers (all self-reported/claims)

- Gateway overhead: **~8ms P50 ("Rust-based core")** vs OpenRouter "~40ms" (comparison-page claim); funding post says "50ms average added latency vs competitors' 200ms+"; blog: purpose-built gateways add 2–20ms. Numbers are inconsistent across their own materials.
- Failover: **<20ms** switchover (enterprise page), <50ms (blog).
- Smart-routing classification: ~50ms (20–100ms range), 65M-param in-house transformer.
- Uptime: **99.99% contractual SLA** (vs OpenRouter: none).
- Cost: caching saves 40–60%; smart routing + caching "40–80% API cost reduction"; "customers save $400k/yr"; 30–50% typical reduction.
- No independent third-party benchmarks found.

## 5. Weaknesses & Gaps

1. **Closed source, cloud-only** — no self-host/on-prem/VPC; explicitly confirmed "Self-Hosting: Not available." Hard disqualifier for many enterprises and the entire LiteLLM-shaped market.
2. **Cannot route to private/self-hosted models** (fine-tuned Llama/Mistral in your own infra) — external hosted providers only.
3. **5% markup on all traffic** (vs LiteLLM/self-hosted gateways at 0%); BYOK exists but limited to 4 providers, one key per provider, no Vertex.
4. **Dashboard-first configuration; no management API** — policies/guardrails/whitelists are clicked, not declared. No Terraform/IaC/GitOps story; weak for agent-driven control planes.
5. **MCP gateway is HTTP-only** — no stdio servers (most local MCP servers), limiting real-world MCP coverage.
6. Guardrails are regex/pattern-grade DLP — no prompt-injection defense, no custom rules, no LLM-based moderation documented.
7. **No environment isolation** (dev/staging/prod are organizational "groups," not infrastructure-isolated policy domains).
8. Governance covers the request path only — no batch jobs, deployments, or long-running-agent lifecycle.
9. Observability gaps: no documented alerting, log export, retention policy, or OTel/Datadog forwarding; analytics locked in their console.
10. API surface is thin: chat + embeddings only documented (no audio/transcription/rerank/images-API endpoints documented).
11. Small community footprint: near-zero Reddit/HN discussion found; OSS repos have 0–8 stars; brand confusion with Requestly and the old G2 listing; ecosystem trust vs OpenRouter is unproven.
12. Marketing-number inconsistency (300 vs 400+ models; 8ms vs 50ms overhead; <20ms vs <50ms failover) undermines credibility of perf claims.

## 6. What to Steal (for an OSS single-binary gateway)

- **Policies as virtual models** (`model="policy/name"`) — zero-SDK adoption of failover/LB/latency routing; plus **nested policies** and "no charge for failed attempts."
- **Approved-models enforcement through `/v1/models` filtering** so agent tools auto-honor governance without client config.
- **Claude Code git-context attribution** (branch/repo/dev headers, stripped before provider) → per-branch AI cost. Directly relevant to coding-agent fleets.
- **Zero-instrumentation session reconstruction** from message-prefix matching for agent-trace grouping.
- **MCP gateway with per-server tool whitelisting + org/per-user credential modes + MCP analytics**, same API key as LLM traffic, auto-discovery in Claude Code.
- **`cost` field in every response's usage object.**
- **llms.txt + llms-full.txt + OpenAPI** as first-class doc surfaces, and a **CC-BY open-data catalog of gateway telemetry** (latency leaderboards, error distributions, cache hit rates) as a community/credibility moat.
- Smart-routing via a small local classifier model (~65M params, ~50ms) as an opt-in "auto" model.
- Layered policy resolution (key > groups-union > org) with "approved list is the floor" semantics.

## 7. Differentiation Opportunities Against Requesty

- Open source + single binary + self-host (their #1 structural gap, and they price at 5% take).
- Declarative/API-first control plane (config as code, admin API, MCP-controllable management) vs their dashboard-only config.
- stdio MCP support + local model routing (Ollama/vLLM) — both absent.
- Exportable/OTel-native observability instead of a walled console.
- True environment isolation (per-env gateways/policies).
