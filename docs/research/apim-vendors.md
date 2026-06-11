# APIM Vendors Adding AI Gateway Features: Traefik Hub AI Gateway, Zuplo, Gravitee

Competitive-intelligence report for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).
Research date: 2026-06-10. Sources: official docs, vendor blogs/press, release notes, HN/community forums.

These three are classic API-management vendors retrofitting AI capabilities onto an existing gateway substrate. The common pattern: **AI gateway = a set of new policies/middlewares on the existing proxy**, plus an MCP story added in late 2025/early 2026. None ships an open-source AI gateway; all gate the AI features behind commercial tiers.

---

## 1. Traefik Hub AI Gateway

### What it is
Commercial (closed-source) Traefik Hub product line layered on the OSS Traefik Proxy (Go, MIT). Positioned as a "Triple Gate" architecture: **API Gateway + AI Gateway + MCP Gateway** in one runtime. Kubernetes-native: everything is configured via CRDs (`Middleware`, `IngressRoute`, `TraefikService`) and Helm (`hub.aigateway.enabled=true`, or `--hub.aigateway=true` flag). GitOps is the assumed workflow.

### LLM gateway feature surface
- **Providers**: any OpenAI-compatible chat-completions endpoint; verified: OpenAI, Azure OpenAI, Anthropic, Cohere, DeepSeek, Gemini, Mistral, Ollama, Qwen, Amazon Bedrock, plus local models via KServe/vLLM.
- **Routing**: `Model(<pattern>)` route matcher — routes on the `model` field inside the JSON request body. Max request body size enforcement (default 1 MiB; 413 on overflow).
- **Middlewares (the AI feature set)**:
  1. **Chat Completion** — turns a route into a chat-completion endpoint; enforces model/parameter controls.
  2. **Semantic Cache** — see below.
  3. **Content Guard** — Presidio-based PII detection with configurable rules and custom entities; filters both prompts and completions.
  4. **LLM Guard** — generic external-guard integration framework (see below).
  5. **Parallel LLM Guard** — runs multiple guard checks concurrently; total enforcement latency = slowest guard (announced March 2026).
- **March 2026 additions ("Triple Gate" expansion)**: Regex Guard (sub-millisecond pattern matching for SSNs/credit cards/proprietary formats), IBM Granite Guardian integration (hallucination detection + RAG quality assessment), multi-provider **failover routing** via circuit-breaker chains, **token-level rate limiting/quotas** per user/team/endpoint with proactive blocking, and **agent-aware error handling** — structured refusal responses returned as HTTP 200 instead of 403 so autonomous agent workflows can continue instead of crashing.

### Semantic cache (detail)
- Vector DBs: **Redis Stack, Milvus, Weaviate** (Oracle DB 23ai "coming soon").
- Vectorizers/embedding providers: OpenAI, Azure OpenAI, Gemini, Mistral, Ollama (local), Bedrock.
- Config: `maxDistance` similarity threshold, `ttl` (0 = permanent), `readOnly` mode (no cache writes — for staging), `allowBypass` (respect client `Cache-Control`), Go-template `contentTemplate` for extracting cacheable text from arbitrary JSON.
- Two variants: generic `semantic-cache` (any REST API, custom template) and `chat-completion-semantic-cache` (OpenAI-format aware, built-in message handling).
- Marketing claims: 40–70% cost savings; sub-10ms cached responses vs 3–10s LLM calls ("10–100x faster"); cache-poisoning-avoidance mode.

### Guard policies (detail)
- **LLM Guard** is an integration *framework*, not a built-in classifier: point it at Llama Guard, Llama Prompt Guard (BERT prompt-injection detection), any OpenAI-format endpoint, Ollama, or any HTTP service. Three guard-response formats (`ccr`, `responsesAPI`, `custom`).
- **Expression-based block conditions**: `JSONEquals()`, `JSONGt()`, `JSONStringContains()`, `JSONRegex()`, `Contains()`, logical operators — e.g. `blockConditions: condition: Contains("unsafe")` → 403. Separate `traceConditions` add OTel span attributes without blocking (observe-only mode).
- **Composable multi-vendor safety stack**: Microsoft Presidio (PII) + NVIDIA Safety NIMs (jailbreak detection, content safety across 22+ categories, topic control) + IBM Granite Guardian + custom regex, executed in parallel.
- Limitation: when response rules are configured, the entire stream must buffer before analysis — real-time streaming is lost.

