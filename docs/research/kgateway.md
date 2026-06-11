# Competitive Intel: Gloo / kgateway AI Gateway → agentgateway (Solo.io)

Researched 2026-06-10. Category: LLM gateway / AI-native gateway (K8s + standalone).

## TL;DR / Strategic picture

The "Gloo AI Gateway" lineage has gone through three incarnations, and you must track the **current** one:

1. **Gloo AI Gateway** (2024) — enterprise add-on to Gloo Gateway (Envoy + Go ext-proc). Prompt guards, semantic caching, RAG, token rate limiting. Enterprise-licensed, K8s-only.
2. **kgateway** (2025) — Gloo Gateway donated to CNCF (sandbox), Apache 2.0, Go control plane on Envoy. Carried the AI features in OSS for a while.
3. **agentgateway** (2025–2026, current) — a ground-up **Rust data plane** donated to the Linux Foundation/CNCF by Solo.io. As of kgateway v2.3.0, **all AI/agentic features were migrated out of kgateway into agentgateway**; kgateway is now "just" the API gateway, and agentgateway is the AI/LLM/MCP/A2A gateway. kgateway (and Solo Enterprise) can use agentgateway as the data plane via the same K8s Gateway API control plane.

So the real competitor to a "new open-source AI gateway (unified LLM gateway + MCP gateway, single binary)" is **agentgateway**: Rust, Apache 2.0, runs as a **standalone single binary** OR Kubernetes-native (Gateway API), with LLM gateway + MCP gateway + A2A gateway + inference routing in one proxy, an admin UI, an `agctl` CLI, and unusually agent-friendly docs (llms.txt, .md-suffixed pages).

Solo.io monetizes via **Solo Enterprise for agentgateway** (custom pricing, sales-led) focused on MCP security lifecycle, token exchange, audit trails, and 24/7 support.

---

## 1. Lineage & governance

- **Gloo** launched 2018 (Solo.io), production since 2019; "processes billions of API requests for many of the world's biggest companies."
- **kgateway**: CNCF project, Apache 2.0, Go (93%) on Envoy, K8s Gateway API implementation. ~5.6k GitHub stars. Positions itself as "the most mature and widely deployed gateway."
- **agentgateway**: Linux Foundation project (donated by Solo.io; co-announced with support from Microsoft, AWS contributions noted in community), Apache 2.0, **Rust (62%) + Go control-plane bits (26%) + TypeScript UI (9%)**. ~3.2k stars, 530 forks, Discord, public community meetings. Latest release at research time: v1.3.0-alpha.1 (May 2026). Built by the team behind Istio Ambient Mesh's ztunnel (Rust proxy pedigree).
- kgateway v2.3.0 release notes: "AI and agentic features migrated to the separate agentgateway repository; kgateway focuses exclusively on API gateway functionality." kgateway's own AI docs page now says "AI Gateway documentation is published on the agentgateway site."

## 2. Architecture & deployment model

- **Two deployment modes** with parallel doc trees:
  - **Standalone binary**: single Rust binary, static YAML/JSON config file (validated by a published JSON schema; docs include a "configuration schema explorer"). Runs on bare metal, VMs, Docker, K8s.
  - **Kubernetes**: kgateway-derived Go control plane programs agentgateway proxies over **xDS**; configured via **K8s Gateway API** (Gateway, HTTPRoute) plus CRDs: `AgentgatewayPolicy`, `AgentgatewayBackend`, `AgentgatewayParameters`. Helm/ArgoCD/FluxCD install paths.
- **Rationale for ditching Envoy** (their words): MCP's stateful bidirectional streaming mismatches Envoy's stateless design; traditional gateways are "body-blind"; Envoy AI filters required out-of-process Go ext-proc (latency + complexity). Rust = no GC pauses, deep body inspection in-process, native MCP/A2A.
- **Policy engine**: policies attach at gateway/listener/route/backend levels with documented processing order, targeting/merging semantics, and **conditional policies via CEL expressions** (CEL is pervasive: RBAC, rate-limit keys, transformations, Prometheus labels). Docs ship a **CEL playground** and interactive CEL context explorer.
- **Extensibility**: ExtProc (external processing) support; custom guardrail **webhooks** with a published OpenAPI spec; transformations via a templating language; Rust-based custom transformation extension story (CNCF blog, May 2026). No WASM plugin marketplace; no scripting plugin system like Kong/LiteLLM Python hooks.

