# Envoy AI Gateway — Competitive Intelligence Report

Date: 2026-06-10. Subject category: LLM gateway (Kubernetes-native, CNCF-adjacent). Researched for a team building a new open-source AI gateway (unified LLM + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

## 1. Identity & Positioning

- **What it is**: Open-source gateway for GenAI traffic, built as an extension layer on **Envoy Gateway** (which itself manages Envoy Proxy). Routes app→LLM-provider traffic with a unified OpenAI-compatible API, plus a full **MCP gateway** for agent→tool traffic.
- **Origin**: Co-founded by **Tetrate and Bloomberg** (announced 2024, v0.1.0 Feb 2025) explicitly to stop "everyone builds their own bespoke Envoy AI extension" fragmentation. Lives under the `envoyproxy` GitHub org; community-led with maintainers from Bloomberg, Tetrate, Google, Tencent, Nutanix, Netflix.
- **Governance move (2026)**: open proposal to move the project under the **Agentic AI Foundation (AAIF)** — positioning itself as the vendor-neutral governance/data-plane layer for enterprise AI agents (alongside MCP and Goose). No renaming or API breaks proposed.
- **License / language**: Apache 2.0. **Go** (control plane + external processor) on top of Envoy (C++) data plane. ~1.7k GitHub stars, ~270 forks, 97 contributors, 23 releases; latest **v0.7.0 (June 2026)**.
- **Commercial angle**: no pricing of its own (pure OSS). Tetrate monetizes via **Tetrate Agent Router Service (TARS)** and Agent Operations Director built on top; TARS is even listed as a "provider" in the gateway's own provider table.

## 2. Architecture & Deployment Model

- **Two-tier gateway pattern**: Tier 1 gateway = auth, top-level routing, global rate limiting; Tier 2 = fine-grained control of self-hosted model access with **endpoint-picker (InferencePool)** support for inference optimization.
- **Mechanics**: Envoy Gateway provisions Envoy Proxy; the AI-specific logic (request/response translation, token accounting, provider auth) runs as a Go **external processor (extproc)** attached via Envoy's ext_proc filter. Config is delivered as **Kubernetes CRDs** layered on the k8s Gateway API.
- **CRDs (v1beta1 as of v0.6.0 — first "production-ready" API surface)**:
  - `AIGatewayRoute` — model-aware routing (match on model name, headers, now hostname), unified API surface, `llmRequestCosts` token-cost extraction.
  - `AIServiceBackend` — a provider backend + its API schema (OpenAI / AWSBedrock / Anthropic / GCPVertexAI / GCPAnthropic / AzureOpenAI / Cohere).
  - `BackendSecurityPolicy` — upstream credentials (API key, AWS creds, Azure, GCP ADC/Workload Identity/service accounts, OIDC token exchange).
  - `GatewayConfig` — gateway-scoped extproc settings (env vars, resources) for multi-gateway deployments (added v0.5).
  - `MCPRoute` — MCP server multiplexing, tool selection, OAuth, authorization.
- **Deployment paths**:
  1. **Kubernetes** (primary): two Helm charts (CRDs + app) on top of an Envoy Gateway install; feature add-ons (rate limiting Redis, InferencePool) require extra Helm values files that patch the Envoy Gateway install.
  2. **Standalone CLI**: `aigw run` runs Envoy Gateway + Envoy + extproc **in a single local process, no Docker/Kubernetes** (Linux/macOS). Auto-configures from OpenAI SDK env vars (`OPENAI_API_KEY` etc.), serves at `localhost:1975`, admin on `:1064` (`/metrics`, `/health`). Marked **experimental**. Docker images published (`envoyproxy/ai-gateway-cli`).
- **Scaling**: multiple controller replicas + HPA documented; control plane benchmarked to **2,000 AIGatewayRoutes** with linear resource growth and zero routing failures.

## 3. Feature Surface (full enumeration)

### 3.1 Provider integrations (16–20+)
OpenAI, Azure OpenAI, AWS Bedrock (native: Converse + new InvokeModel for Claude, Titan embeddings), Google Gemini (OpenAI-compat endpoint only), Google Vertex AI (native), Anthropic (native /v1/messages), Anthropic-on-Vertex, Cohere (native v2 + rerank), Mistral, Groq, Grok (xAI), Together AI, DeepInfra, DeepSeek, Hunyuan, Tencent LLM Knowledge Engine, SambaNova, TARS, and any self-hosted OpenAI-compatible server (vLLM etc.).
- Two integration modes: **native schema translation** (OpenAI↔Bedrock, OpenAI↔Vertex/Anthropic, etc.) vs pass-through to OpenAI-compatible endpoints with path prefixes.
- **Vendor-specific fields**: inject backend-specific params into OpenAI-shaped requests.
- **Cross-schema tricks** (v0.6–0.7): clients can call **Anthropic `/v1/messages` against any OpenAI backend**; Anthropic Messages → Bedrock Converse translation; unified `reasoning_effort` knob mapped across Anthropic/OpenAI/Gemini; provider-agnostic **prompt caching** via unified `cache_control` (Bedrock, GCP Claude, Gemini prefix caching); Google Search grounding for Gemini.

### 3.2 Supported API endpoints (unified surface)
`/v1/chat/completions` (streaming + tools + JSON schema), `/v1/completions` (legacy), `/anthropic/v1/messages` (extended thinking, streaming; Anthropic/GCP/AWS), `/v1/embeddings`, `/v1/images/generations`, `/v1/audio/transcriptions`, `/v1/audio/translations`, `/v1/responses` (OpenAI Responses API incl. MCP tools, reasoning, multimodal; Azure too in v0.7), `/cohere/v2/rerank`, `/v1/models` (lists configured models). Notable: **no batch API yet** (on roadmap).

### 3.3 Traffic management
- **Model virtualization**: virtual model names mapped to provider models; model-name-based routing via `x-ai-eg-model` header extraction; v0.7 adds **hostname-based model scoping** (different model sets per tenant hostname).
- **Provider fallback**: priority-ordered `backendRefs` (priority 0 = primary); retries via Envoy Gateway `BackendTrafficPolicy` (retriable status codes, connection failures, exponential backoff 100ms→10s, per-retry 30s timeout, `numAttemptsPerPriority`). Health-check details thin in docs; streaming-failover semantics undocumented.
- **Token-aware (usage-based) rate limiting**: `llmRequestCosts` extracts InputToken / CachedInputToken / OutputToken / TotalToken or a **custom CEL expression** (e.g. discounted cached tokens, 1.5× output weighting) into per-route metadata; enforced via Envoy Gateway Global Rate Limit (**requires Redis**), keyed on arbitrary headers (e.g. `x-tenant-id`) × model. Checks happen at request receipt against accumulated usage (debit model: a request that would exceed gets 429). Only parses OpenAI-schema responses for usage.
- v0.7: **backend quota rate-limit filter injection** for quota-aware routing.
- **Header & body mutations** per route/backend (request body field mutation added v0.5).
- **InferencePool support** (Gateway API Inference Extension): endpoint-picker-based intelligent load balancing to self-hosted model replicas; works with plain HTTPRoute or AIGatewayRoute.

### 3.4 Security & governance
- **Upstream auth**: API keys from k8s Secrets; AWS credentials/IRSA; Azure credentials; GCP ADC, service accounts, Workload Identity Federation (GKE Workload Identity in v0.6); OIDC token exchange.
- **Client-side auth/authz**: inherited from Envoy Gateway `SecurityPolicy` (JWT, OIDC, ext-auth, API key auth) — not AI-specific; AI-specific authz exists on the MCP side (CEL/JWT/tool-level).
- **Compliance**: request/response **body redaction** (v0.6).
- **No virtual-key hierarchy, no per-key budgets/spend caps, no built-in guardrails/content-safety** (content-safety integration is roadmap).

### 3.5 MCP Gateway (first-class since v0.4/v0.5, expanded through v0.7)
- Aggregates **multiple MCP servers behind one endpoint**; tool names auto-prefixed per backend (`github__issue_read`); routes tool calls to the right upstream.
- `MCPRoute` CRD with **toolSelector** (exact + regex, per-backend) to control tool exposure.
- **OAuth** per the MCP Authorization spec (PKCE, configurable issuer/audience/protected-resource metadata); upstream API-key injection from Secrets; client header forwarding with renaming.
- **Fine-grained authorization**: JWT scopes/claims, per-backend/per-tool targeting, **CEL expressions** over headers/MCP method/params; v0.7 filters `tools/list` responses by authorization.
- Full **2025-06 MCP spec**: tool calls, notifications (multi-server SSE merge), prompts, resources, bi-directional requests, Streamable HTTP transport, session management with `Last-Event-ID` reconnection.
- Same OTEL tracing + Prometheus metrics as LLM traffic; Envoy-native rate limiting/circuit breaking applies.
- Published MCP perf: gateway adds ~0.2ms vs competing MCP proxy; absolute MCP-proxying overhead 160–390ms vs direct (dominated by session handling); session-encryption KDF tuning takes tens-of-ms → 1–2ms per new session.

### 3.6 Observability
- **Metrics**: Prometheus metrics following **OTEL GenAI semantic conventions** (token usage, time-to-first-token, inter-token latency, model/provider dimensions) from the extproc; Envoy/Envoy Gateway metrics underneath.
- **Tracing**: OpenTelemetry with **OpenInference semantic conventions** — full chat-completion request/response capture, compatible with eval systems like **Arize Phoenix**.
- **Access logs**: AI metadata (model, token usage) injectable into Envoy access logs.
- **No product UI/console/dashboard of its own** — bring-your-own Grafana (generic Envoy Gateway dashboards exist on grafana.com; nothing AI-gateway-specific shipped). No cost dashboards, no usage explorer, no admin UI.

## 4. Agent Experience (AX) Notes

- **Agent-facing posture is strong on the data plane, weak on the control plane.** MCP gateway is among the most spec-complete (June-2025 spec, OAuth, CEL authz, tool multiplexing) — designed for *agents as clients*.
- `aigw run` auto-configures from **OpenAI SDK env vars**, making it a drop-in local sidecar for agent dev loops (point any OpenAI/Anthropic SDK at `localhost:1975`).
- **Control plane is kubectl/YAML-only**: configuring the gateway means authoring CRDs — fine for GitOps, hostile to "agent edits config via API/MCP". No REST admin API, no MCP server *for managing the gateway itself*, no imperative CLI beyond `aigw run`/translate. CRDs being machine-readable/declarative is agent-friendly in k8s contexts but there is no story outside k8s.
- Docs are docs-site MDX; a DeepWiki mirror exists; no llms.txt observed.

## 5. Performance (published numbers)

- Control plane: **2,000 AIGatewayRoutes** with consistent route-readiness latency, linear resource growth, zero routing failures, no data-plane overhead from scale (official blog benchmark).
- MCP: ~**0.2ms** average difference vs competing MCP proxy implementation; both add 160–390ms vs direct MCP upstream; session encryption tunable to 1–2ms.
- v0.5 switched JSON handling to **sonic**, claims latency reduction across all requests; MCP HTTP connection reuse improved multi-backend throughput.
- No published end-to-end LLM-proxying overhead numbers (req/s, P99 added latency) — relies on Envoy's general reputation. Kong's adversarial benchmark targets LiteLLM/Portkey, not Envoy AI Gateway.

## 6. Adoption

- **Production adopters (per AAIF proposal)**: Bloomberg, Tencent Cloud, Nutanix, LY Corporation, National Research Platform, Alan by Comma Soft, Paper Compute Co., Simplifai — 9+ orgs.
- Bloomberg uses it as the central quota/access-control point for all GenAI usage. Google Cloud blog champions Envoy as the agentic-era network layer (relevant: GKE Inference Gateway shares the InferencePool machinery).
- Weekly community meetings, active Slack (#envoy-ai-gateway), 97 contributors across 5+ companies.

## 7. Weaknesses & Gaps (the attack surface)

1. **Kubernetes-or-bust**: real deployments require Envoy Gateway + two Helm charts + CRDs + add-on values files; Helm CRD management is a documented pain; standalone `aigw` is experimental and positioned as a dev/test tool, not production.
2. **No dashboard/UI whatsoever** — no usage explorer, no cost view, no key management screen; everything is YAML + Prometheus + BYO Grafana.
3. **No virtual keys / key hierarchy / budgets / spend tracking in dollars** — token-aware rate limiting exists, but cost-per-key budget management (a LiteLLM/Portkey staple) does not.
4. **No semantic caching** (open design issue #30 since early 2025) and no response caching.
5. **No built-in guardrails/content safety** (PII redaction limited to body-redaction; content-safety integration is roadmap).
6. **Rate limiting needs Redis** and piggybacks on Envoy Gateway's global rate limit — multi-CRD, multi-component setup for a feature competitors ship as one config line; usage extraction only understands OpenAI-schema responses.
7. **Young APIs**: CRDs reached v1beta1 only in v0.6 (May 2026); pre-1.0; breaking-change risk acknowledged; security maturity incomplete (no OpenSSF badge yet).
8. **Provider breadth** trails LiteLLM (100+) — 16–20 providers, several only via generic OpenAI-compat pass-through; no batch API.
9. **Config UX**: a simple "route model X to provider Y with a fallback and a rate limit" spans 4–5 CRDs (`AIGatewayRoute`, `AIServiceBackend`, `BackendSecurityPolicy`, `BackendTrafficPolicy`, `Backend`) — high concept count vs a single YAML/JSON file in LiteLLM-style gateways.
10. **No admin API / no agent control plane**: gateway can only be reconfigured through k8s API server; nothing for non-k8s operators or agent-driven config.
11. **Observability is emit-only**: no built-in log store, request explorer, or evaluation hooks beyond exporting OpenInference traces to third parties (Phoenix etc.).
12. Failover semantics under streaming and health-check behavior are under-documented.

## 8. What to Steal (best-in-class ideas)

- **MCP gateway design**: backend-prefixed tool multiplexing, `toolSelector` (exact+regex per backend), CEL/JWT/tool-level authorization, authz-filtered `tools/list`, full Streamable-HTTP/session/`Last-Event-ID` handling — the most complete OSS MCP-gateway spec story; replicate in the single binary.
- **CEL-based token cost expressions** for rate limiting (weighted cached/output tokens) — elegantly general.
- **Cross-schema endpoint translation**: serving Anthropic `/v1/messages` on top of OpenAI backends (and vice versa via Bedrock Converse) + one `reasoning_effort` knob across providers + unified `cache_control` prompt caching.
- **`aigw run` zero-config local mode** that bootstraps from OpenAI SDK env vars — great dev-loop on-ramp; make this the *primary* (not experimental) mode in a single-binary product.
- **OTEL GenAI metrics + OpenInference traces** with full request/response capture compatible with eval tooling — adopt the same semantic conventions for ecosystem interop.
- **Two-tier pattern + InferencePool** awareness for self-hosted model fleets (GPU-aware endpoint picking) — increasingly table stakes for k8s inference.
- **Hostname-scoped model catalogs** for multi-tenancy (v0.7).
- Vendor-neutral, multi-company governance as a trust signal (and its flip side: slower, consensus-bound roadmap a startup can outrun).

## 9. Table Stakes Checklist (what everyone in this category has)

- OpenAI-compatible unified API across providers, streaming, tool calling
- Multi-provider routing + model-name virtualization
- Provider failover with priority/retries
- Token-aware rate limiting per consumer
- Upstream credential management (API keys + cloud-IAM auth)
- Prometheus metrics + OTEL tracing with GenAI conventions
- Embeddings/images/audio endpoint coverage beyond chat
- Helm/k8s deployment, Apache-2.0 OSS
- MCP support (rapidly becoming table stakes through 2026)

## Sources

- https://aigateway.envoyproxy.io/docs/capabilities/ (and subpages: supported-providers, supported-endpoints, usage-based-ratelimiting, provider-fallback, mcp, observability, cli)
- https://github.com/envoyproxy/ai-gateway (+ /releases)
- https://aigateway.envoyproxy.io/release-notes/ (v0.5, v0.6, v0.7)
- https://github.com/aaif/project-proposals/issues/18 (AAIF proposal: adopters, roadmap, governance)
- https://aigateway.envoyproxy.io/blog/benchmarking-control-plane-scaling/, https://aigateway.envoyproxy.io/blog/mcp-in-envoy-ai-gateway/, https://tetrate.io/blog/envoy-ai-gateway-mcp-performance
- https://tetrate.io/blog/envoy-ai-gateway-concept-to-reality, Bloomberg press release
- https://dev.to/pranay_batta/best-open-source-ai-gateway-in-2026-2flb, https://jimmysong.io/blog/ai-gateway-in-depth/, https://www.getmaxim.ai/articles/top-open-source-ai-gateways-for-enterprises-in-2026/ (third-party gap analysis)
- https://github.com/envoyproxy/ai-gateway/issues/30 (semantic caching design issue)
