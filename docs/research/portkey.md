# Portkey — Competitive Intelligence Report

Research date: 2026-06-10. Subject: Portkey (LLM gateway + MCP gateway + AI control plane).

## TL;DR

Portkey is the most "platform-complete" commercial AI gateway: a unified OpenAI-compatible API to 1,600+ models, JSON-config-driven routing (fallbacks, load balancing, conditional routing, circuit breakers), 50+ guardrails enforced inline, a full prompt-management studio, virtual keys evolved into an org-wide Model Catalog, deep observability (logs/traces/40+ metrics, OTel-compliant), and — since Jan 2026 GA — an MCP Gateway (registry, OAuth 2.1 + PKCE, per-tool RBAC, tool-call audit logs). In March 2026 they open-sourced their actual production gateway ("Gateway 2.0", Apache 2.0), collapsing the previous OSS/SaaS split. **On June 2, 2026 Palo Alto Networks announced acquisition of Portkey** (closed May 29, 2026) to make it the AI Gateway for Prisma AIRS — repositioning it as a security control plane for autonomous agents, and creating strategic uncertainty for neutral/indie users. This is a major market opening for a new independent OSS gateway.

## 1. Company / positioning

- Founded ~2023; "Production Stack for Gen AI Builders" → "Control Panel for Production AI" → (2026) "control plane for autonomous enterprise agents."
- Claimed scale: 2T LLM tokens/day, >$1M LLM spend routed/day, 24,000+ orgs, 400B+ tokens/day attributed via their pricing dataset, 200+ enterprises.
- Acquired by Palo Alto Networks (announced June 2, 2026; deal value undisclosed; closed May 29, 2026 per PANW press). Will become the AI Gateway inside Prisma AIRS ("monitor, route and secure every AI transaction"), following PANW's Protect AI and CyberArk acquisitions.
- Partnerships: F5 (2025, secure enterprise AI), AWS Marketplace listing.

## 2. Open source status

- Repo: github.com/Portkey-AI/gateway — ~12k stars, 1.1k forks, 81 releases, 3,457 commits.
- Implementation: **TypeScript (~96%)**; runs on Node (`npx @portkey-ai/gateway`), Docker, Cloudflare Workers, Replit; enterprise on AWS/Azure/GCP/OpenShift/Kubernetes. Local endpoint `http://localhost:8787/v1`.
- License history: gateway was MIT; **March 24, 2026: "Gateway 2.0" — the actual production codebase (previously split internal/public) was unified and open-sourced under Apache 2.0**, with previously SaaS-only features now free to self-hosters with no license keys:
  - Circuit breakers (P99 latency / error-rate config, probe-based recovery)
  - Semantic caching, budget limits, usage policies (token/cost enforcement at ingress)
  - Model catalog with pricing data
  - Real-time metrics (cost, latency, usage)
  - Metadata governance + config management
  - MCP Gateway (registry, OAuth 2.1 with PKCE)
- Still commercial (enterprise on-prem): gRPC, SSO, SCIM, AWS KMS, RBAC, JWT auth, audit logs, SOC2/GDPR/HIPAA compliance package, advanced PII redaction, managed observability persistence, support.
- Companion OSS: `Portkey-AI/models` — pricing/config dataset for LLM cost attribution (powers cost calc; notable reusable asset). Terraform provider (`Portkey-AI/terraform-provider-portkey`). Python + JS/TS SDKs.

## 3. Feature surface

### 3.1 Unified LLM gateway
- OpenAI-compatible universal API (REST + SDKs) to 1,600+ models across 45+ providers (OpenAI, Azure OpenAI, Anthropic, Bedrock, Vertex/Gemini, Cohere, Mistral, Together, Perplexity, Groq, Ollama, DeepInfra, Stability, Nomic, …).
- Multimodal through one signature: chat, vision, audio (TTS/STT), image generation; "gateway for other APIs" exposes rerank, video, files, batches, Responses API (enhanced Jan 2026), unified Rerank API.
- gRPC support (beta, enterprise) for lower-latency binary transport.
- Custom hosts: route to privately hosted / local models (Ollama, vLLM, etc.).
- Provider batch-API passthrough + custom batching for offline workloads.

