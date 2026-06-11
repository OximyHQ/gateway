# Higress (Alibaba) — Competitive Intelligence Report

Researched: 2026-06-10. Subject category: LLM gateway / AI-native API gateway (also functions as MCP gateway, Kubernetes ingress, microservice gateway).

## 1. Identity & Positioning

- **What it is:** Open-source "AI Native API Gateway" built on **Envoy + Istio**, created inside Alibaba (originally to replace Tengine/Nginx for ingress; reload issues, gRPC/Dubbo LB gaps). Now powers Alibaba Cloud's AI services (Tongyi Bailian model studio, PAI platform, Qwen-related serving).
- **Positioning:** "Three gateways in one" — traffic gateway + microservices gateway + AI gateway in a single control plane. Markets itself explicitly as an *API gateway with AI capabilities* rather than a thin LLM proxy, and contrasts itself against OneAPI and LiteLLM on its comparison page.
- **CNCF Sandbox project** as of March 25, 2026 (TOC approved). Repos: `alibaba/higress` (mirrored at `higress-group/higress`), plus `higress-console`, `higress-standalone`, `openapi-to-mcpserver`, `higress-ops-mcp-server`.
- **License:** Apache 2.0. **Languages:** Go ~80%, C++ ~13% (Envoy data plane), Rust ~2%. Wasm plugins in Go/Rust/JS.
- **Named adopters:** Alibaba Group, Ant Group, BOSS Zhipin, Cathay Insurance, Ctrip, DJI, Kuaishou, Sealos, Vipshop. Strong China-market gravity.
- **Commercial model:** OSS core; Alibaba Cloud enterprise edition (multi-AZ deployment, auto-failover, container security scanning, content-security integration). No SaaS pricing for OSS — monetization is via Alibaba Cloud.

## 2. Architecture & Deployment Model

- Data plane: Envoy. Control plane: Istio-derived, control/data plane separation, dynamic config via xDS — **millisecond config propagation, no reload** (key pitch vs Nginx for AI: long-lived SSE/gRPC streaming connections survive config changes; "traffic-lossless hot updates").
- Incremental config loading: initial Ingress config time cut from >2min to ~3s at large scale (Sealos case: thousands of tenants).
- **Deployment options:**
  - Docker one-liner: `curl -sS https://higress.cn/ai-gateway/install.sh | bash` (console :8001, HTTP :8080, HTTPS :8443). NOT a single binary — it's a container stack (gateway + console; Redis required for some MCP/caching features).
  - Kubernetes via Helm chart (artifacthub `higress/higress`), full Ingress controller + Gateway API v1.4 (HttpRoute, GrpcRoute, TcpRoute, UdpRoute, BackendTLSPolicy GA) + **Gateway API Inference Extension (GIE)** for model-aware routing/priority scheduling on K8s.
  - Standalone (non-K8s) via separate `higress-standalone` repo.
  - Alibaba Cloud managed enterprise edition.
- Service discovery integrations: Nacos, ZooKeeper, Consul, Eureka; Dubbo protocol support.
- Nginx Ingress migration story: compatible with mainstream Ingress annotations (rewrite, rate limit, auth, TLS), gray traffic switching, mirroring, one-click rollback; CNCF blog showcases AI-assisted migration of 60+ resources in 30 minutes.

## 3. LLM Gateway Feature Surface

### Providers / protocol (ai-proxy plugin)
- **40+ providers:** OpenAI, Azure OpenAI, Anthropic Claude, Google Gemini, Vertex AI, AWS Bedrock, Cloudflare Workers AI, GitHub Models, Groq, Grok (xAI), OpenRouter, Fireworks, Together, Mistral, Cohere, DeepSeek, Ollama, NVIDIA Triton, DeepL, Dify, plus deep Chinese coverage (Qwen, Moonshot, Baidu Ernie, Hunyuan, Spark, Baichuan, Yi, Zhipu, 360, Doubao, Coze, MiniMax, Stepfun…). Claims "100+ LLM models."
- **Multi-protocol frontend:** OpenAI protocol (`/v1/chat/completions`) AND native **Claude protocol (`/v1/messages`)** with **automatic protocol detection** (no config); "original protocol" passthrough mode; capability passthrough mapping for native vendor endpoints (embeddings, rerank, image, files).
- Provider-specific depth: Azure URL modes, Vertex 3 auth modes, Bedrock SigV4 or bearer + prompt-caching retention config, Qwen search/file upload, Gemini safety settings + thinking budget, **"Claude Code mode" OAuth token auth** (explicitly supports proxying Claude Code traffic), `mergeConsecutiveMessages` for strict-alternation Chinese models, KlingAI video generation.

