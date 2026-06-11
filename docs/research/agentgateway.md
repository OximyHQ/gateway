# Competitive Intelligence Report: agentgateway (agentgateway.dev)

**Date:** 2026-06-10
**Category:** MCP gateway / AI-native agentic proxy (closest existing thing to a unified LLM+MCP gateway)
**Steward:** Linux Foundation project, originated and primarily developed by Solo.io
**Site:** https://agentgateway.dev | **Repo:** https://github.com/agentgateway/agentgateway

---

## 1. Identity & Positioning

Agentgateway bills itself as the "first AI-native gateway" — an open-source **proxy data plane + gateway control plane** purpose-built for agentic traffic: **agent-to-LLM, agent-to-tool (MCP), and agent-to-agent (A2A)**, while also handling plain HTTP/gRPC so one gateway serves both microservice APIs and AI workloads ("unified data plane"). Donated to the Linux Foundation by Solo.io in August 2025; backed publicly by AWS, Cisco, Huawei, IBM, Microsoft, Red Hat, Shell, Zayo.

Core pitch: traditional API gateways (Envoy, NGINX) were built for short-lived stateless REST; agentic workloads need **session/message awareness** (stateful MCP sessions, SSE/streaming, long-lived bidirectional connections, fan-out) that existing proxies "cannot support without major re-architecture."

- **License:** Apache 2.0, fully open source.
- **Implementation:** Rust data plane (Tokio, Hyper, Tonic, cel-rust) — lessons from Istio ztunnel; Go control plane (~26% of repo); TypeScript UI (~9%).
- **Community:** ~3.2k GitHub stars, ~530 forks, ~1,962 commits, 187 open issues; latest release 1.3.0-alpha.1 (May 2026); public community meetings + Discord.
- **Commercial:** "Solo Enterprise for agentgateway" (Oct 2025) — enterprise distribution with MCP onboarding/registration, tool-server fingerprinting/versioning, runtime policy against tool poisoning/rug-pulls/tool shadowing/naming collisions, secure elicitation with enterprise IdP/IAM. Pricing is custom/sales-led (no public price list). Also "Solo Labs for MCP" (forward-deployed engineering engagement).

## 2. Deployment & Configuration Model

Two first-class modes with parallel doc trees:

1. **Standalone binary** — single binary, local YAML/JSON static config file; binary, Docker, or K8s deployments. Admin UI on port 15000 is "fully interactive" in this mode (config changes without restart). JSON-schema validation + a published configuration schema explorer.
2. **Kubernetes mode** — full **Gateway API implementation** (Gateways, HTTPRoutes) plus its own CRDs: `AgentgatewayBackend`, `AgentgatewayPolicy` (TrafficPolicies), `AgentgatewayParameters`. Go control plane watches K8s resources + secret stores, translates to **xDS snapshots**, distributes to Rust proxies over gRPC — dynamic config updates with no proxy restarts. Install via Helm, ArgoCD, FluxCD. Auto xDS TLS with controller-generated certs + rotation.

Policy model: policies attach at gateway/listener/route/backend levels, with **targeting + merging rules**, documented **policy processing order**, **route delegation** (parent routes delegate path segments to child routes across namespaces, label-based delegation, multi-level, policy inheritance/overrides), and **conditional policies gated by CEL expressions**.

## 3. LLM Gateway Feature Surface