### 3.2 Routing & reliability (the "Configs" system)
- **Configs** = versioned JSON objects defining routing strategy; attached per-request via header, per-API-key (enforced default configs), or in SDK init. Manageable via UI and API.
- Fallbacks (provider/model chains, trigger on configurable status codes).
- Automatic retries (up to 5, exponential backoff).
- Load balancing (weighted, across keys/providers).
- Conditional routing (route on metadata/custom conditions — e.g. user tier, region).
- Canary testing (percentage traffic to new models).
- Circuit breaker per strategy (P99 latency / error-rate thresholds, probe requests test recovery).
- Request timeouts (granular).
- Caching: simple (exact) + semantic, with TTL; reduces cost/latency.

### 3.3 Guardrails (50+)
- 20+ built-in deterministic checks: regex, JSON schema validation, code detection (SQL/Python/TS), word/sentence/character counts, URL checks; LLM-based: prompt-injection detection, gibberish detection; PII detection/redaction (response carries `transformed` flag).
- Partner integrations: Aporia, SydeLabs, Pillar Security, Azure (Shield Prompt, Protected Material), CrowdStrike AIDR, etc. — bring partner API key, enforce their policies inline.
- Custom webhook guardrails for in-house pipelines.
- Attach via `input_guardrails` / `output_guardrails` arrays in configs (or before/after request hooks).
- Orchestration semantics: async vs blocking, sequential vs parallel execution; failure actions: deny (HTTP **446**), process-anyway-with-flag (HTTP **246**), feedback-only (log verdicts for eval datasets). `hook_results` in responses show verdicts + execution times.

### 3.4 Virtual keys → Model Catalog
- Virtual keys (vaulted provider credentials with per-key budget/rate limits) upgraded into the org-level **Model Catalog**:
  - **Integrations** = secure credential vaults; one integration powers many provider slugs (`@openai-dev`, `@openai-prod`) with different governance.
  - **AI Providers** = governed slugs referenced in code as `@provider_slug/model_name`.
  - **Models** = gallery with slugs, token limits, pricing, code snippets; allowlist/denylist per integration; custom fine-tuned/self-hosted models with custom pricing.
  - Workspace provisioning: org admins grant teams specific providers/environments.
- Budget limits: USD or token caps with periodic resets. Rate limits: requests or tokens per minute/hour/day + concurrency caps.
- Vault integration (Mar 2026): fetch credentials at runtime from AWS Secrets Manager, Azure Key Vault, HashiCorp Vault via `secret_mappings`.

### 3.5 MCP Gateway (GA Jan 2026)
- Central proxy between MCP clients and MCP servers: authentication, access control, logging "without modifying your agents or servers."
- **MCP server registry**: org-wide catalog of approved servers/tools, version control + deprecation tracking, team-scoped access (anti-"shadow tooling").
- Auth: OAuth 2.1 + PKCE, API tokens, header-based, JWT validation, identity forwarding to downstream servers, BYO IdP (Okta, Entra), passthrough headers.
- Per-tool / per-server / per-team RBAC; credentials centralized, shareable across teams without exposure.
- Observability: unified traces of agent tool invocations, searchable audit logs per tool call, latency/error metrics per server.
- Policy enforcement on tool calls: PII redaction, content filtering, blocking unauthorized invocations pre-execution; usage policies (token/cost) at ingress; circuit breakers on MCP servers.
- Pre-integrated remote MCP servers (e.g., Atlassian) in docs.

### 3.6 Prompt Engineering Studio
- Playground across 1,600+ models, side-by-side comparison, multimodal, custom tool testing.
- Templates with variables (Mustache-style), partials (reusable components), folders/library with access controls.
- Versioning with rollback + labels; deploy via Completions and Render API endpoints (prompt ID referenced at request time — decouples prompt from code).
- Prompt observability: usage logs, performance metrics, version comparison. AI-assisted prompt improvement.

