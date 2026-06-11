# Competitive Intelligence Report: Apache APISIX AI Gateway

**Category:** LLM gateway (AI capabilities layered onto a general-purpose cloud-native API gateway)
**Researched:** 2026-06-10
**Latest release at time of research:** APISIX 3.16.0 (2026-04-07); AI Gateway formally announced with 3.12.0 (April 2025)

---

## 1. Positioning & Identity

Apache APISIX is a top-level Apache Software Foundation project: a dynamic, real-time, cloud-native API gateway built on NGINX/OpenResty + LuaJIT with etcd as its configuration store. Since early 2025 it markets itself as "The Cloud-Native API Gateway and AI Gateway." The AI Gateway is **not a separate product** — it is a set of `ai-*` plugins running inside the same gateway binary/deployment. The pitch: if you already run APISIX (or want one gateway for both API and AI traffic), you get LLM routing, token rate limiting, and prompt security without deploying a second proxy layer.

Commercial backing comes from **API7.ai**, which sells API7 Enterprise (a hardened distribution with full-lifecycle API management, AI gateway features, portal, multi-cluster control plane) priced per gateway CPU core with Standard/Premium tiers and 99.95% SLA. The OSS project itself is free, Apache-2.0.

---

## 2. Architecture & Deployment Model

- **Data plane:** NGINX + OpenResty, plugins written in Lua (repo is ~81% Lua). Single deployable; radixtree-based route matching; fully dynamic — routes/plugins/upstreams/certs hot-reload without restarts.
- **Config store:** etcd by default (watch-based, ms-level config propagation across a fleet). This is the most-complained-about operational dependency.
- **Standalone mode:** YAML-file-driven, etcd-free mode. 3.13.0 added a *standalone Admin API* — HTTP PUT/GET of the entire in-memory config (JSON or YAML), fully stateless with worker broadcast, plus a readiness endpoint. 3.16.0 added rejection of configs referencing unknown plugins in standalone mode.
- **Control plane:** Admin API (REST, default :9180, protected by admin key); decoupled CP/DP deployment supported.
- **Kubernetes:** Helm chart + APISIX Ingress Controller; Gateway API support.
- **Dashboard:** historically a separate `apisix-dashboard` repo (now deprecated/unmaintained); since 3.13.0 a **new lightweight dashboard is embedded into APISIX itself**, built from a pinned git hash at release time. Users report the new dashboard has far fewer features than the legacy one.
- **Multi-platform:** runs on bare metal, VM, Docker, K8s; ARM64 supported.

## 3. AI Gateway Feature Surface (the `ai-*` plugin family)

### Proxy & request management
- **`ai-proxy`** — converts a plugin config into the provider's request format; one route = one LLM service. Supported providers (3.15+): `openai, deepseek, azure-openai, aimlapi, anthropic, openrouter, gemini, vertex-ai, openai-compatible`. Handles chat completions by default; embeddings via `override.endpoint`; streaming passthrough via `options.stream`. Auth via header or query param; GCP service-account auth (with token TTL/early-expiry controls) for Vertex AI. Connection controls: timeout (default 30s), keepalive pool, ssl_verify.
- **`ai-proxy-multi`** — multi-instance version adding **load balancing, priorities/weights, retries, fallbacks, and health checks**:
  - Balancer algorithms: weighted round-robin or consistent hash (`hash_on` vars/headers/cookie/consumer).
  - `fallback_strategy`: `instance_health_and_rate_limiting`, `http_429`, `http_5xx`, `rate_limiting` — i.e., traffic spills to other instances when one is unhealthy or its token quota is exhausted.
  - Active health checks per LLM endpoint (http/https/tcp probe, concurrency, path, cert verification) — added in 3.14.0.
  - Supports mixing providers and multiple deployments of the same model (e.g., private DeepSeek vs official API) with priority failover.