## 3. LLM gateway feature surface (OSS agentgateway)

**Providers (first-class translation)**: OpenAI, Anthropic, Gemini, Vertex AI, Azure (OpenAI + AI Foundry), Amazon Bedrock, Ollama, vLLM, xAI (Grok), any OpenAI-compatible endpoint, multiple endpoints per provider; mock-LLM (httpbun) for testing. Older Gloo list also had Mistral/DeepSeek via OpenAI-compat.

**API types accepted at the front door**: OpenAI Chat Completions, OpenAI **Responses API**, Anthropic **Messages API**, **OpenAI Realtime (websocket)**, plus raw passthrough. (Notable: most OSS gateways only do chat/completions; Realtime and Responses support is ahead of many.)

**Routing & traffic**:
- Model aliasing; model failover (priority groups); load balancing across providers/endpoints; traffic splitting / canary / A/B by weights; **content-based routing** (route on request body content); locality-aware routing; retries with per-try timeouts; mirroring (standalone); Dynamic Forward Proxy.
- **Inference routing**: K8s Gateway API **Inference Extension** support — endpoint picking for self-hosted models by GPU/KV-cache utilization, LoRA adapter presence, prompt criticality, queue depth; KServe and vLLM Semantic Router integrations.

**Keys, cost, budgets**:
- Centralized provider credential management (K8s Secrets / backend auth config); passthrough of client tokens also supported.
- **Virtual keys**: per-consumer API keys with per-key token budgets + cost tracking — notably implemented as a *composition* of API-key auth + token rate limiting + CEL-keyed metrics, not a monolithic key-management subsystem (no key issuance UI/API like LiteLLM).
- Budget & spend limits; **token-based rate limiting** (local + global rate limit server); LLM cost tracking via Prometheus metrics tagged by user/model.

**Prompt handling**: prompt enrichment (inject/override system & user prompts per route), prompt templates, request/response transformations (incl. body field filtering, validation/defaults, LLM model headers).