### 3.7 Observability
- Logs of every multimodal request/response (view, filter, debug, replay); request tracing across full lifecycle (incl. agent tool calls).
- Analytics: 21+ key metrics (Gateway 2.0 marketing says 40+) — cost, tokens, latency, error rates, cache hits, guardrail violations, per user/team/model.
- Custom metadata tags (org can enforce mandatory metadata JSON schemas on every request — strong cost-attribution story).
- Feedback API (values + weights) for quality signals; alerts (Pro+); data-lake exports (Enterprise); OpenTelemetry-compliant.

### 3.8 Governance / admin
- Org → Workspaces → Teams → Users hierarchy; RBAC; SSO, SCIM (enterprise); audit logs (enterprise).
- API key types: Service vs User; org-wide Admin API keys vs workspace keys.
- Enforced defaults: attach mandatory configs and metadata schemas to API keys at creation.
- Compliance: SOC2 Type 2, ISO 27001, GDPR, HIPAA, CCPA, custom BAAs.
- Deployment models: managed SaaS, hybrid (self-hosted data plane + managed control plane), fully air-gapped.

## 4. Agent experience (AX) — how agents use/control it

- **Agent Control Plane (Apr 2026)**: register any agent, swap base URL to Portkey endpoint → governed stack with no code changes (governance, 40+ metrics, fallbacks/LB, 50+ guardrails). Agent frameworks supported: Autogen, CrewAI, LangChain, LlamaIndex, Phidata, ControlFlow, custom.
- **Portkey CLI** (`npx portkey` / `npx portkey setup`): interactive wizard that fetches the workspace's MCP integrations and writes them into each coding agent's config (Claude Code, etc.), injecting the Portkey API key into headers — onboarding coding agents to governed MCP in one command.
- **Admin/Control-plane REST API** (`api.portkey.ai/v1`): programmatic CRUD on workspaces, configs, prompts, guardrails, virtual keys/providers, MCP servers (e.g. List MCP Servers endpoint) — the whole control plane is API-first.
- **Terraform provider** for IaC-managed gateway state.
- **Docs are agent-readable**: `portkey.ai/docs/llms.txt` and `llms-full.txt` published.
- Gateway 2.0 thesis explicitly reframes the gateway around agentic traffic (agents invoking tools, calling other agents, MCP) rather than chat request/response.
- Gap: no first-party MCP server *for controlling Portkey itself* found (control is REST/Terraform/CLI, not MCP-native) — an opening for an agent-first competitor.

## 5. Performance

- Self-published: "<1ms latency overhead" (internal benchmark), 122 KB bundle; "blazing fast"; 9.9% uptime claims aside, 2T tokens/day production scale is real social proof.
- Adversarial (Kong benchmark, EKS, 12 CPUs each, WireMock-mocked LLM, 400 VUs): **Kong 228% higher throughput than Portkey** (and 859% vs LiteLLM); **Kong 65% lower latency than Portkey** (86% vs LiteLLM). I.e., Portkey beats LiteLLM handily but a C/Nginx-based gateway beats the TypeScript runtime at raw proxying. Vendor benchmark — treat with caution, but the TS-runtime ceiling is plausible.
- gRPC (beta) is their answer for latency-sensitive paths; enterprise-only.

## 6. Pricing

- **Developer (free)**: 10k recorded logs/mo, 3-day log retention, 30-day metrics, fallbacks/LB/retries, 3 prompt templates, community support.
- **Production ($49/mo)**: 100k logs/mo, +$9 per 100k overage (to 3M), 30-day logs / 90-day metrics, alerts, LLM+partner guardrails, unlimited prompt templates, RBAC, service-account keys, simple+semantic caching.
- **Enterprise (custom)**: 10M+ logs, custom retention 90d+, custom guardrail hooks, SSO, granular budgets/rate limits, private cloud/VPC, data-lake exports, SOC2/GDPR/HIPAA, 24/7 support; reported real-world range $2k–$10k+/mo.
- **Billing unit is recorded logs, not gateway requests** — OSS self-host has no per-request fees.

## 7. Weaknesses / complaints