- **`ai-request-rewrite`** — sends the incoming request to an LLM with a prompt to redact/enrich/reformat it before forwarding to the real upstream (AI-assisted request transformation, e.g. PII scrubbing).

### Traffic & cost control
- **`ai-rate-limiting`** — **token-based** (not request-based) rate limiting using actual token usage reported by the LLM:
  - `limit_strategy`: `total_tokens` (default) / `prompt_tokens` / `completion_tokens`.
  - Global limit+time_window, plus **per-instance quotas** (different ceilings per LLM backend).
  - 3.16.0 added nginx-variable-based keys and multiple `rules` (dynamic, key-scoped limits).
  - Emits `X-AI-RateLimit-Limit/Remaining/Reset-{instance}` headers; configurable `rejected_code` (default 503) and message.
  - Combined with ai-proxy-multi fallback: when one instance's quota is consumed, traffic shifts to others — quota-aware routing.
- Classic plugins (limit-count/limit-conn/limit-req) still apply for request-level limiting per route/service/consumer/consumer-group, fixed or sliding windows, node-local or cluster (Redis) state.
- **`lago` plugin (3.13.0)** — integration with Lago open-source billing for **API monetization including token-based and per-call billing**.

### Prompt processing & safety
- **`ai-prompt-guard`** — regex allow/deny pattern matching on prompt content; can inspect only the latest message or full conversation history; scope to all roles or end-user role only. (Pattern-based, not ML-based.)
- **`ai-prompt-decorator`** — prepends/appends fixed messages around the user's prompt (system-prompt injection / context framing).
- **`ai-prompt-template`** — fill-in-the-blank prompt templates with variable substitution; clients send variables, gateway renders the prompt.
- **`ai-aws-content-moderation`** — AWS Comprehend toxicity detection on request content; blocks above-threshold content. (An Aliyun content-moderation equivalent exists in the ecosystem/enterprise track.)
- Marketing also claims PII redaction and response filtering/moderation of outputs — in OSS practice these are assembled from ai-request-rewrite + moderation + prompt plugins rather than dedicated turnkey guardrail plugins.

### Data enrichment
- **`ai-rag`** — retrieval-augmented generation at the gateway: currently wired to **Azure OpenAI embeddings + Azure AI Search**; gateway fetches relevant docs and stuffs them into the request before the LLM call. (Provider-narrow.)

### MCP support
- **`mcp-bridge` plugin (3.13.0, prototype)** — converts **stdio-based MCP servers into HTTP SSE** endpoints: APISIX launches the MCP server as a subprocess, owns its stdio, translates HTTP SSE ⇄ MCP. Existing auth (OAuth2/JWT/OIDC/key-auth) and rate-limiting plugins then govern MCP traffic.
  - Documented limitations: sessions **not shared across APISIX instances** (needs sticky LB), loop-driven SSE (inefficient; message-queue/event-driven rework planned), session management "just a prototype."
- **`apisix-mcp` (separate repo, api7/apisix-mcp, npm)** — an MCP **server for the Admin API**: lets Claude/Cursor/Copilot agents CRUD routes, services, upstreams, SSL, plugins (discovery + schema retrieval), consumers/credentials/secrets, global rules, stream routes via natural language. This is their "manage the gateway with AI" story.
- API7 (commercial) adds an **OpenAPI-to-MCP** converter in its hub.

### AI observability
- Access-log / logger variables: `llm_model`, `request_llm_model`, `llm_prompt_tokens`, `llm_completion_tokens`, `llm_time_to_first_token`, `request_type` (`ai_chat` / `ai_stream` / `traditional_http`), `apisix_upstream_response_time` (added across 3.14/3.15).
- Per-plugin `logging.summaries` (model, duration, tokens) and `logging.payloads` (full request/response bodies).
- These flow through APISIX's 20+ existing logger plugins (Kafka, Loki, ClickHouse, Splunk, Datadog, file, http, etc.) and Prometheus metrics; OpenTelemetry tracing plugin (more spans added in 3.16.0).