- **Providers (native):** OpenAI, Anthropic, Amazon Bedrock, Azure (OpenAI + AI Foundry), Google Gemini, Vertex AI, GitHub Copilot. **OpenAI-compatible:** xAI/Grok, Cohere, Together, Groq, DeepSeek, Mistral, Perplexity, Fireworks. **Self-hosted:** Ollama, vLLM, LM Studio.
- **API types served:** `/v1/chat/completions`, `/v1/responses` (OpenAI Responses API), `/v1/messages` (Anthropic), `/v1/embeddings`, `/v1/realtime` (OpenAI Realtime / WebSocket), `/v1/messages/count_tokens`, plus raw **passthrough** mode. Streaming + non-streaming across completions/responses/messages.
- **Routing:** model-field matching with wildcards, **model aliasing** (decouple client-facing names from provider models), priority by specificity then config order, header-based matching, **content-based routing**, multi-endpoint/multi-provider per route.
- **Resiliency:** load balancing, **model failover**, retries (+ per-try timeouts), timeouts, traffic splitting, mirroring, locality-aware routing.
- **Cost/spend:** token-based rate limiting (`localRateLimit` with maxTokens/tokensPerFill/fillInterval), request-time estimation (`tokenize: true`) + response-time true-count reconciliation, budget/spend limits, **LLM cost tracking** via Prometheus token-usage metrics; **virtual keys** (gateway-issued API keys with per-key metadata, tracking, and rate limits — though true per-key budgets require routing-based config workarounds).
- **Prompt features:** prompt enrichment (system/append messages), prompt templates, request/response transformations, "inject LLM model headers."
- **Guardrails (multi-layer pipeline):** regex filters (block/mask PII etc.), OpenAI Moderation API, **AWS Bedrock Guardrails** (incl. masking), **Google Model Armor**, Azure Content Safety, and **custom webhook guardrails with a published OpenAPI spec** — composable in layers on both prompt and response.
- **Inference routing:** implements Kubernetes **Gateway API Inference Extension** — endpoint-picking for self-hosted models based on GPU utilization, KV-cache state, LoRA adapters, queue depth; integrations with vLLM, **vLLM Semantic Router**, KServe, Argo Rollouts (canary for models).
- **CEL-based RBAC for LLM routes** and rate limiting per consumer.

## 4. MCP Gateway Feature Surface

- **Transports:** stdio, SSE, Streamable HTTP — gateway bridges between them (e.g., expose a stdio server over Streamable HTTP); stateful session handling with idle TTLs.
- **Multiplexing / Virtual MCP:** federate multiple MCP servers (targets) into one unified server; tools auto-prefixed `targetname_toolname`; single connection for clients; "Virtual MCP" backend abstraction in both standalone and K8s.
- **Tool governance:** per-tool/prompt/resource **CEL authorization** (`mcp.tool.name`, `mcp.prompt.name`, `mcp.resource.name`, `mcp.tool.target` against JWT claims like `jwt.sub`, `"admin" in jwt.roles`); unauthorized tools are **automatically filtered out of tools/list responses**; backend-level vs per-target policy override; tool access lists.
- **MCP authentication:** OAuth 2.x protected-resource flow at the gateway — serves/proxies `.well-known` protected-resource + authorization-server metadata and client registration; JWT/JWKS validation with required-claims config; **provider adapters for Keycloak and Auth0** (Okta documented too); strict/optional/permissive modes; token passthrough for already-protected servers.
- **OpenAPI → MCP:** turn existing REST APIs into MCP tools declaratively (a key "integrate existing APIs as agent-native tools" play).
- **Dynamic MCP (K8s):** auto-discover MCP servers via label selectors (`appProtocol: agentgateway.dev/mcp`) — add/remove servers with no config changes (Streamable HTTP only).
- **MCP rate limiting, target policies, stateful session support, MCP observability** (metrics/traces of client↔tool interactions).

## 5. A2A Gateway

Proxies Google's Agent2Agent protocol: capability discovery, modality negotiation (text/forms/audio-video), secure long-running task collaboration without exposing agent internals. Applies the same security/observability/policy stack to agent-to-agent calls. (Docs here are notably thinner than MCP/LLM areas.)

## 6. Full API-Gateway Feature Floor (non-AI)

HTTP + gRPC routing; TCP listeners; HTTPS/mTLS (FrontendTLS/BackendTLS, post-quantum X25519_MLKEM768, Istio workload certs); matching on header/host/method/path/query; redirects/rewrites; rich body+header **transformations with a templating language** (incl. CEL); direct responses; buffering; **Dynamic Forward Proxy**; **ExtProc (Envoy-style external processing)**; external auth (BYO ext-auth service, basic auth); JWT auth; API-key auth; OIDC browser authentication; CORS/CSRF; local + global rate limiting; access logging (CEL-enrichable); cert-manager/external-dns integrations; **ingress-nginx migration tooling** with worked examples (canary, SSL redirect, ext auth...).