- Self-hosted OSS historically had broken/minimal logging (GitHub #1254: console logging broken, streams break logs); meaningful observability effectively required the managed tier or DIY logging infra. Gateway 2.0 narrows but doesn't fully close this (persistent observability layer still cloud).
- Raw proxy performance loses to compiled-language gateways (Kong benchmark above); TypeScript/Node runtime is the structural ceiling.
- G2/review complaints: initial complexity + learning curve for LLMOps newcomers; advanced analytics/visualization still immature vs older enterprise tools; documentation gaps in advanced areas; pricing high for small teams; product churns "almost every other day"; workspace-level security settings limited (only org-level).
- Until Mar 2026, OSS repo was a feature-limited shadow of the production SaaS (semantic cache, RBAC, model catalog all withheld) — eroded OSS trust; the 2.0 unification is an admission of that.
- **Palo Alto acquisition risk**: roadmap will bend toward Prisma AIRS security use cases; neutral-vendor buyers, indie devs, and OSS community face uncertainty (pricing, license direction, pace of OSS investment). Classic post-acquisition window for an independent alternative.
- Guardrail/PII enforcement adds inline latency on every request (LLM-based checks especially); blocking semantics (446/246 status codes) are non-standard HTTP.
- No single-binary story: Node/npm or Docker required; no Go/Rust static binary; config split across UI-saved Configs + Model Catalog + guardrail IDs can feel indirection-heavy ("id-xxx" references everywhere).

## 8. Differentiators worth stealing

1. **Configs as versioned, attachable JSON routing programs** — strategy (fallback/LB/conditional/canary/circuit-breaker) declared once, referenced by ID per request/key; enforceable as default on an API key.
2. **Guardrails as first-class inline pipeline** with deny/observe/feedback modes, async vs blocking, and explicit 246/446 semantics + `hook_results` audit payloads.
3. **Model Catalog**: org-level integration vault → governed provider slugs (`@openai-prod/gpt-5`) → per-team provisioning with budgets/rate limits; the cleanest virtual-key evolution in market.
4. **MCP Gateway registry + OAuth 2.1/PKCE + per-tool RBAC + tool-call audit logs** — earliest mature MCP governance plane; January 2026 GA.
5. **CLI that auto-wires coding agents** (`npx portkey` writes MCP config into each agent's config with keys injected).
6. **Mandatory metadata schemas** on every request for cost attribution/governance.
7. **Open pricing dataset** (Portkey-AI/models) powering cost attribution.
8. Billing on recorded logs (not requests), llms.txt docs, Terraform provider, runtime secret-fetch from KMS/Vault.

## 9. Implications for a new OSS gateway (single binary, LLM+MCP, agent-first)

- Portkey validates LLM-gateway + MCP-gateway as ONE product; their gap is being a Node service with cloud-attached observability, not a single binary, and now PANW-owned.
- Beat them on: single static binary, local-first full observability (no cloud tier needed for logs), compiled-language perf (Kong's numbers show the headroom), MCP-native control plane (control the gateway itself via MCP, not just REST), genuinely-open everything from day one.
- Match (table stakes): OpenAI-compatible universal API, fallbacks/retries/LB/conditional routing, virtual keys with budgets/rate limits, caching, logs/traces/cost analytics, guardrail hooks, prompt templating, OTel export.

## Sources

- https://github.com/Portkey-AI/gateway (README, discussion #1576, issue #1254)
- https://portkey.ai/docs/product/ai-gateway, /observability, /guardrails, /prompt-engineering-studio, /model-catalog, /administration, /mcp-gateway
- https://portkey.ai/features/mcp ; https://portkey.ai/pricing ; https://portkey.ai/blog/gateway-2-0/
- https://portkey.ai/docs/api-reference/admin-api/introduction ; https://portkey.ai/docs/guides/coding-agents/agent-cli ; https://portkey.ai/docs/llms.txt
- https://portkey.ai/docs/changelog/2026/january
- https://www.paloaltonetworks.com/company/press/2026/palo-alto-networks-to-acquire-portkey-to-secure-the-rise-of-ai-agents (+ completion press release, PRNewswire, pulse2, incyber)
- https://konghq.com/blog/engineering/ai-gateway-benchmark-kong-ai-gateway-portkey-litellm
- https://thenewstack.io/portkey-gateway-open-source/ ; https://www.truefoundry.com/blog/portkey-pricing-guide ; https://www.merge.dev/blog/portkey-vs-litellm ; G2 reviews