### Reliability & routing
- Multi-model load balancing + fallback models; **API-key pools with per-token failover** (auto-remove unhealthy tokens, health-check or cooldown recovery); retry mechanism for failed non-streaming requests; canary release of models; `modelToHeader` for model-based routing/rate-limiting/metering; model-mapper & model-router plugins (route by model name in body); "intent-based load balancing" (ai-intent plugin); ai-load-balancer plugin (incl. inference-aware strategies).
- True full streaming body processing in Wasm plugins (SSE-aware plugin SDK) — a genuine architectural differentiator vs request-buffer gateways.

### Cost & quota governance
- **ai-token-ratelimit:** token-level (not QPS) rate limiting per consumer/route/model.
- **ai-quota:** token-consumption quota management.
- Consumer management with multi-dimensional authentication (key-auth, JWT, HMAC, OIDC, ext-auth, OPA).

### Caching & augmentation plugins
- ai-cache (semantic caching with vector-similarity, pluggable vector DB), ai-rag (gateway-side RAG), ai-search (LLM web-search augmentation), ai-history, ai-prompt-template, ai-prompt-decorator, ai-transformer, ai-json-resp (structured JSON enforcement), ai-image-reader, ai-agent (gateway-hosted agent plugin), ai-intent.

### Security
- ai-security-guard: prompt-injection detection, sensitive-content recognition, data masking, real-time streaming filtering (integrates Alibaba content-safety service — strongest in commercial edition); WAF, bot-detect, replay-protection, request-block, ip-restriction, geo-ip plugins.

### Observability
- ai-statistics plugin: token usage per provider/model/consumer, input/output rates, latency tracking; AI Dashboard in console; Prometheus/Grafana metrics, access logs, tracing; **Agent Session Monitor** (v2.2.0) — real-time parsing of gateway access logs to track multi-turn agent conversations.

## 4. MCP Gateway Feature Surface

- **MCP server hosting via Wasm plugin mechanism:** the gateway itself hosts/serves MCP servers (tools execute at the gateway), not just proxies them. Public marketplace at `mcp.higress.ai` (Product Hunt launch; free hosted tools like Wolfram Alpha, Code Interpreter w/ daily quotas).
- **openapi-to-mcpserver tool:** declarative YAML conversion of any OpenAPI spec → hosted remote MCP server, no code, no redeploy on API change; bulk conversion supported. This is their flagship "API → agent tool" story.
- **Database-to-MCP:** direct MCP exposure of PostgreSQL, MySQL, ClickHouse, SQLite (configure DSN; gateway generates tools).
- **Transports:** SSE (path suffix `/sse`, session persistence via Redis + match_list routing rules) and Streamable HTTP (2025-03-26 spec, no ConfigMap needed).
- **MCP governance:** unified auth/authz at gateway, per-tool/per-API-key rate limiting, audit logs, observability, dynamic updates without dropping live MCP connections; mcp-router plugin for intelligent tool routing across servers; "private MCP marketplace" pattern for enterprises.
- **Nacos 3.0 registry integration:** MCP servers discovered dynamically from service registry (`/mcp/{service-name}/sse`).
- Unified management of "LLM API + MCP API + Agent API" is the stated product frame.

## 5. Agent-Experience (AX) Surface — how agents control/are served by it

- **hgctl CLI** with `hgctl mcp` (register tool servers) and **`hgctl agent`** — an interactive natural-language agent for managing Higress itself; integrates with agent runtimes (Claude Code, Qoder) via "AgenticCore"; syncs config between CLI and console.
- **Higress API MCP Server (built-in, v2.1.10+):** manage the gateway over MCP — tools: list/add/update/delete-route, list/add-ai-route, list/get/add/update/delete-ai-provider, list-plugin-instances, get-plugin, delete-plugin. Also separate `higress-ops-mcp-server` repo. → The control plane itself is MCP-addressable; agents can reconfigure routing/providers without the UI.
- Roadmap: "Higress Agent" for autonomous traffic governance (CNCF blog).
- AI-assisted Nginx-Ingress migration tooling (LLM converts 60+ resources in ~30 min — marketed workflow).

## 6. Console / Dashboard

- Out-of-the-box web console (separate `higress-console` repo/releases; demo at demo.higress.io): admin bootstrap on first login; LLM Provider Management (API keys, failover policy per provider); AI Route Config (domain, model-match, fallback, consumer authorization); Strategy layer (auth, rate limit, RAG, prompt templates, semantic cache toggles); AI Dashboard (token metrics per provider/model); plugin marketplace UI with custom Wasm plugin upload and hot reload; MCP server configuration.

## 7. Performance Claims (published)

