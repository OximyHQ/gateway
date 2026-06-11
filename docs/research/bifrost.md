# Bifrost (Maxim AI) — Competitive Intelligence Report

**Date:** 2026-06-10
**Category:** LLM gateway + MCP gateway (open-core, Go)
**Vendor:** Maxim AI (also sells the Maxim eval/observability platform; Bifrost is their gateway product)
**Repo:** https://github.com/maximhq/bifrost — Apache 2.0, ~5.7k stars / 734 forks, very active (releases every 1–2 days, 1,700+ tagged releases across modules)
**Docs:** https://docs.getbifrost.ai • Marketing: https://www.getmaxim.ai/bifrost • Pricing: https://www.getmaxim.ai/bifrost/pricing

---

## 1. Positioning

"Fastest enterprise AI gateway (50x faster than LiteLLM) with adaptive load balancer, cluster mode, guardrails, 1000+ models support & <100 µs overhead at 5k RPS." Bifrost is explicitly positioned as the post-LiteLLM gateway: a compiled Go single binary replacing a Python proxy, with an "honest benchmarks" narrative. Marketing escalated from "40x" to "50x faster than LiteLLM" during 2025–2026. They also lean on a March 2026 LiteLLM supply-chain incident in content marketing ("compiled Go binary eliminates an entire class of attack vectors").

It is one control plane for three things: **LLM gateway** (routing/failover/caching), **MCP gateway** (tool governance for agents), and an emerging **agent gateway** (on the 2026 Q1 roadmap).

## 2. Implementation & Architecture