## 7. Observability

- **OpenTelemetry-native:** metrics, traces, logs; documented metric/trace references; OTel stack guide; Prometheus `/metrics`; Grafana + Jaeger guides; control-plane metrics; NACK monitoring.
- **GenAI semantics:** token usage metrics (`agentgateway_gen_ai_client_token_usage_sum`), TTFT for SSE streams, dedicated stream parsers for non-SSE formats (e.g., Bedrock event stream).
- **LLM-observability integrations:** Langfuse, LangSmith, Arize Phoenix.
- **Custom Prometheus labels via CEL** on policies; `/debug/pprof/heap` profiling.

## 8. Dashboard / UI

Built-in **Admin UI** (port 15000): shows listeners/binds, routes, backends; in standalone mode it is fully interactive — config can be managed through it without restarts; includes an MCP tool listing view (federated "Available Tools") and playground-style exploration of agent-tool connections. K8s mode UI is more read-only/inspection oriented. A separate `solo-io/agentgateway-new-ui` repo suggests a UI revamp in flight. The UI is a convenience layer — config files/CRDs remain the source of truth.

## 9. CLI & Agent Experience (AX)

- **`agctl` CLI:** `agctl config` (dump runtime binds/listeners/routes/backends/workloads/services), `agctl trace` (tap-style live request tracing with interactive TUI **or JSON output "for tool integration"**), shell completions. Notably weak install story: build-from-source via `go install` (needs Go 1.22+, Git, Graphviz).
- **Docs are aggressively machine-readable:** full `/docs/llms.txt` index, every doc page served as raw markdown by appending `.md` — explicitly designed for LLM consumption.
- **Schema surfaces:** published JSON config schema + interactive schema explorer; **CEL playground** and interactive CEL context explorer; guardrail webhook OpenAPI spec.
- **Declarative-first:** everything is YAML/CRDs — trivially generatable by coding agents; xDS hot-reload means an agent can mutate config without restarts (K8s mode).
- **Gaps for agents:** no MCP-based control plane (you cannot manage the gateway itself via MCP tools), no REST admin/config API documented for standalone (config file + UI only), no CLI-first scaffolding (`init`-style) workflow, agctl requires compiling from source.
- Client config guides for Claude Code, Claude Desktop, Cursor, Windsurf, Continue, GitHub Copilot, OpenAI SDK, OpenCode, VS Code, Antigravity IDE — including a "Claude Code CLI proxy" tutorial (route Claude Code through the gateway).

## 10. Performance (published)

- **~500k QPS** with 512 connections in Solo's benchmark, "outperforming peer proxies under similar conditions."
- **<0.2 ms P99 latency at 30k QPS** (512 concurrent connections).
- Third-party reference: John Howard's Gateway API Bench v2 used as methodology.
- Marketing claim: "the most performant, reliable, and mature LLM/MCP gateway on the market."
- Note: these are HTTP-proxy benchmarks; no published numbers for MCP-session or LLM-streaming overhead specifically.

## 11. Weaknesses & Criticisms (third-party review + observed gaps)

From the detailed dev.to review (spacewander, an APISIX maintainer) and doc analysis:

1. **Stateful-by-default MCP with in-process SessionManager** — no distributed session consistency; multi-replica routing of MCP sessions is fragile (reviewer: "a mistake"; suggests consistent hashing on MCP-Session-ID instead).
2. **No generic provider-to-provider translation** — only OpenAI `/v1/chat/completions` and Anthropic `/v1/messages` entry formats convert; structured output unsupported in conversion.
3. **Thin MCP metrics** — "basically just an mcp_requests counter"; no per-tool latency/error visibility despite the governance pitch.
4. **JWKS fetched only at config parse** — no periodic runtime refresh (key rotation hazard).
5. **Doc drift** — features without docs (Anthropic, at time of review) and docs referencing nonexistent metrics (`list_calls_total`); fast-moving project, docs lag code.
6. **OpenAPI→MCP rough edges** — JSON-only request bodies, no HTTPS upstreams, no structured output, additionalProperties edge cases.
7. **Per-key budgets are awkward** — `localRateLimit` is gateway-wide; true per-virtual-key budgets need routing-based config contortions; over-budget streaming responses still complete (post-hoc accounting).
8. **No persistent storage** — cost tracking is Prometheus-metrics-only; no built-in spend ledger, log store, or historical analytics UI (you must bring Langfuse/Grafana).
9. **OAuth well-known routing requires explicit route-match config** when multiple protected resources share a host.
10. **IdP integration requires code changes** (McpIDP enum) beyond Keycloak/Auth0.
11. **agctl is build-from-source**; no packaged install.
12. **Two divergent product shapes** (standalone vs K8s) with different feature sets/doc trees creates cognitive overhead; the K8s mode pulls in heavy Gateway-API/CRD machinery for what small teams may want as a simple binary.
13. Enterprise-grade MCP supply-chain protections (tool poisoning, rug-pull, fingerprinting, registry) are **held back for the paid Solo Enterprise tier**.