### MCP Gateway (launched Oct 2025, Oracle AI World)
- Proxies MCP servers over **Streamable HTTP**; JSON-RPC introspection.
- **OAuth 2.0/2.1 Resource Server** implementation with auto-generated `/.well-known/oauth-protected-resource` metadata; mandatory upstream JWT middleware.
- **TBAC (Task/Tool/Transaction-Based Access Control)**: dual-layer — `listPolicies` filter what tools/prompts/resources are even *discoverable* via `tools/list`, while `policies` govern actual invocation, with JWT-claim expressions and `${jwt.field}` substitution, numeric transaction limits (e.g. cap a payment amount).
- Session affinity via Highest Random Weight hashing (stateful MCP sessions without sticky cookies, automatic failover).
- OTel metrics/traces auto-tagged with MCP session ID, method, tool name, resource URI, request arguments.

### Observability
OpenTelemetry-first: GenAI semantic-convention metrics and traces (token usage, operation duration), Prometheus/Grafana. No bundled AI-specific dashboard product — bring your own Grafana.

### Deployment / licensing / pricing
- Closed-source binary; OSS Traefik Proxy underneath (Go). Node-based + API-developer-portal-based licensing; pricing not public (sales contact). Free OSS proxy → paid API Gateway → paid API Management tiers.
- **Full offline/air-gapped deployment** across the whole platform (announced Oct 2025) — a differentiator for sovereign/military/regulated deployments.

### Weaknesses / complaints
- AI/MCP gateway features are entirely commercial; no self-serve pricing.
- Kubernetes-only ergonomics — CRD/Helm config model; weak story outside K8s.
- Long-standing community complaints about Traefik documentation quality and breaking changes between releases (HN threads); the label/CRD config model is easy to silently misconfigure.
- LLM Guard does no analysis itself — quality depends entirely on the external guard service you wire up; adds per-request latency hops.
- Guarded streaming responses must fully buffer.
- No published latency/throughput benchmarks for the AI gateway itself; cache claims are marketing-grade.

### Agent-experience (AX) notes
- Best-in-class **agent-aware refusal semantics**: structured HTTP-200 refusals so agent loops don't break on 403s — explicitly designed for autonomous consumers.
- TBAC transaction-attribute scoping (e.g., "agent may call `transfer` only below $500") is the most granular MCP authorization model among the three.
- Config surface is GitOps/CRD — machine-writable, but no CLI/MCP control plane for the gateway itself.

---

## 2. Zuplo (AI Gateway + MCP Server + MCP Gateway)

### What it is
Fully-managed, edge-deployed programmable API gateway (closed-source SaaS; runs on V8 isolates across ~300 edge locations; policies written in **TypeScript**; config lives in a git repo — GitOps native). AI Gateway launched 2025 ("AI Week"); MCP Gateway in private beta (2026). AI Gateway, MCP Gateway, and developer portals are **included in every plan** including free tier.