**Guardrails** (OSS — this used to be Gloo-enterprise-only): regex filters (block/mask, e.g. PII patterns), **OpenAI Moderation API**, **AWS Bedrock Guardrails** (incl. masking), **Google Model Armor**, **Azure Content Safety** (recent), **custom webhook** guardrails (OpenAPI-spec'd request/response), and **multi-layered guardrail chaining**. Prompt-guard responses can reject or mask both prompts and responses, streaming-aware.

**Streaming**: SSE token streaming with provider-specific parsers (incl. Bedrock's non-SSE event framing); OpenAI Realtime websockets.

**Other**: function calling passthrough, OIDC browser auth, JWT auth w/ claim-based fine-grained control, CEL-based RBAC for LLM routes, CORS/CSRF, external auth (BYO ext-auth service, basic auth), access logging with CEL enrichment.

**Conspicuously absent vs old Gloo AI Gateway (Envoy/enterprise)**: **semantic caching** (Redis/Weaviate-backed) and **RAG-at-gateway** are not in agentgateway OSS docs/FAQ — apparent feature regression in the new stack (or reserved for enterprise roadmap). Also no built-in eval/playground, no batch APIs, no embedding-route specialization documented.

## 4. MCP gateway feature surface

- **Transports**: stdio, SSE, Streamable HTTP — gateway bridges between them (e.g. expose a stdio server over Streamable HTTP).
- **Static + dynamic MCP** backends; **Virtual MCP**: federate/multiplex many MCP servers behind one endpoint with tool aggregation, naming-collision handling, and per-tool exposure control ("tool access" lists).
- **OpenAPI → MCP conversion**: turn any REST API into MCP tools at the gateway.
- **MCP auth**: full OAuth 2.1 resource-server flow (JWKS validation), Keycloak/Auth0/Okta guides, JWT auth for MCP services, **fine-grained MCP authorization** (which user/agent can call which tool, CEL RBAC), MCP rate limiting.
- **Stateful MCP session management** (sessions by default; reviewer notes the in-process SessionManager doesn't distribute across replicas — scaling caveat).
- MCP observability page + metrics (sparse today, per reviewer).
- **A2A gateway**: agent-to-agent protocol routing, capability discovery, same policy stack applied to agent traffic.

## 5. Dashboard / UI / CLI / API surface

- **Admin UI** built in (TypeScript): visual config of listeners/backends/policies, explore agent-to-tool and agent-to-agent connections, an **MCP playground** to test tools, and LLM traffic views. (Local admin UI, not a multi-tenant SaaS console.)
- **`agctl` CLI**: inspect live gateway config (`agctl config all/backends`), **`agctl trace`** — trace a request through policy/filter processing for debugging. K8s + standalone.
- **Config-as-data**: standalone = one YAML file with JSON-schema validation; K8s = Gateway API + 3 CRDs. No REST admin/management API for CRUD of keys/routes (config is declarative files/CRs — GitOps-first, ArgoCD/FluxCD documented).
- Docs UX: every page available as raw markdown via `.md` suffix; full **`/docs/llms.txt`** index; CEL playground; interactive API/schema explorers.

## 6. Observability

- OpenTelemetry-native: metrics, **distributed tracing** (incl. standard OTEL env vars), trace headers generation; Prometheus metrics with **custom labels via CEL**; control-plane metrics + xDS NACK monitoring; access logs (CEL-enriched).
- LLM-specific: token counts, cost tracking by user/model; **Langfuse, LangSmith, Arize Phoenix** integration guides; Grafana/Jaeger guides.
- Gaps: MCP metrics are thin (request counters, no per-tool latency); third-party review found docs referencing metrics that don't exist in code.

## 7. Agent-facing posture (how AGENTS use it)

This is one of the most agent-forward gateways in the market:

- **llms.txt + .md-suffix doc mirror** — docs explicitly built for LLM consumption.
- **Tutorial: "Claude Code CLI proxy"** — first-class guides for pointing Claude Code, Claude Desktop, Cursor, Windsurf, GitHub Copilot, Continue, OpenCode, Antigravity IDE, Goose at the gateway (both as LLM proxy and MCP endpoint). Coding agents are treated as primary consumers.
- **kagent ecosystem**: Solo's CNCF agent framework includes a **kgateway-agent** (an AI agent that operates/debugs the gateway) — gateway-ops-by-agent is part of their story.
- The gateway itself *is* MCP infrastructure (federation, OpenAPI→MCP), so agents consume tools through it; but there is **no MCP control plane for configuring the gateway itself** (config is YAML/CRDs, not an MCP server exposing admin tools) — that's an open flank.

## 8. Performance claims

- **<1 ms p99 latency overhead at 10,000 QPS**; claimed **25–100× lower latency than LiteLLM** under high throughput; "significant gains in RPS, memory footprint, and stability vs LiteLLM proxies."
- Solo Enterprise marketing: "300× better memory utilization, 35× higher throughput, >100× latency reduction vs retrofit API-gateway approaches."
- **Open-sourced reproducible benchmark harness**: github.com/howardjohn/gateway-api-bench (v2) — they invite independent validation, which is a credibility differentiator.

## 9. Licensing / pricing

- agentgateway + kgateway: **Apache 2.0**, Linux Foundation / CNCF — strong neutrality story (not BSL, not open-core CRD tricks; enterprise features live in a separate Solo distribution).
- **Solo Enterprise for agentgateway**: separate license, custom sales-led pricing (no public list; comparable Envoy-gateway enterprise starts ~$19k/yr on AWS Marketplace per limited cores, scales up). Adds: full MCP security lifecycle (onboarding/registration, tool-server fingerprinting, versioning, runtime policy; protection against tool poisoning, rug-pulls, tool shadowing, naming collisions), secure elicitation extension to downstream SaaS (Google/GitHub/Salesforce), **automatic token exchange for least privilege**, "cryptographically verifiable" end-to-end audit trails, ambient waypoint deployment, 24/7 support, "Solo Labs for MCP" services program.
- Legacy Gloo AI Gateway (1.x): enterprise-only add-on license; prompt guards/PII/DLP/token rate limiting were paywalled there — now largely OSS in agentgateway (significant strategy shift to OSS-generous).

## 10. Weaknesses & complaints (third-party review + analysis)

- **Project churn/confusion**: three renames/replatforms in ~2 years (Gloo AI Gateway → kgateway AI → agentgateway); docs scattered across docs.solo.io, kgateway.dev, agentgateway.dev with version-specific trees; users must figure out which stack is current.
- **Pre-1.0 maturity**: latest is 1.3.0-alpha; reviewer found **docs describing features that don't exist in code and code features without docs** (e.g., phantom `list_calls_total` metric).
- **Stateful MCP scaling**: in-process session manager doesn't distribute across gateway replicas; needs consistent-hashing workarounds for HA.
- **Not a generic any-to-any translator**: only `/v1/chat/completions` and `/v1/messages` front-door routes get translation; **no structured-output support yet**; admits it's "not a full generic X→Y provider converter."
- **Semantic caching + RAG regression**: present in old Gloo AI Gateway (Redis/Weaviate), absent from agentgateway OSS docs/FAQ.
- **Thin MCP observability**: request counters only; no per-tool performance metrics.
- **No management/admin REST API or key-issuance plane**: virtual keys are hand-assembled from Secrets + CEL rate limits + metrics — fine for GitOps, hostile to self-serve teams; no end-user portal in OSS.
- **K8s-centricity of the full experience**: the rich policy/CRD/delegation model needs the Go control plane; standalone binary uses a different (file-based) config dialect — two config models to learn.
- **Inference-extension skepticism**: reviewer questions value of forwarding to external endpoint-picker schedulers that re-process the whole request.
- **Enterprise gate on the hardest MCP security problems** (tool poisoning/rug-pull defense, token exchange, verifiable audit) — OSS users don't get the headline MCP-security story.

## 11. Features worth stealing

1. **Single Rust binary that is simultaneously LLM gateway + MCP gateway + A2A + plain API gateway** with one policy engine.
2. **llms.txt + .md doc mirror + JSON-schema'd config + CEL playground** — agent-consumable surfaces everywhere.
3. **Virtual MCP**: federation/multiplexing of many MCP servers with per-tool access control + OpenAPI→MCP conversion.
4. **Guardrail composability**: regex + provider moderation (OpenAI/Bedrock/Model Armor/Azure) + custom webhook, chainable in layers, streaming-aware masking.
5. **CEL everywhere**: conditional policies, RBAC, rate-limit keys, metric labels, log enrichment — one expression language instead of N mini-DSLs.
6. **`agctl trace`**: trace a request through the policy pipeline for debugging.
7. **Front-door support for Responses API, Anthropic Messages, and OpenAI Realtime**, not just chat completions.
8. **Open reproducible benchmark harness** as a marketing/credibility asset.
9. **First-class "point Claude Code/Cursor/Copilot at the gateway" guides** — coding agents as a named user persona.
10. **Inference Extension routing** (GPU/KV-cache/LoRA/queue-depth aware) for self-hosted models.

## Sources

- https://github.com/kgateway-dev/kgateway · https://github.com/agentgateway/agentgateway
- https://kgateway.dev/docs/envoy/latest/ai/ · https://kgateway.dev/docs/envoy/2.0.x/ai/about/
- https://agentgateway.dev/docs/llms.txt (full doc index) · https://agentgateway.dev/docs/standalone/latest/faqs.md
- https://agentgateway.dev/docs/kubernetes/latest/llm/virtual-keys.md · /reference/release-notes.md
- https://docs.solo.io/gateway/1.21.x/ai/overview/ (legacy Gloo AI Gateway)
- https://www.solo.io/blog/why-traditional-gateways-failed-ai-workloads-and-how-kgateways-rust-powered-agentgateway-fixes-it
- https://www.solo.io/blog/introducing-solo-enterprise-for-agentgateway
- https://dev.to/spacewander/agentgateway-review-a-feature-rich-new-ai-gateway-53lm (independent technical review)
- https://www.truefoundry.com/blog/solo-ai-gateway-pricing-a-complete-breakdown-for-2026 (competitor pricing analysis; treat with skepticism)
- https://github.com/howardjohn/gateway-api-bench/tree/v2 (benchmark harness)