## 4. Inherited general-gateway surface (relevant table stakes)

- Protocols: HTTP/1.1/2/3(QUIC), TCP/UDP (stream proxy with L4 health checks since 3.13), WebSocket, gRPC + gRPC-Web, MQTT, Dubbo.
- Auth: key-auth, basic, JWT (more algorithms in 3.16), HMAC, OAuth2, OIDC (Redis session storage in 3.16), LDAP, forward-auth, Casbin authz, CSRF, IP allow/deny, UA blocking.
- Traffic: canary/blue-green (traffic-split), proxy-cache (disk/memory; exact-match only — **no semantic cache**), request validation, circuit breaker (api-breaker), retries, mirroring, fault injection.
- Service discovery: Consul, Nacos, Eureka, DNS, Kubernetes (EndpointSlices + readiness in 3.15/3.16).
- Extensibility: hot-loaded Lua plugins; **external plugin runners** in Java/Go/Python/Node over RPC; **WebAssembly** plugins; serverless hooks (inline Lua, AWS Lambda, Azure Functions); `ext-plugin` pre/post phases.
- Secrets: Vault/AWS/GCP secret manager integration; encrypted plugin-config storage in etcd (off by default).
- Multi-tenancy primitives: Consumers, Consumer Groups, credentials, global rules, plugin config objects.

## 5. Performance