- **Language:** Go (~75%), TypeScript UI (~17%, React + Vite), small Python (4.5%). Requires Go 1.26.x to build.
- **Multi-module Go workspace:** `core/` (engine: request queuing, provider lifecycle, routing, MCP, object pooling, streaming), `framework/` (config/log storage, vector stores, tracing, encryption), `transports/bifrost-http/` (HTTP gateway, 27 endpoint handlers + SDK compatibility layers), `plugins/` (9 modules: governance, telemetry, logging, semantic cache, OpenTelemetry, mocking, JSON parsing, Maxim observability, LiteLLM compat), `ui/`, `tests/e2e/` (Playwright), `docs/` (Mintlify MDX).
- **Performance engineering details:** fasthttp (not net/http) for provider calls (Bedrock excepted, needs HTTP/2); per-provider isolated worker pools (failures don't cascade); sync.Pool object pooling for channels/responses/buffers; ~10 ns weighted API-key selection; 1.67 µs average queue wait.
- **Request flow:** parse → SDK conversion → plugin pre-hooks → provider selection → queue → worker → provider call → post-hooks → response. Plugins hook LLM, MCP, HTTP transport, and observability layers; pre-hooks run in registration order, post-hooks in reverse.
- **Two consumption modes:** standalone HTTP gateway with web UI, or **embedded Go SDK** (import the gateway as a library — zero network hop).

## 3. Deployment & Config Model

- Install paths: `npx -y @maximhq/bifrost` (30-second start), `docker run -p 8080:8080 maximhq/bifrost`, Go package import, Kubernetes manifests, Helm chart, Terraform.
- Zero-config startup; then config via **three coequal surfaces**: web UI, REST API ("API-driven configuration"), or file (`config.json`). Dynamic provider config at runtime.
- Enterprise: cluster mode (multi-node, zero-downtime deploys, claimed 99.99% uptime), VPC/on-prem/air-gapped deployment (file-scheme pricing URLs added for air-gapped), restricted-egress overrides.

## 4. Provider & API Surface

- **20–23+ providers / "1000+ models"**: OpenAI, Anthropic, AWS Bedrock, Google Vertex, Azure, Groq, Cerebras, Mistral, Cohere, Ollama, Hugging Face, Perplexity, vLLM, SGLang, etc. (9 of these are "OpenAI-compatible" implementations sharing one code path.)
- One **OpenAI-compatible unified API** plus **drop-in SDK compatibility layers**: point the OpenAI, Anthropic, AWS Bedrock, Google GenAI SDKs at Bifrost by changing only the base URL; also native integrations for LiteLLM, LangChain, PydanticAI.
- Multimodal: text, images, audio, streaming. Anthropic prompt-caching passthrough (top-level cache control) added by user demand.
- Provider interface internally is 30+ methods (their own AGENTS.md flags this as a friction point for adding operations).

## 5. Routing, Reliability, Caching

- Automatic failover across providers and models, weighted load balancing across multiple API keys per provider, custom routing rules, model aliasing/renaming (shipped after being a top-requested feature).
- **Adaptive load balancing** (latency/health-aware across providers) — Enterprise.
- **Semantic caching** (vector-similarity response dedup) + simple caching — OSS.
- Automatic retries with exponential backoff; per-provider health monitoring (Enterprise).

## 6. Governance & Security

- **Virtual keys** are the primary governance primitive: access control, budgets, rate limits per key. Hierarchy: virtual keys → teams → customers/orgs (multi-tenant), with provider-level and model-level budget controls (recently shipped), user attribution on virtual keys, paginated VK APIs for large fleets.
- OSS gets budgets/rate-limits/VKs and a **governance CLI**; Enterprise adds: SAML/OIDC SSO (Okta, Entra), SCIM/directory sync, RBAC, vault integrations, audit logs (SOC 2 / GDPR / HIPAA / ISO 27001 narratives), log exports, Datadog connector.
- **Guardrails are Enterprise-only**: content-safety integrations with AWS Bedrock Guardrails, Azure, Google, Patronus AI.
- Hardening seen in changelog: SSRF URL validation, secure key handling via env vars.

## 7. MCP Gateway (their second product surface)

- Bifrost is both **MCP client** (connects to external MCP servers: filesystem, web search, DBs, custom APIs) and **MCP server** (Claude Desktop / agents connect to Bifrost as a single MCP endpoint).
- Transports: STDIO, HTTP, SSE, with auto-reconnect/backoff.
- **Tool discovery** is automatic at startup and cached (~100–500 µs discovery, ~50–200 ns tool filtering).
- **Execution model is approval-first:** LLM tool calls are suggestions; execution requires explicit call to `/v1/mcp/tool/execute` — unless Agent Mode pre-approves.
- **Agent Mode:** Bifrost becomes the agent runtime — auto-executes allowlisted tools (`tools_to_auto_execute`), feeds results back, loops to `max_depth`. Pitched for read-heavy workflows.
- **Code Mode (differentiator):** instead of exposing 100+ tool schemas to the model, Bifrost exposes connected MCP servers as a virtual filesystem of Python `.pyi` stubs; the model reads only what it needs, writes one orchestration script, and Bifrost runs it in a **sandboxed Starlark interpreter**. Claims 50%+ (up to 92.8%) token reduction and 40–50% latency reduction vs classic MCP.
- MCP auth: 5 types — none, static headers, OAuth, per-user OAuth, per-user custom headers — with "MCP Sessions" for inspecting/re-authing/revoking per-user credentials. Tool filtering per request, per client, and per virtual key.

## 8. Dashboard / UI

Built-in web UI at :8080: visual provider/key configuration, real-time monitoring + analytics, live log view, prompt repository + playground (OSS), 20+ workspace features. UI is React+Vite; E2E tested with Playwright keyed on data-testid attributes.

## 9. Observability

Prometheus metrics (native), OpenTelemetry metrics + traces, distributed tracing, built-in dashboards, request-header capture with wildcard patterns, content-logging controls for OTel exports, Datadog connector (Enterprise), automated log export (Enterprise), plus first-party plugin shipping data into Maxim's eval/observability platform (their upsell path).

## 10. Extensibility

- Custom plugin framework: middleware pre/post hooks in **Go or WASM**. Community example: "Heimdall" — a user-built plugin doing price/performance-based model routing on top of Bifrost.
- Plugin surface covers LLM calls, MCP calls, HTTP transport, and observability.

## 11. Performance Claims & Benchmark Methodology

Published at https://www.getmaxim.ai/bifrost/resources/benchmarks and a "How We Benchmarked" dev.to post:

- **Setup:** identical AWS EC2 instances (t3.medium 2vCPU/4GB and t3.xlarge 4vCPU/16GB), us-east-1, same network; **mocked OpenAI endpoint with fixed 60 ms response** to isolate gateway overhead; 60+ second sustained runs; 500 virtual users; ~10 KB payloads.
- **500 RPS head-to-head (t3.medium):** Bifrost vs LiteLLM — P50 804 ms vs 38.65 s (48x), P99 1.68 s vs 90.72 s (54x), throughput 424 vs 44.8 req/s (9.5x), success 100% vs 88.78%, peak memory 120 MB vs 372 MB (68% less). Gateway overhead on 60 ms mock: 0.99 ms vs ~40 ms ⇒ the headline "40x".
- **5,000 RPS Bifrost-only stress:** 100% success; internal overhead 59 µs (t3.medium) / 11 µs (t3.xlarge); avg queue wait 1.67 µs; key-selection ~10 ns.
- **Caveats they admit:** only LiteLLM is benchmarked (no Portkey/Kong/Helicone/OpenRouter comparison); "validate higher targets on your own instance sizes, provider mix, payload profile."
- **Critique to exploit:** numbers like "P50 of 38.65 s" mean LiteLLM was driven far past saturation on a 2-vCPU box — it's a saturation benchmark, not a like-for-like overhead measurement; HN commenters call out "poor design of benchmarks" and want independent evaluation. The 40x/50x figure is overhead-ratio marketing, not end-to-end latency (a provider call is 1–30 s; 40 ms vs 1 ms overhead is invisible in most workloads). Image size is also larger than minimal Go rivals (69.8 MB vs GoModel's 17 MB, per HN).

## 12. Pricing / Licensing