- "Hundreds of thousands of requests per second" in Alibaba production; 2+ years internal validation; 99.99% gateway availability backing Alibaba Cloud AI services.
- Route-change effectiveness "10x faster than nginx-ingress"; millisecond config effect; no-reload (vs Nginx reload breaking long-lived AI/gRPC streams).
- Initial config load >2min → ~3s (incremental loading, large-tenant case).
- Lower memory in high-bandwidth streaming scenarios (full streaming processing, no body buffering).
- No published head-to-head latency/QPS benchmark vs Kong/LiteLLM/Portkey from Higress itself; third-party AI-gateway benchmarks generally omit Higress (Kong's benchmark covers Kong/Portkey/LiteLLM only).

## 8. Release Velocity (recent)

- v2.1.10 (early 2026): hgctl agent, Higress API MCP Server. v2.2.0: Gateway API v1.4 + Inference Extension, Claude Code mode auth, Agent Session Monitor. v2.2.1: Qwen rerank/conversation compat, mergeConsecutiveMessages, Azure image concurrency. v2.2.2 (May 2026): modelToHeader routing, KlingAI, Bedrock prompt-cache retention, custom Claude URLs. Cadence: 40–80 changes per minor release; very active.

## 9. Weaknesses & Complaints (observed)

- **Operational complexity:** Envoy+Istio control plane, ConfigMaps, Redis dependency for MCP sessions/caching — reviewers note "beginners or small teams may find setup and operations somewhat complex"; clearly aimed at platform/infra teams. Not a single binary.
- **China-first ecosystem:** docs/blogs strongest in Chinese (higress.cn vs higress.ai split); some plugin docs English-lagging; Western community presence (Reddit/HN) nearly nonexistent — discovery and community support outside China is thin.
- **K8s-era debt:** Gateway API version skew bugs (v2.2.0 used deprecated v1alpha2 TLSRoute, broke with Gateway API v1.5 — controller sync loop); plugin image-path pull failures; Docker Desktop/WSL2 startup failures (proxy misconfig); Swagger UI empty request-body schemas.
- **Config model fragmentation:** behavior split across console UI, CRDs/annotations, ConfigMaps, and per-plugin YAML — no single declarative file for the whole gateway; quickstart install requires internet at startup.
- **Wasm plugin DX:** sandbox safety but harder debugging; ABI/runtime limitations vs native middleware; plugin ecosystem quality varies (55 plugins in-tree, "90% of scenarios" claim is marketing).
- **No per-request cost analytics/billing-grade spend tracking** comparable to Portkey/Helicone (token stats yes; USD cost attribution is not a first-class surface in OSS).
- Enterprise content-security and multi-AZ failover gated to Alibaba Cloud commercial edition — OSS guardrails depend on Alibaba content-safety service integration.
- Vendor-perception risk for Western buyers: Alibaba-governed (CNCF Sandbox helps but is the lowest tier).

## 10. Implications for a New OSS Gateway (steal/avoid)

**Steal:**
1. MCP-addressable control plane (their API MCP server + `hgctl agent`) — agents managing the gateway is exactly the agent-first control-plane thesis; Higress validates it but buries it in hgctl/K8s complexity.
2. openapi-to-mcp + database-to-mcp declarative converters — fastest "existing API → agent tool" path in the market.
3. Dual-protocol frontend (OpenAI + Claude `/v1/messages` with auto-detect) and Claude Code OAuth mode — coding-agent traffic is a real segment.
4. Token-level rate limiting + API-key pools with health-based token failover.
5. No-reload streaming-safe config updates as a marketing wedge (long-lived SSE/MCP sessions).
6. Agent Session Monitor (multi-turn conversation tracking from gateway logs).

**Avoid:**
- Their complexity tax (Envoy/Istio/ConfigMap/Redis). A true single binary with one config file + first-class CLI/MCP is the open flank.
- Console/CRD/ConfigMap config fragmentation.
- Docs/community split across languages and domains.

## Sources

- https://github.com/alibaba/higress (README)
- https://higress.ai/en/ai-gateway/ ; https://higress.ai/en/comparison/ ; https://higress.ai/en/docs/ai/quick-start/ ; https://higress.ai/en/docs/ai/mcp-quick-start/
- https://github.com/alibaba/higress/blob/main/plugins/wasm-go/extensions/ai-proxy/README.md
- https://github.com/alibaba/higress/tree/main/plugins/wasm-go/extensions (plugin inventory)
- https://github.com/alibaba/higress/releases
- https://www.cncf.io/blog/2026/03/25/higress-joins-cncf-delivering-an-enterprise-grade-ai-gateway-and-a-seamless-path-from-nginx-ingress/
- https://www.cncf.io/blog/2026/04/23/from-ingress-nginx-to-higress-migrating-60-resources-in-30-minutes-with-ai/
- https://deepwiki.com/alibaba/higress/6.3-hgctl-mcp-and-agent-commands ; https://github.com/higress-group/higress-ops-mcp-server ; https://github.com/higress-group/openapi-to-mcpserver
- https://www.alibabacloud.com/blog/higress-mcp-service-management-helps-build-a-private-mcp-market_602344
- https://www.alibabacloud.com/blog/higress-has-supported-the-new-gateway-api-and-its-ai-inference-extension_602891
- https://github.com/alibaba/higress/issues/3578 (Gateway API v1.5 breakage)
- Third-party comparisons: konghq.com AI-gateway benchmark, particula.tech, slashllm.com, nsddd.top market analysis