- Published claims (README): **~18k QPS per core with avg latency < 0.2 ms**; AWS 8-core test ~**140k QPS**. These are classic API-proxy numbers, not LLM-path numbers (LLM routes add body parsing/transform per request).
- Third-party comparisons consistently rank APISIX fastest among OSS gateways (vs Kong's +2–5 ms overhead) thanks to LuaJIT + radixtree routing + etcd watch.
- No published benchmarks specifically for the ai-proxy path or token-counting overhead.

## 6. Pricing / Licensing

- OSS: Apache-2.0, no paid tiers, all `ai-*` plugins free. ~16.7k GitHub stars.
- API7 Enterprise (commercial distro): per-CPU-core annual licensing, Standard/Premium tiers, custom quotes; adds full API lifecycle management, developer portal, multi-gateway management, richer AI gateway UX/guardrail use-cases (e.g., documented PII/guardrails recipes), 24/7 support.

## 7. Agent Experience (AX) Notes

- **Admin-API-first**: everything is a REST resource (routes/upstreams/plugins/consumers); no restart needed for any change — very automatable. Standalone Admin API (3.13) makes a fully declarative "PUT the whole config" workflow possible, good for GitOps and agents.
- **First-party MCP control plane**: `apisix-mcp` exposes full Admin API CRUD + plugin schema discovery to MCP clients — agents can introspect plugin schemas before configuring them. This is genuinely agent-forward and worth noting.
- **MCP hosting**: mcp-bridge makes the gateway a host/converter for MCP servers (stdio→SSE) so agent tool traffic gets gateway auth/rate-limits — but it's a prototype with single-instance session affinity.
- **No agent-native CLI**: no first-party CLI of consequence (ADC — API Declarative CLI — exists in the API7 ecosystem); OSS interaction is curl-against-Admin-API or dashboard.
- Plugin schemas are JSON-schema-validated server-side — machine-checkable configs.

## 8. Weaknesses & Complaints

1. **Dashboard saga**: legacy apisix-dashboard officially unmaintained/deprecated; the new embedded dashboard (3.13+) is intentionally minimal and users complain it "has far fewer features… and development has stalled." No real AI-specific dashboard (no spend/cost views, no per-key budgets UI).
2. **etcd operational burden**: heavyweight dependency for small deployments; reported latency issues over HTTPS-to-etcd, "failed to fetch data from etcd" errors; plaintext secret storage in etcd unless encryption explicitly enabled.
3. **Security history**: default admin key/dashboard-credentials misconfigurations were exploited in the wild (Trend Micro study); dashboard has direct config access if exposed.
4. **AI features are bolt-on, not AI-first**: no semantic caching (proxy-cache is exact-match), no virtual keys with per-key budget/spend enforcement, no built-in cost tracking in dollars (tokens only), no unified OpenAI-compatible client-facing API across providers (provider is fixed per route/instance config rather than model-name-based routing), guardrails are regex or AWS-Comprehend, RAG plugin is Azure-only.
5. **Provider coverage ~9 named providers** (plus openai-compatible escape hatch and AIMLAPI's 300+-model aggregator) vs LiteLLM's 100+ native integrations; no native Bedrock provider in the OSS list.
6. **mcp-bridge is a prototype**: no cross-instance session sharing, inefficient loop-driven SSE, subprocess model only (stdio servers running on the gateway box).
7. **Lua/NGINX expertise required** for anything custom; multi-language plugin runners add RPC hops; learning curve and config verbosity are recurring complaints.
8. **Docs and feature discovery are fragmented** between apisix.apache.org, api7.ai docs, and blog posts; AI-gateway capabilities (e.g., guardrails/PII recipes) often documented only on the commercial API7 side.
9. Token-based rate limiting is **post-hoc** (counts tokens from responses after the fact) — first over-quota request still reaches the provider; no pre-call token estimation.

## 9. What's worth stealing / learning from

- **Quota-aware fallback**: ai-rate-limiting + ai-proxy-multi `fallback_strategy: rate_limiting` — when one provider's token budget is exhausted, the balancer automatically spills to the next instance. Clean composition of rate limiting with LB.
- **Token-denominated rate limiting with per-instance quotas** and standard X-AI-RateLimit-* response headers.
- **First-class LLM log variables** (`llm_time_to_first_token`, token counts, `request_type=ai_chat|ai_stream`) injected into the generic logging pipeline so every existing logger/exporter gets LLM telemetry for free.
- **stdio→SSE MCP bridging at the gateway** so MCP servers inherit gateway auth/rate-limit/observability — right idea even if their implementation is a prototype.
- **MCP server over the Admin API with plugin-schema discovery** — agents can fetch the JSON schema of any plugin before writing config.
- **Hot-reload everything** (routes/plugins/certs) with ms-propagation; standalone "PUT whole config" API for GitOps.
- **Health-checked LLM instances** with priority+weight and consistent-hash session affinity options.
- **Monetization hook** (Lago plugin) for token-based billing — gateway as a revenue meter, not just a cost meter.

## 10. Sources

- https://apisix.apache.org/ai-gateway/
- https://apisix.apache.org/blog/2025/04/08/introducing-apisix-ai-gateway/
- https://apisix.apache.org/blog/2025/02/24/apisix-ai-gateway-features/
- https://apisix.apache.org/docs/apisix/plugins/ai-proxy/ and /ai-proxy-multi/ and /ai-rate-limiting/
- https://github.com/apache/apisix (README, CHANGELOG)
- https://apisix.apache.org/blog/2025/06/27/release-apache-apisix-3.13.0/
- https://apisix.apache.org/blog/2025/10/10/release-apache-apisix-3.14.0/
- https://apisix.apache.org/blog/2026/02/05/release-apache-apisix-3.15.0/
- https://github.com/api7/apisix-mcp
- https://apisix.apache.org/blog/2025/04/21/host-mcp-server-with-api-gateway/
- https://api7.ai/pricing , https://api7.ai/apisix-vs-enterprise
- https://github.com/apache/apisix-dashboard/issues/3297 (dashboard maintenance complaints)
- https://www.trendmicro.com/vinfo/us/security/news/cybercrime-and-digital-threats/apache-apisix-in-the-wild-exploitations-an-api-gateway-security-study
- Comparison roundups: dev.to "Best Open Source AI Gateway in 2026", getmaxim.ai gateway comparisons, nsddd.top AI gateway market analysis