### LLM gateway feature surface
- **Providers**: OpenAI, Anthropic, Google (Gemini), Mistral, plus OpenAI-compatible custom providers. Chat completions, text completions, embeddings.
- **Unified endpoint**: one gateway URL per application; swap providers in config, not code. Zuplo-managed virtual API keys — provider credentials never reach clients.
- **Cost controls (flagship feature)**: **hierarchical USD budgets** — nest organizations → teams → sub-teams → apps, each with daily and monthly dollar budgets cascading top-down; requests hard-halt when budget is hit ("no overspend"), or warn-only mode.
- **Semantic caching**: vector-similarity response cache (managed; details thinner than Traefik's — no user-selectable vector DB).
- **Guard policies**: `prompt-injection-outbound` policy to block malicious instructions; PII-leakage prevention on requests and responses; described as "AI firewall policies." Because the whole gateway is programmable TypeScript, any custom guard logic can be written as a policy.
- **Observability**: per-call latency, tokens, cost; spend dashboards; streaming export of every request to **Galileo, Comet Opik**, or your own OTel collector.

### MCP story (two distinct products)
1. **MCP Server Handler** (GA): turn any OpenAPI spec/route into a remote MCP server at the gateway — tools, resources, and **prompts** derived from existing routes; custom tools with TypeScript handlers for multi-step workflows; same auth/rate-limit/audit policies as regular APIs; works with Claude Desktop, Cursor, ChatGPT, MCP Inspector.
2. **MCP Gateway** (private beta): the *consumption* side — governs employee access to third-party MCP servers from Claude/Cursor/ChatGPT. Core primitive = **virtual MCP server**: a curated view of an upstream exposing only chosen tools/prompts/resources at its own URL. Central catalog, per-role RBAC, SSO, audit log of every tool call.
   - Architecture: bidirectional OAuth intermediary — full RFC stack inbound (8414, 9728, 7591 dynamic client registration, 7636 PKCE, 8707, 6750); issues its own opaque tokens bound per-route; outbound supports per-user OAuth (individual consent, tokens encrypted at rest, auto-refresh) or shared admin-managed OAuth.
   - Deliberately **stateless**: Streamable HTTP, POST-only, no MCP sessions, no server-initiated notifications, no subscriptions — trades MCP-spec completeness for horizontal edge scaling.
   - Explicitly does NOT include: tools/list caching, native prompt-injection or PII scanning on MCP traffic, default rate limiting on OAuth endpoints (add via separate policies).

### Pricing / licensing
Free forever tier (no credit card); paid from $25/mo; Enterprise from ~$1,000/mo annual (99.5% SLA, 1M requests). Usage-based (requests), no per-seat fees. Closed source (docs portal Zudoku is OSS).

### Weaknesses / complaints
- Closed SaaS; self-hosting only via "managed dedicated"/enterprise arrangements — non-starter for data-sovereign buyers.
- Edge-isolate runtime imposes memory/CPU limits (competitors poke at "Zuplo's memory limits"); long-running/streaming-heavy AI workloads can hit platform constraints.
- Semantic cache is a black box vs Traefik/Gravitee (no vectorizer/vector-DB choice documented).
- Guardrails are thinner than Traefik's multi-vendor stack — one prompt-injection policy + PII policy + DIY TypeScript.
- MCP Gateway still private beta; stateless design drops parts of the MCP spec (notifications, sessions, sampling).
- Little independent community feedback (small footprint on Reddit/HN); most comparison content is Zuplo's own SEO.

### Agent-experience (AX) notes
- Everything-as-code in a git repo + TypeScript policies = very legible to coding agents; `zuplo` CLI + local dev server; gateway config is plain files an agent can edit and PR.
- OpenAPI→MCP autogeneration (incl. prompts and custom TS tools) is the smoothest "make my API agent-consumable" path of the three.
- Hierarchical dollar budgets are the cleanest agent cost-containment primitive seen anywhere — worth stealing.

---

## 3. Gravitee (AI Gateway / "AI Agent Management")

### What it is
Java-based (Vert.x) open-source APIM platform (community core Apache-2.0 on GitHub: `gravitee-io/gravitee-api-management`) with an Enterprise Edition. Rebranding around "The AI Agent Management Platform." The AI Gateway = three new native API types on the V4 gateway reactor: **LLM Proxy + MCP Proxy + A2A Proxy**, shipped across releases 4.8 (A2A) → 4.10 (LLM Proxy + MCP Proxy GA) → 4.11 (dashboards, AI PII filtering, semantic cache). Deployment: SaaS, self-hosted, hybrid.

### LLM gateway feature surface
- **LLM Proxy API type**: exposes an **OpenAI-compatible API** (`/chat/completions`, `/responses`, `/embeddings`) and translates to provider-specific formats. Providers: OpenAI, Anthropic, AWS Bedrock, Gemini, Vertex AI (Google + Anthropic publishers), OpenAI-compatible. Supports streaming, tool calling, temperature/top-p/stop/seed etc. (provider-dependent matrix documented).
- **Model routing**: automatic routing to the right provider/model from the request.
- **Key abstraction**: provider API keys held at the gateway; consumers authenticate with a single gateway-managed key (standard Gravitee plan/subscription machinery reused).
- **Policies applicable to LLM proxies**:
  - **Token Rate Limit policy** — rate limit on tokens, not requests.
  - **Guard Rails policy** — runtime detection of harmful/obscene/exploitative/policy-violating prompts; rejects before forwarding to the provider.
  - **AI-Powered PII Filtering policy** (4.11) — detects and redacts PII in payloads using **on-device AI models** (no external service call).
  - **AI Semantic Caching** (4.11) — returns cached responses for semantically similar prompts to cut token spend and latency.
- **Dashboards** (4.11): native **LLM Dashboard** (token usage, costs) and **MCP Dashboard** (request latency) inside the APIM Console — the only one of the three with a bundled AI-specific dashboard.

### MCP + A2A
- **MCP Proxy** (4.10): protocol-native API type proxying upstream MCP servers; introspects JSON-RPC 2.0 payloads to know which methods/tools/prompts are invoked; "Proxy and Studio modes"; method-level and resource-level access control; MCP Resource Server v2 (token introspection, certificate management). Also: expose existing REST APIs as MCP tools.
- **A2A Proxy** (4.8, dedicated V4 reactor in 4.11): governs google A2A agent-to-agent delegations with actor-aware token exchange — unique among the three.
- The gateway classifies incoming traffic as LLM request vs MCP tool call vs A2A delegation and routes to the protocol-specific proxy.

### Observability / governance
- OpenTelemetry traces enriched with **agent identity, tool usage, policy decisions, cost data**; connected traces across LLM+MCP+A2A workflows; cost attribution by team/model/use-case/agent; unified audit logging; OAuth 2.1 + PKCE; shared authn/authz across all traffic types.

### Pricing / licensing
- Community OSS APIM is free to self-host (Apache 2.0), but **the AI features (LLM Proxy, MCP Proxy, A2A, AI policies, dashboards) sit in Enterprise Edition packages** — EE is sold in three packages, pricing via sales; Gravitee Cloud is per-gateway priced, not public.

### Weaknesses / complaints
- AI gateway is effectively enterprise-only despite the OSS halo; "open source" applies to the legacy APIM core.
- Heavyweight platform: Java stack + MongoDB/JDBC + management API + console — community reports of slow management API (e.g., 7s responses breaking the Kubernetes operator), JDBC liquibase migration failures, deadlocks on upgrade, painful multi-component upgrades.
- Newest entrant on actual AI features (semantic cache and PII filtering only landed in 4.11, 2026); least field-proven.
- No published performance numbers for the LLM/MCP proxies; legacy gateway not known for raw speed.
- Configuration via APIM Console/management API — heavier ceremony than file-based GitOps (GKO operator exists but is a frequent pain point in the community).

### Agent-experience (AX) notes
- Strongest *conceptual* agent-governance story: single runtime that distinguishes LLM/MCP/A2A traffic and attaches agent identity to every trace; A2A governance is unique.
- But the control plane is console/UI-centric — no agent-first CLI or MCP control surface for configuring the gateway itself.

---

## Cross-cutting takeaways for our build

**Table stakes across all three** (the market floor for an AI gateway in 2026):
multi-provider OpenAI-compatible unified endpoint with streaming; provider-key vaulting + virtual consumer keys; semantic caching; prompt-injection/content guardrails; PII detection/redaction; token-aware rate limiting; per-team cost tracking; OTel GenAI traces/metrics; an MCP gateway with OAuth and tool-level allow/deny.

**Ideas worth stealing:**
- Traefik: parallel multi-vendor guard pipeline with expression block/trace conditions; TBAC transaction-attribute limits; HTTP-200 structured refusals for agents; air-gapped mode; pluggable vector DB + vectorizer for the cache.
- Zuplo: hierarchical cascading USD budgets with hard stop; OpenAPI→MCP (tools+prompts+custom code tools); virtual MCP servers (curated upstream views); full OAuth RFC stack with per-user token vaulting; everything-as-code TypeScript extensibility.
- Gravitee: token (not request) rate limiting as a first-class policy; on-device PII models (no external guard hop); bundled LLM/MCP dashboards; agent-identity-enriched traces; A2A governance.

**Shared gaps = our opening:**
- None is open source for the AI parts; none is a single binary; all are heavy (K8s CRDs / SaaS-only / Java platform).
- None has an agent-first control plane (CLI/MCP to configure the gateway itself).
- Guarded streaming is broken (buffering) in the only vendor that documents it.
- No vendor publishes real AI-gateway performance benchmarks.