- **OSS (Apache 2.0, $0, self-host):** unified API/1000+ models, drop-in SDK replacement, OTel metrics+traces, real-time dashboard, virtual keys + budgets + rate limits, custom routing + fallbacks, semantic + simple caching, MCP gateway incl. code mode, prompt repo/playground, plugin dev, governance CLI, Discord support.
- **Enterprise (custom quote, 14-day trial, "book a demo"):** guardrails, clustering/zero-downtime, adaptive load balancing, health monitoring, SAML/OIDC SSO, SCIM, RBAC, vault integrations, audit logs + log exports, compliance certs, VPC/on-prem/air-gapped, dedicated engineers + SLAs + private Slack, custom plugin development. No public numbers anywhere.
- Versioning (June 2026): core v1.5.18, HTTP transport v1.5.11, framework v1.3.18, enterprise base v1.4.8; coordinated multi-module releases every 1–2 days.

## 13. Agent Experience (AX) Notes

- **Repo ships AGENTS.md** (for both `main` and `dev`) — a detailed map for coding agents: module layout, request-flow, plugin hook ordering, pooling gotchas, make targets (`make test-core PROVIDER=...`, `make test-mcp`, `make run-e2e`), style rules. They explicitly design for AI contributors.
- **MCP server mode** means agents (Claude Desktop, Claude Code) consume Bifrost as a single governed MCP endpoint; Maxim publishes a "Using an MCP Gateway with Claude Code" guide. Known rough edge: issue #3365 "bifrost cli + kilo + claude code is giving connection refused."
- **Agent Mode** makes the gateway itself an agent loop runtime (auto-exec allowlisted tools, max_depth).
- **Code Mode** is the most agent-native idea in the space: tools-as-Python-stubs + sandboxed Starlark execution to crush tool-schema token bloat.
- **API-first config:** everything settable via REST as well as UI/file; there is a **governance CLI** in OSS. "Agent gateway" + agent authZ/discovery are on the 2026 Q1 roadmap — they are moving up the stack from LLM gateway to agent control plane.
- Subscription-auth gaps for agents: open issues for GitHub Copilot as a provider (OAuth/device-code) and ChatGPT/Codex OAuth device flow — agent users want to route subscription models, not just API keys.

## 14. Community Sentiment

**Praise (HN):** "Bifrost is the way" vs LiteLLM; multiple users migrating off LiteLLM citing its code quality, slowness, scalability, and unresponsive maintainers; "overcomes performance issues of litellm and is easy to get going"; used as router in front of OpenRouter; plugin extensibility praised.

**Complaints / skepticism:**
- "Fast but many features paywalled" (smcleod) — guardrails, HA/clustering, adaptive LB, SSO/RBAC/audit all Enterprise.
- Benchmark skepticism: "poor design of benchmarks," builder-run tests, only-LiteLLM comparisons, red flags in presentation.
- Crowded market skepticism ("gateway market is tough").
- Larger Docker image than leaner Go rivals.
- Provider/auth gaps: GitHub Copilot provider, ChatGPT device-code OAuth, bidirectional API-format conversion (#3378), per-key proxy settings, finer observability controls.
- Internal complexity debt acknowledged in their own AGENTS.md: 30+ method provider interface; OpenAI changes ripple across 9 providers.
- Velocity cuts both ways: releases every 1–2 days implies fast fixes but churny surface.

## 15. Lessons for Our Gateway

1. **Single Go binary + embeddable SDK** is the architecture users are fleeing LiteLLM toward — table stakes for us, and they validated demand.
2. **Code Mode is the standout idea worth stealing**: tool stubs + sandboxed script execution; quantified 50–92% token savings is a compelling, agent-native pitch.
3. **Their biggest strategic weakness is the open-core line**: guardrails, clustering/HA, adaptive LB, SSO/RBAC/audit are paywalled — a truly open gateway that ships these in OSS attacks them directly (HN already complains).
4. **Benchmark theater works but invites blowback** — publish independent-reproducible harnesses with realistic (non-saturation) load and compare more than one rival.
5. **AGENTS.md + API-first config + governance CLI + MCP-server-of-the-gateway** is the emerging AX pattern; their "agent gateway" roadmap shows where the category is heading.
6. **Virtual keys → teams → customers hierarchy** with model/provider-level budgets is the governance shape enterprises ask for.
7. Gaps to exploit immediately: subscription-model OAuth (Copilot/ChatGPT device flow), lean image size, public pricing, independent benchmarks, OSS guardrails.

---

### Sources
- https://github.com/maximhq/bifrost (README, AGENTS.md, issues, releases, discussion #1524)
- https://docs.getbifrost.ai/overview, https://docs.getbifrost.ai/mcp/overview
- https://www.getmaxim.ai/bifrost, https://www.getmaxim.ai/bifrost/pricing, https://www.getmaxim.ai/bifrost/resources/benchmarks
- HN: https://news.ycombinator.com/item?id=46215001 + Algolia comment search (bifrost/litellm, bifrost/benchmark)
- dev.to: "How We Benchmarked Bifrost against LiteLLM", "What is Code Mode in Bifrost MCP Gateway"
- Medium: "LiteLLM vs Bifrost in 2026: An Honest Comparison After the Supply Chain Wake-Up Call"