## 12. Differentiators Worth Stealing

- One data plane for HTTP + gRPC + MCP + A2A + LLM — collapses API gateway and AI gateway into one operational surface.
- CEL everywhere as the single policy language: tool-level MCP RBAC, conditional policies, rate-limit keys, custom metric labels, log enrichment, transformations — with a CEL playground in docs.
- Auto-filtering unauthorized tools from MCP `tools/list` (agents never see what they can't call).
- Virtual MCP federation with target-name prefixing; OpenAPI→MCP conversion; dynamic K8s label-selector discovery of MCP servers.
- Gateway-terminated MCP OAuth (.well-known protected-resource metadata served/proxied at the gateway, IdP adapters).
- Gateway API Inference Extension (GPU/KV-cache/LoRA/queue-depth aware routing to self-hosted models).
- Multi-layer guardrails pipeline mixing regex + 3 cloud moderation services + custom webhooks (OpenAPI-specced).
- llms.txt + .md-suffix docs, JSON-schema explorer, agctl JSON trace output — strong machine-readable surface area.
- Rust single binary with real published numbers (500k QPS, <0.2ms P99) and xDS hot reload.

## 13. Implications for a New OSS LLM+MCP Gateway

Agentgateway is the closest incumbent to "unified LLM+MCP gateway, single binary." Its soft spots map directly to opportunity: (a) no MCP/REST control-plane API for agents to manage the gateway itself; (b) no built-in persistence — no spend ledger, request log explorer, or historical dashboard; (c) per-key budgets/quotas are clumsy; (d) MCP observability is thin; (e) distributed MCP session handling unsolved in OSS; (f) MCP supply-chain security paywalled in Solo Enterprise; (g) heavyweight K8s-centric posture vs a dev-friendly single-binary-with-great-dashboard experience; (h) agctl install friction. A challenger that ships a batteries-included dashboard + durable analytics + agent-first (MCP) control plane + simple per-key budgets in one binary would attack exactly where agentgateway is weakest while needing to match its table stakes (provider breadth, transports, federation, CEL-grade policy, OTel).

---

### Sources

- https://github.com/agentgateway/agentgateway
- https://agentgateway.dev/ and https://agentgateway.dev/docs/llms.txt (full doc index)
- https://agentgateway.dev/docs/standalone/latest/... (mcp/about, mcp/connect/virtual, mcp/mcp-authn, mcp/mcp-authz, llm/about, llm/virtual-keys, agent/about, operations/ui, faqs)
- https://agentgateway.dev/docs/kubernetes/latest/... (about/architecture, mcp/dynamic-mcp, operations/agctl, reference/release-notes)
- https://www.solo.io/blog/designing-agentgateway-a-unified-high-performance-gateway-for-ai-and-api-traffic (benchmarks)
- https://www.solo.io/blog/solo-contributes-agentgateway-linux-foundation
- https://www.solo.io/press-releases/enterprise-agentgateway-mcp-labs (Solo Enterprise for agentgateway)
- https://dev.to/spacewander/agentgateway-review-a-feature-rich-new-ai-gateway-53lm (third-party technical review/criticisms)
- https://thenewstack.io/why-tech-giants-are-backing-the-new-agentgateway-project/
