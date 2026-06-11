# SYNTHESIS — Competitive Landscape for an Open-Source, Agent-First LLM+MCP Gateway

**Date:** 2026-06-10 · **Inputs:** 65 research agents (45 reports in `undefined/`, plus `.competitive-intel/`, `.competitive-research/`, `.research/`, `research/`, `/tmp/competitive-*`) · **Gap-check:** `undefined/gap-check.md` confirms category coverage is complete.

**Product thesis being tested:** unified LLM gateway + MCP gateway, single binary + dashboard, with an **agent-first control plane** (CLI + admin-MCP + config-as-code).

**Verdict in one line:** every feature in this market exists somewhere, but **no product combines (a) LLM+MCP in one OSS single binary, (b) a control plane agents can operate (MCP admin server + AXI-grade CLI + one config source of truth), and (c) compiled-language performance with batteries-included governance/observability** — that triple intersection is empty and is the product.

---

## 1. Master Feature Matrix

Legend: ✅ = ships it (table stakes or strong) · ⭐ = best-in-class / differentiator · ◐ = partial, beta, or buggy · 💰 = exists but paywalled (enterprise/cloud-only) · ✖ = absent · n/a = out of scope for that product.

### 1a. LLM-gateway feature areas × major competitors

| Feature area | LiteLLM | Portkey | OpenRouter | Helicone | Bifrost | Kong | Cloudflare | Vercel | TensorZero | Envoy AI GW | agentgateway | Higress | APISIX | TrueFoundry |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| OpenAI-compatible unified API (chat) | ⭐ 100+ providers | ✅ 1,600+ models | ✅ 500+ models | ✅ 100+ | ✅ 20+ prov | ✅ | ✅ ~24 prov | ✅ ~45 prov | ✅ ~20 native | ✅ 16-20 | ✅ | ✅ 40+ | ✅ ~9 | ✅ 25+ |
| Anthropic /v1/messages ingress (Claude Code drop-in) | ⭐ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ◐ | ✅ | ✅ | ⭐ (auto-detect + Claude Code OAuth mode) | ✖ | ✅ |
| OpenAI Responses API | ✅ | ✅ | ◐ beta | ◐ | ✅ | ✅ | ⭐ /ai/v1/responses | ✅ | ✅ | ✅ | ✅ | ◐ | ✖ | ✅ |
| Gemini generateContent ingress | ◐ (AI Studio only) | ✖ | ✖ | ✖ | ◐ compat | ✖ | ✖ | ✖ | ✖ | ✖ | ✖ | ◐ | ✖ | ✖ |
| Streaming SSE fidelity / tool-call deltas | ◐ (chronic regressions) | ✅ | ✅ | ⭐ byte-faithful passthrough | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Embeddings / images / audio (TTS+STT) | ⭐ all | ✅ all | ✅ all | ✅ | ✅ | ✅ (3.11) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | embeddings only | ✅ all |
| Batch + Files APIs | ⭐ cross-provider | ✅ + custom batching | ✖ | ✖ | ✅ | ✅ 3.11 | ✖ | ✖ | ✅ provider batch | ✖ | ✖ | ✖ | ✖ | ✅ |
| Realtime (WebSocket/WebRTC) | ✅ GA + WebRTC | ◐ OpenAI-only | ✖ | ✖ | ✖ | ✖ | ⭐ any-provider WS wrapper + Realtime/Live | ✖ | ✖ | ✖ | ✅ OpenAI Realtime | ✖ | ✖ | ✅ |
| Video generation | ⭐ /videos (only one) | ✖ | ◐ async endpoint | ✖ | ✖ | ✖ | ✖ | ✅ Veo/Kling | ✖ | ✖ | ✖ | ✖ | ✖ | ✖ |
| Rerank endpoint | ✅ | ✅ | ✅ | ✖ | ✖ | ✅ | ✖ | ✅ | ✖ | ✅ | ✖ | ✖ | ✖ | ✅ |
| Cost-tracked native passthrough routes | ⭐ 16 variants | ✅ | n/a | ⭐ passthrough-first | ✅ | ✅ llm_format | ✅ provider routes | ✖ | ✅ raw + escape hatches | ✖ | ✅ raw | ✅ | ✖ | ✅ raw proxy |
| Fallback chains / retries / cooldowns | ✅ 4 fallback types | ⭐ nested config tree | ⭐ marketplace pooling | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ + hedging | ✅ | ✅ | ✅ | ✅ TrueFailover |
| Load balancing (weighted/latency) | ✅ 6 strategies | ✅ | ⭐ inverse-square price + 30s outage filter | ⭐ P2C+PeakEWMA | ⭐ adaptive (💰) | ⭐ 7 balancers incl. tpot | ✅ | ✅ sort:cost/ttft/tps | ✅ | ✅ | ✅ | ✅ | ✅ | ⭐ flap-proof latency |
| Semantic / learned / preference routing | ✖ | ◐ conditional | ⭐ Exacto quality routing | ✖ | ✖ | ⭐💰 semantic routing | ◐ dynamic route graphs | ✖ | ◐ variants | ✖ | ◐ content-based | ◐ | ✖ | ✖ |
| Exact response caching | ✅ | ✅ | ✅ | ⭐ buckets/seeds/ignore-keys | ✅ | ✅ | ✅ | ✖ | ✅ read_only/max_age | ✖ | ✖ | ✅ | ✅ exact only | ✅ |
| Semantic caching | ✅ (redisearch pain) | 💰 | ✖ | ✖ | ⭐ dual-layer + threshold header | 💰 | ✖ (promised 2024, never) | ✖ | ✖ | ✖ (issue #30) | ✖ | ✅ | ✖ | ✅ |
| Provider prompt-cache passthrough + affinity routing | ⭐ PromptCachingDeploymentCheck | ✅ | ⭐ sticky + cache_discount | ✅ | ✅ | ✅ | ◐ | ✅ auto markers | ✅ | ⭐ unified cache_control translation | ◐ | ✅ | ✖ | ✅ |
| Virtual keys + per-key budgets | ⭐ | ✅ | ⭐ provisioning API | ◐ header-declared | ⭐ 4-level cascade + native-header VKs | ◐ consumers | ✖ per-key | ✅ | ✖ | ✖ | ◐ clumsy | ✅ | ✖ | ✅ |
| USD budgets / spend limits (hard block) | ✅ (bypass-bug history) | 💰 | ✅ guardrails | ✅ GCRA $-limits | ✅ | ✅ cost-as-rate-limit | ◐ beta + fallback-to-cheaper | ✅ | ✖ | ✖ | ◐ | ◐ token quota | ✖ | ✅ quarterly |
| Token/TPM/RPM rate limiting | ✅ | ✅ | ✅ | ⭐ GCRA req/token/$ | ✅ | ⭐ 6 AND-able dimensions | ◐ gateway-global only | ◐ | ✅ scoped | ✅ CEL token costs | ✅ | ✅ + key-pool rotation | ✅ per-instance quota | ✅ one rule grammar |
| Teams/orgs hierarchy + RBAC | ✅ (SSO 💰) | ✅ (SCIM 💰) | ✅ young | ◐ | 💰 | 💰 | ◐ | ✅ | ✖ | ✖ | ✖ | ◐ | ◐ | ✅ |
| SSO / SCIM / audit logs | 💰 | 💰 | ◐ | 💰 | 💰 | 💰 | ◐ ZT | ◐ | ✖ | ✖ | 💰 | 💰 Alibaba | ✖ | ✅ |
| Cost tracking + price registry | ⭐ auto-sync prices | ⭐ open pricing repo | ⭐ usage.cost inline always | ⭐ OSS cost package | ✅ | ✅ | ◐ (broken cases) | ✅ /v1/generation | ✅ ClickHouse | ✖ $ | ◐ Prometheus only | ◐ tokens only | ✖ $ | ✅ |
| Request logs w/ payloads + dashboard | ✅ | 💰 cloud | ✅ | ⭐ + HQL SQL | ✅ live WS logs | 💰 Konnect | ✅ capped | ✅ | ✅ own ClickHouse | ✖ | ✖ | ✅ | ◐ | ✅ |
| OTel GenAI semconv / Prometheus | ✅ (metrics 💰) | ✅ OTLP sink | ◐ export-only | ✅ | ✅ | ✅ | ◐ GraphQL | ◐ | ⭐ dual formats | ⭐ exact semconv | ✅ | ✅ | ✅ | ✅ |
| Agent session/trace grouping | ✅ | ✅ | ◐ | ⭐ session headers | ✅ | ◐ | ◐ | ◐ | ⭐ episodes | ◐ | ◐ | ⭐ Agent Session Monitor | ✖ | ✅ agent traces |
| Guardrails framework (PII/injection/moderation) | ⭐ 40+ adapters (governance 💰) | ⭐ verdict algebra | ✅ regex+PII | ◐ 2-tier | 💰 | 💰 + restore-mode PII | ✅ Llama Guard (+500ms) | ✖ | ✖ | ✖ | ✅ multi-layer | ✅ | ◐ regex/AWS | ⭐ 4-hook incl. MCP |
| Prompt management / versioning | ✅ pluggable backends | ✅ + partials | ◐ presets | ⭐ in-gateway compile, typed vars | ✖ | ◐ enforced templates | ✖ | ✖ | ⭐ functions/variants | ✖ | ✖ | ◐ | ✖ | ✅ registry |
| Experimentation / evals / A-B | ◐ mirroring | ◐ canary configs | ✖ | ✖ (deprecated own) | ✖ | ✖ | ✅ logs→datasets→evals | ✖ | ⭐ adaptive A/B + evals + optimization | ✖ | ✅ traffic split | ✅ canary | ✅ canary | ◐ |
| MCP gateway (federation, OAuth, tool ACL) | ⭐ OAuth/OBO/SigV4 + per-tool cost | ⭐ GA registry + RBAC | ✖ | ✖ (query-MCP only) | ⭐ + Code Mode + is-an-MCP-server | ⭐💰 OpenAPI→MCP + RFC8693 | ◐ separate product (CF One) | ✖ | ◐ own API via /mcp | ⭐ spec-complete + CEL authz | ⭐ Virtual MCP + A2A | ⭐ openapi/db→MCP + marketplace | ◐ mcp-bridge prototype | ⭐ Cedar tool policies + OAuth broker |
| A2A protocol gateway | ⭐ | ✖ | ✖ | ✖ | ✖ | ✅ 3.14 GA | ✖ | ✖ | ◐ | ✖ | ⭐ | ✖ | ✖ | ✅ agent gateway |
| Plugin/middleware extensibility | ✅ Python hooks | ✅ TS plugins | n/a | ◐ | ⭐ 4 hook ifaces incl. MCP hooks, Go/WASM | ⭐ PDK + schema.lua | ◐ | ✖ | ◐ escape hatches | ✅ ExtProc | ✅ CEL + ExtProc | ⭐ Wasm sandbox + hot reload | ✅ Lua | ◐ webhook guardrails |
| Admin REST API (full UI parity) | ⭐ full Swagger | ⭐ + Terraform | ⭐ provisioning/guardrails/presets APIs | ◐ | ⭐ =UI=file | ⭐ + decK | ✅ + Terraform | ✖ clickops | ✖ (TOML only) | ✖ (CRDs only) | ✖ (file/UI) | ✅ + MCP | ⭐ hot-reload | ◐ CLI yaml |
| CLI (admin) | ✅ litellm-proxy | ✅ npx portkey | ◐ | ✖ | ✅ governance CLI | ✅ decK | ◐ wrangler | ✖ | ◐ evals only | ◐ aigw run | ◐ agctl (build-from-src) | ⭐ hgctl agent (NL) | ✖ OSS | ✅ tfy apply |
| MCP control plane for the gateway itself | ✖ | ✖ (community wraps) | ✖ | ✖ | ◐ (is MCP server for tools, not admin) | ⭐ Konnect MCP (3 meta-tools, SaaS) | ⭐ API MCP server (search+execute) | ✖ | ◐ /mcp (API, not admin ops) | ✖ | ✖ | ⭐ Higress API MCP Server | ⭐ apisix-mcp | ✖ (CLI skills instead) |
| Config-as-code single source of truth | ✖ (yaml-vs-DB split brain) | ◐ config IDs | n/a | ◐ | ✅ config.json=API=UI | ⭐ decK dump/diff/apply | ◐ | ✖ | ⭐ GitOps TOML | ✅ CRDs | ✅ YAML/CRDs | ✖ fragmented | ✅ standalone PUT-config | ✅ YAML |
| Single binary / light footprint | ✖ Python 350-500MB | ✖ Node | n/a SaaS | ⭐ Rust ~15-30MB (dormant) | ⭐ Go single binary | ✖ | n/a | n/a | ◐ (+CH/PG/Valkey) | ✖ k8s | ◐ standalone mode | ✖ Envoy+Istio | ◐ +etcd | ✖ k8s+NATS+CH |
| Self-host / air-gap | ✅ | ✅ (CP caveats) | ✖ | ◐ rough | ✅ | ✅ | ✖ | ✖ | ✅ only | ✅ | ✅ | ✅ | ✅ | ◐ (license server) |
| llms.txt / .md agent docs | ✅ | ⭐ + llms-full | ⭐ + skills repo | ✅ | ✅ | ◐ | ✅ | ◐ | ✅ | ✖ | ⭐ + schema/CEL explorers | ◐ | ✖ | ◐ skills |
| Published perf (<5ms overhead class) | ✖ (collapses 400-500 RPS) | ◐ (~20-40ms real) | ◐ ~25-40ms edge | ⭐ <1ms, 3k RPS | ⭐ 11-100µs @5k RPS | ⭐ ~26k RPS p95 8ms | ◐ | ◐ ~200ms small | ⭐ <1ms p99 @10k QPS | ✅ 1-3ms | ⭐ <0.2ms p99 @30k | ◐ | ⭐ 0.2ms | ◐ ~3-4ms |

### 1b. MCP-gateway feature areas × MCP-focused competitors

| Feature area | ContextForge | Docker MCP GW | agentgateway | MetaMCP | MCPJungle | Obot | ToolHive | Microsoft (APIM/ODR) | Lasso/Invariant | Commercial wave (Runlayer/MintMCP/Natoma) |
|---|---|---|---|---|---|---|---|---|---|---|
| Multi-server federation, one endpoint | ✅ + cross-GW federation | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ vMCP | ✅ | ✅ | ✅ |
| Transport bridging stdio/SSE/streamable | ✅ all + WS | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ◐ stdio-centric | ✅ |
| Virtual/composite servers (curated tool subsets) | ⭐ core primitive | ✅ profiles | ✅ | ⭐ namespaces + tool rename/annotations | ✅ Tool Groups | ⭐ + rename/override | ✅ | ◐ | ✖ | ⭐ MintMCP Virtual MCP |
| Per-tool ACL / RBAC | ✅ teams | ✅ filters | ⭐ CEL + filtered tools/list | ◐ no RBAC | ◐ basic/💰 | ✅ roles | ⭐ Cedar default-deny | ◐ scope-based | ✖ | ⭐ 6-dim ABAC (Runlayer) |
| MCP OAuth 2.1 (inbound resource server) | ✅ + DCR | ◐ | ⭐ gateway-terminated | ✅ | ✖ bearer only | ✅ | ✅ OIDC | ✅ PRM flow | ✖ | ✅ |
| Per-user OAuth brokering to upstreams | ✅ identity propagation | ✅ flows | ◐ | ✖ | ◐ beta | ⭐ shim + RFC 8693 | ◐ | ✅ credential manager | ✖ | ⭐ vaults (Scalekit-class) |
| Secrets injection (never to client) | ✅ | ⭐ + --block-secrets default-on | ✅ | ◐ env interp | ✅ | ⭐ shim | ⭐ keyring/1P/Vault | ✅ | ◐ masking | ✅ |
| Tool-call audit log | ✅ | ✅ --log-calls | ◐ thin | ✖ | ✖ | ✅ | ✅ | ✅ + consent ledger | ✅ traces | ⭐ SIEM export |
| Threat detection (poisoning/rug-pull/shadowing) | ◐ | ⭐ frozen descriptions + signed images | 💰 Solo ent. | ✖ | ✖ | ◐ filters | ✅ provenance | ◐ | ⭐ scanners + toxic-flow DSL | ⭐ pre-approval scan pipelines |
| Dynamic/semantic tool discovery (anti context-bloat) | ◐ TOON compression | ⭐ mcp-find/add + code-mode | ✖ | ✖ | ✖ | ✖ | ⭐ find_tool/call_tool | ⭐ AgentCore semantic search (AWS) | ✖ | ✖ |
| Gateway admin over MCP (agent-operable) | ✖ REST only | ⭐ Dynamic MCP meta-tools (session-only) | ✖ | ✖ (roadmap) | ✖ | ⭐ obot setup skill | ✅ own MCP server + hooks | ⭐ registry-as-MCP-server (GA) | ✖ | ✖ dashboard-only |
| Sandboxing / isolation of servers | ◐ | ⭐ container-per-server | ✖ | ✖ | ✖ raw child procs | ✅ containers | ✅ containers + permission profiles | ⭐ OS agent session (ODR) | ◐ | ✅ |
| Registry/catalog + marketplace sync | ✅ catalog + bulk import | ⭐ 300+ signed, OCI artifacts | 💰 | ◐ | ◐ | ✅ 84+ vetted | ⭐ multi-source Registry Server | ✅ API Center + Group Policy enforcement | ✖ | ✅ 18k catalog (Runlayer) |
| LLM gateway in same product | ◐ (pair w/ LiteLLM) | ✖ | ⭐ yes (Rust, one binary-ish) | ✖ | ✖ | ◐ own chat only | ✖ (thv llm = plumbing) | ◐ Unified Model API preview | ◐ Invariant proxies both | ✖ |
| Single binary | ✖ Py+PG+Redis | ◐ Docker-coupled | ◐ | ✖ Node+PG, 2-4GB | ⭐ Go | ✖ PG+K8s | ✖ 6 repos | n/a | ◐ | n/a SaaS |
| Published perf | ~800 RPS/pod | ✖ none | ⭐ 500k QPS claim | ✖ none | ✖ none | ✖ none | ✖ (token claims only) | ✖ none | ✖ none | ✖ none |

### 1c. Adjacent categories (absorb-or-integrate calls)

| Category | Leader(s) | Our posture |
|---|---|---|
| K8s inference routing (KV-cache/queue-aware) | llm-d, NVIDIA Dynamo, AIBrix, GIE EPP | **Integrate**: speak the GIE EPP ext-proc protocol; treat self-hosted fleets as backends |
| Learned per-prompt model routing | Not Diamond, Martian, RouteLLM, Arch-Router | **Absorb light**: human-readable policy routing (Arch-Router style) + ND-style knobs (cost-quality dial, max-cost cap); pluggable router seam |
| AI firewall / runtime security | Prisma AIRS(+Portkey), Cisco, Lakera, F5 | **Integrate**: Lakera-shaped single-endpoint external-guardrail contract; structured detection fields + Reject/Modify/Annotate verbs |
| Billing/metering/monetization | Stripe(+Metronome), OpenMeter(Kong), Lago, Revenium | **Export adapters**: idempotent canonical event → Stripe meter_events / OpenMeter CloudEvents / Lago /events / OTLP GenAI; keep credits ledger + real-time enforcement in-gateway |
| Agent identity / XAA | Okta, Descope, Scalekit, SPIFFE | **Implement**: MCP Enterprise-Managed Authorization (ID-JAG), CIMD, RFC 8693 token-exchange vault — first OSS mover slot is open |
| Sandboxing/egress runtimes | E2B, Daytona, Modal, Vercel Sandbox | **Be the egress target**: accept Vercel forwardURL / Anthropic srt BYO-proxy traffic; unified MCP+egress audit log |
| Tool/auth platforms | Composio, Arcade | **Interop**: URL Elicitation (accepted MCP SEP), auth-interrupt pattern; don't rebuild 1,000-app catalogs |
| Registries | Smithery, Glama, PulseMCP, official MCP Registry | **Conform**: implement the official Generic Registry OpenAPI as a private subregistry; consume PulseMCP-style enriched metadata |

---

## 2. Table-Stakes Checklist (v1 MUST-HAVES — "nobody leaves for a small thing")

### Unified API & protocols
- [ ] OpenAI-compatible `/v1/chat/completions` over base-URL swap; provider/model slugs; `/v1/models` listing
- [ ] **Anthropic `/v1/messages` ingress** with `anthropic-beta`/`anthropic-version` header forwarding (Claude Code hard-requires; Anthropic publishes gateway requirements)
- [ ] OpenAI **Responses API** with exact SSE event-sequence reproduction (strict clients reject partial sequences); conform to openresponses.org "Router" role
- [ ] `/v1/embeddings`, images generation, TTS/STT audio, rerank (Cohere format), batch + files passthrough
- [ ] Streaming SSE with normalized chunk/finish_reason semantics; tool-call deltas preserved in mixed outputs; usage in final chunk (`stream_options.include_usage`); never lose usage on aborted streams
- [ ] Tool/function calling translation incl. parallel calls, both directions; structured outputs (json_schema → Anthropic output_format / Gemini responseSchema), with forced-tool-call emulation fallback
- [ ] Vision/multimodal content-part translation; extended-thinking/reasoning params (unified `reasoning_effort` knob à la Envoy)
- [ ] **Cost-tracked native passthrough routes** (escape hatch so new provider features never block on translation)
- [ ] Provider-agnostic **prompt caching**: `cache_control` translation (Anthropic/Bedrock/Vertex/Gemini), cached-token accounting, never silently drop cache directives
- [ ] Explicit **UnsupportedOperationError** + dropped-param warnings instead of silent degradation (Bifrost model); per-pair fidelity matrix in docs
- [ ] `count_tokens` endpoint unified across providers
- [ ] MCP 2025-11-25 spec + **2026-07-28 stateless-core RC readiness** (Mcp-Method/Mcp-Name headers, ttlMs/cacheScope, Tasks, URL elicitation); run the conformance suite (SEP-2484) in CI

### Routing & reliability
- [ ] Priority fallback chains (multi-vendor) triggered on configurable error classes; 4-type taxonomy (general / context-window / content-policy / default)
- [ ] Retries with exponential backoff + jitter; honor provider `Retry-After`
- [ ] Weighted LB across keys/deployments; latency-aware strategy; per-deployment cooldown/passive circuit breaker with auto-recovery
- [ ] Per-request routing overrides in body (models[], provider prefs, disable_fallbacks) — OpenRouter-style agent self-service
- [ ] Response metadata: which deployment served, whether fallback fired (x-served-by / x-fallback headers)
- [ ] Request timeouts: global, per-deployment, and streaming-aware (TTFT vs total, TensorZero-style)
- [ ] Multi-instance state sharing (Redis optional; gossip preferred to keep single-binary story)
- [ ] Canary / % traffic split

### Governance & multi-tenancy
- [ ] Virtual keys: budgets (USD, multi-window), TPM/RPM/parallel limits, model allowlists, expiry/TTL, revoke, regenerate, metadata/tags
- [ ] Teams → orgs hierarchy; per-team budgets and model access; end-customer budgets without key issuance (LiteLLM `/customer` pattern)
- [ ] Hard budget block BEFORE provider call + soft alert thresholds (85/95%) to Slack/webhook; fail-closed enforcement (LiteLLM's fail-open bypass bugs are the cautionary tale)
- [ ] Key hierarchy: **provisioning keys** (control-plane-only, mint scoped child keys) separate from data-plane keys (OpenRouter pattern)
- [ ] 429s with remaining-quota headers; budget precedence documented
- [ ] RBAC admin/member minimum; **basic SSO free** (undercut the SSO tax); SCIM/org-scale RBAC can be paid
- [ ] Audit log of admin actions
- [ ] One governance model spanning **models AND MCP tools** (per-key tool allowlists — only Bifrost does this in OSS today)

### Cost & token accounting
- [ ] Cost from provider-reported usage (never local tokenization as primary); per-request cost inline in response (`usage.cost` + response header)
- [ ] Maintained price registry incl. cache read/write, reasoning, audio, tiered >128k/>200k pricing; hot-reload day-0 prices (consume models.dev / LiteLLM JSON); custom price overrides per deployment and per request
- [ ] Correct cached-token math (the market leader's 10x overcharge bugs are a wedge); streaming usage correctness
- [ ] Spend APIs grouped by key/team/user/model/tag; spend dashboard; export
- [ ] Custom per-request metadata/tags for attribution

### Caching
- [ ] Exact-match response cache (SHA-256 of model+endpoint+body, tenant-scoped); Redis + in-memory L1
- [ ] Per-request controls via header AND body: TTL, skip, no-store, namespace/seed; cache-status response headers (HIT/MISS/age)
- [ ] Only cache 200s; streaming replay; cache ops endpoints (ping, delete/clear)
- [ ] Cache analytics (hit rate, $ saved)

### Observability
- [ ] Request logs with full payloads, cost, latency, TTFT, errors; filterable/searchable; content-logging opt-out at global/key/request level (metadata-only mode)
- [ ] Prometheus `/metrics` (AUTHENTICATED by default — LiteLLM leaks PII here) + OTel GenAI semconv traces/metrics (TTFT, time-per-output-token) + **MCP semconv (v1.39)**
- [ ] Session/trace grouping via headers (Helicone-style hierarchical session paths)
- [ ] Recipes for Langfuse/Datadog/Grafana; webhook alerts on spend/error/latency
- [ ] Self-overhead response header (x-overhead-duration-ms — LiteLLM's one great idea)

### Guardrails & security
- [ ] Pre/post/during-call hook stages with block/mask/flag/log actions; observe-only (logging_only) mode for rollout
- [ ] PII detection/redaction (Presidio-class), content moderation (Llama Guard/OpenAI/Bedrock/Azure adapters), prompt-injection check, regex/keyword topic bans
- [ ] Generic HTTP guardrail webhook contract (Lakera-shaped: POST OpenAI-format, verdict + optional body override)
- [ ] Per-key/team/default_on attachment; per-request enablement; verdicts in logs AND in-band (applied-guardrails header)
- [ ] **MCP-stage guardrails**: pre-tool-call block + post-tool-result scan
- [ ] Secrets-shaped-data blocking on tool payloads (Docker --block-secrets default-on)

### MCP gateway
- [ ] Federate N servers behind one endpoint; stdio/SSE/streamable-HTTP bridging both directions; session lifecycle handling + stateless-RC support
- [ ] Tool namespacing (server__tool), per-server enable/disable, per-tool allowlists per key/team
- [ ] **Virtual/composite servers** with tool rename/redescribe/annotation overrides
- [ ] Inbound OAuth 2.1 resource server (PRM, WWW-Authenticate, PKCE, CIMD + DCR fallback); outbound credential injection + OAuth brokering with refresh
- [ ] Tool-call audit log (who/tool/args/outcome); health checks; OpenAPI→MCP conversion
- [ ] Registry import (official MCP Registry API shape, `updated_since` sync); tool-description pinning/hashing (rug-pull alert)
- [ ] Client auto-config for Claude Code/Desktop, Cursor, VS Code, Codex, Windsurf, goose

### Admin & deployment
- [ ] **Single static binary** (Go or Rust), SQLite default → Postgres upgrade; optional Redis; Docker + Helm + npx/brew one-liners
- [ ] Dashboard with full REST-API parity (dashboard = thin client of the API)
- [ ] **ONE config source of truth**: declarative file with JSON Schema, decK-style dump/diff/apply sync; env-var interpolation; hot reload; validate/--dry-run
- [ ] CLI covering full lifecycle; **admin MCP server** (see whitespace)
- [ ] Provider keys encrypted at rest; Vault/AWS/GCP/Azure secret refs; degraded serve-from-cache mode on DB outage
- [ ] llms.txt + llms-full.txt + every docs page as .md; OpenAPI spec for admin API; AGENTS.md in repo
- [ ] Apache-2.0, no license keys, signed artifacts (cosign + SBOM), nightly→RC→stable releases, published reproducible benchmark harness in-repo

---

## 3. Steal List (best-in-class ideas, attributed)

### Agent experience / control plane
1. **Kong Konnect MCP meta-tool pattern** — expose the entire admin API through 3 tools (search → get_schema → execute), never hundreds of flat tools. Cloudflare's code-mode variant: 2,500 endpoints at ~1K context tokens vs ~244K flat.
2. **Docker Dynamic MCP** — gateway control plane as MCP tools (mcp-find/add/config-set/remove/exec) so agents self-provision servers mid-session; fix their gap by making it **persistent**.
3. **Obot `setup` skill** — install a skill into local coding agents that teaches them to operate the gateway (slash commands); ship an official "operate this gateway" Agent Skill (agentskills.io standard).
4. **TrueFoundry tfy-gateway-skills** — natural-language gateway config from Claude Code/Codex ("migrate this codebase to the gateway", "add a PII guardrail"); MIT, 13 stars, trivially leapfroggable.
5. **AXI CLI discipline** (dim-ax) — --json everywhere, token-lean output, semantic exit codes (distinct "already exists"), idempotent mutations, --dry-run, definitive empty states, next-step help templates. Benchmark: tuned CLI 100% success at ~$0.05/task vs MCP 82-99% at up to 12x cost.
6. **decK (Kong)** — declarative dump/diff/apply state sync; the antidote to LiteLLM's yaml-vs-DB split brain.
7. **OpenRouter provisioning keys** — control-plane-only keys minting scoped, budget-capped, expiring child keys; the primitive for orchestrators spawning sub-agents.
8. **Pydantic `logfire gateway launch <agent>`** — keyless local dev: PKCE browser OAuth → localhost-only proxy → ephemeral bearer in process memory; plus **per-session spend/request/token caps** mapping budgets to agent runs.
9. **Higress `hgctl agent` + API MCP Server** — NL ops agent + route/provider/plugin CRUD over MCP; validates the agent-operated gateway thesis.
10. **Smithery docs surfaces** — .md suffix on every docs URL, llms.txt, docs served AS an MCP server, semantic registry search with deterministic pagination; scoped short-TTL service tokens designed to hand to agents.
11. **Microsoft registry-as-MCP-server** (GA) — one MCP connection bootstraps the agent's entire governed tool inventory; plus governance metadata in `_meta` of tool responses.
12. **APISIX apisix-mcp plugin-schema discovery** — agents introspect a plugin's JSON schema before configuring it.

### Data plane / API design
13. **OpenRouter request-body routing** — models[] fallback array, provider{order/sort/only/max_price/percentile}, :nitro/:floor/:free slug suffixes; all reliability self-served per call.
14. **OpenRouter machine-readable /models** — USD pricing, supported_parameters, context length, per-provider live endpoints; Vercel adds live uptime/p95 TTFT/throughput per provider — together the agent model-selection gold standard.
15. **OpenRouter cost contract** — usage.cost always-on incl. streams, cache_discount, async /generation/:id; cache hits billed $0.
16. **Helicone model-string DSL** — "gpt-4o-mini/azure/deployId", comma fallback chains, "!provider" exclusions; zero-SDK routing. Plus header-controlled everything (cache, limits, sessions, properties).
17. **Helicone byte-faithful passthrough-first** architecture — beta features never break; translation opt-in.
18. **Bifrost Code Mode** — MCP tools as typed stubs + sandboxed Starlark orchestration script (claims 50-92% token savings); and **Bifrost MCP hook taxonomy** (PreMCP/PostMCP + stream-chunk hooks) for the plugin system.
19. **Bifrost native-header virtual keys** — VKs accept OpenAI/Anthropic/Gemini auth header formats so agent SDKs work unmodified; **4-level budget cascade** (customer→team→key→provider) debited atomically.
20. **Envoy AI Gateway** — unified `reasoning_effort` across providers; provider-agnostic `cache_control` translation; CEL token-cost expressions (cached tokens at 0.1x); authz-filtered tools/list.
21. **TensorZero episodes + feedback** — first-class multi-step workflow ID with inference- or episode-level feedback → credit assignment for agents; adaptive A/B as a gateway primitive; `tensorzero_extra_content` escape-hatch fields instead of silent drops; read_only/max_age_s cache for deterministic eval replay.
22. **ContextForge virtual servers** — curated tool/prompt/resource compositions as the core MCP primitive; end-user identity propagation through tool calls; TOON payload compression.
23. **ToolHive find_tool/call_tool + Cedar** — semantic on-demand tool discovery (60-85% token cut) + default-deny Cedar policy; permission profiles (network/FS allowlists per server); **stacklok-claude-hooks** blocking non-governed MCP servers in Claude Code.
24. **AWS AgentCore semantic tool search** — runtime tool index query as a plain MCP tool; the anti-context-bloat benchmark.
25. **Arcade URL Elicitation + auth interrupt** — tool call pauses, returns auth URL, resumes after grant (now an accepted MCP spec enhancement); implement it.
26. **MetaMCP namespace tool overrides** — inline rename/retitle/redescribe + custom annotations (readOnlyHint) merged with upstream metadata.
27. **Arch-Router policy routing** — human-readable domain/action routing policies executed by a small local model (~50ms, 93% accuracy); explainable, agent-editable; pluggable seam rather than core bet.
28. **llm-d/GIE cache-affinity routing** — port prefix-cache-aware stickiness to provider-side prompt caches (87.4% hit / 99.92% pinning numbers prove the value); LiteLLM's PromptCachingDeploymentCheck is the SaaS-side precedent.
29. **Kong dollar-cost-as-rate-limit** + 6 AND-able limit dimensions; **Kong PII restore mode** (redact → LLM → re-insert originals in response).
30. **LiteLLM** — `/guardrails/apply_guardrail` (guardrail-as-API on arbitrary text → perfect MCP tool); tag/User-Agent-conditional guardrail modes (per-agent-identity policy); pass-through endpoints with cost tracking; key aliases; upperbound key-generate params; x-litellm-overhead-duration-ms.
31. **Portkey** — versioned config objects (routing programs referenced by ID); guardrail verdict→routing (fallback on failed check); hook_results in-band telemetry; mandatory metadata JSON schemas; Gateway 2.0 "open-source everything" trust reset.
32. **Cloudflare** — soft spend limit → auto-fallback to cheaper model (vs hard block); metadata-only logging header; logs→datasets→evaluations loop; zero-setup default gateway on first request.
33. **Invariant Guardrails flow DSL** — source-to-sink rules across the agent trace ("block send_email after reading injected inbox"); header-carried per-request policy.
34. **mcp-scan onboarding trick** — temporarily rewrite all discovered client configs to inject the gateway, restore on exit; zero-setup system-wide interception.
35. **Grab internal-gateway patterns** — Slack-bot self-serve exploration keys (short TTL, staging-only) + mini-RFC promotion to production; dual-mode transparent-proxy OR unified-schema.
36. **Block goose requirements doc** — vetted MCP registry, read-only vs destructive tool annotations with confirmation gates, per-server model allowlists, recipes (shareable parameterized tasks).
37. **Revenium MCP billing server** — profile-tiered tool sets (observe-only vs full ops) and agenticJobId outcome tracking; the metering-export contract (idempotent transaction_id event → Stripe/OpenMeter/Lago/OTLP adapters).
38. **Stripe token billing** — auto-synced provider token prices + markup %; register as a Stripe AI "integration partner" that auto-syncs usage.
39. **Descope/Scalekit identity patterns** — dual inbound-AS/outbound-broker model; opaque-reference token vault (agent never touches raw creds); agent registry keyed to human owner; ID-JAG/XAA termination (first OSS mover slot open).
40. **OSS ops**: LiteLLM 12-hour load-test gate before stable; TensorZero public benchmark page + in-repo harness + per-competitor comparison docs; Helicone npx distribution of a Rust binary; goreleaser one-tag fan-out (brew/deb/rpm/Docker/installer); Mintlify docs-as-MCP.
41. **Traffic mirroring** (LiteLLM) for silent A/B of deployments; **PromptLayer dynamic release labels** (%-split + segment routing on a label) — a gateway natively owns routing, so managed prompt A/B is cheap to add.
42. **F5 processor contract** — Reject/Modify/Annotate verdicts with parallel execution and per-processor OTel metrics; **Cloudflare detection-fields vs policy-engine separation**.

---

## 4. Whitespace (what NOBODY does well = our differentiators)

1. **LLM + MCP gateway truly unified in one OSS single binary.** agentgateway is closest (Rust, both planes) but K8s-skewed, thin MCP observability, no persistence/spend ledger, enterprise-gated MCP security. Glama has the shape but is closed SaaS. Everyone else does one half.
2. **Agent-operable control plane.** Zero OSS self-hosted gateways ship an official admin-MCP server (Kong's is SaaS, Higress's is buried in K8s complexity, Docker's is session-only). No CLI meets AXI criteria. The agent that can install, configure, govern, and debug the gateway end-to-end via MCP+CLI is the category-defining feature.
3. **One config source of truth with schema'd validate/diff/apply across UI=API=CLI=MCP=Git.** LiteLLM's split brain is the canonical anti-pattern; only Kong (decK) approximates the fix, and not for AI config.
4. **Agent-queryable own-telemetry over MCP** ("why was p95 slow yesterday?", "what did this session cost?") with query-safety caps. Grafana/Langfuse MCP servers prove demand; no gateway exposes its own store.
5. **Budget/cost introspection as first-class agent calls** — "what will this cost", "what's left in my budget", "cheapest model that fits" — plus pricing MCP tool calls themselves (zero prior art) and MCP tool-call cost accounting (nobody meters tool calls in dollars).
6. **Self-provisioning attenuated sub-keys via MCP** — agent mints child keys with budget ≤ parent remaining, models/tools ⊆ parent, auto-expiring. Combines OpenRouter provisioning keys + Bifrost cascades; nobody ships it.
7. **Provider prompt-cache-aware sticky routing** in a SaaS-provider gateway — naive LB destroys Anthropic/OpenAI cache discounts; only 2 products mitigate, none alert. Proven 87% hit rates in the self-host world.
8. **LLM-aware request hedging** (fire backup at ~p95 TTFT, take first, token-bucket cap; −74% p99 for ~9% overhead in non-LLM impls) and **mid-stream failover/resume** — unsolved across all gateways.
9. **Guardrail/policy CRUD + test/simulate/dry-run as MCP tools and CLI verbs**, with a normalized cross-provider verdict schema. Greenfield.
10. **Honest fully-loaded benchmarks**: auth+limits+logging+guardrails ON, streaming TTFT overhead, MCP-path overhead, multi-hour soaks, open-loop load gen. Nobody publishes these; the whole market benchmarks pass-through mode against mocks.
11. **Conformance-tested translation**: golden fixtures against real agent clients (Claude Code, Codex CLI, Gemini CLI, AI SDK) + MCP conformance suite in CI + per-pair fidelity matrix. The incumbents whack-a-mole the same streaming regressions forever.
12. **Per-model capability API** (modalities, logprobs, structured-output mode, supported params, live pricing) — OpenRouter has the best /models; nobody self-hosted has any.
13. **OSS per-user OAuth brokering + credential vault for MCP** (Obot's shim is closest; the polished versions are all commercial). Plus first OSS implementation of MCP Enterprise-Managed Authorization (ID-JAG/XAA) — only Keycloak is "in progress".
14. **Unified policy + audit across LLM calls, MCP tool calls, AND sandbox egress** (be the forwardURL/BYO-proxy target for E2B/Vercel/Modal/srt) — every vendor splits these into separate products.
15. **Stateless-first MCP (2026-07-28 RC) with a compatibility shim** for 2025 session-ful clients — greenfield advantage while incumbents carry stateful debt (agentgateway's in-process sessions called "a mistake").
16. **Self-hostable registry implementing the official MCP Registry OpenAPI** with continuous sync, dedup, scanning — the official registry refuses self-hosting; PulseMCP is read-only/no-SLA.
17. **Idempotency guards for retried side-effectful tool calls** — agents double-execute today; nobody addresses it.
18. **Per-agent-identity policy** (User-Agent/header-keyed guardrail modes, per-coding-agent metering, per-branch/repo cost attribution à la Requesty) generalized into a first-class identity dimension.
19. **Embedded analytics without infra tax** — single binary with embedded columnar store (e.g. DuckDB) for logs/spend, escaping the Postgres-bloat (LiteLLM) vs bring-your-ClickHouse (TensorZero/Langfuse) dichotomy.
20. **Billing-export as config-as-code** (adapter, markup %, rating dims) drivable via CLI/API/MCP — billing layers are minutes-stale and can't enforce at request time; the gateway can.

---

## 5. Architecture Signals

### Language & runtime
- **Winners are compiled:** Rust (TensorZero, Helicone GW, agentgateway, LangDB, Dynamo core) and Go (Bifrost, MCPJungle, ToolHive, Docker MCP GW, Obot, Envoy AI GW control plane). 
- **Python is disqualified for the data plane:** LiteLLM collapses at 400-500 RPS with P99 90s+, 350-500MB RSS, 3s import cold start, 1.7-4x throughput loss (issue #21046); ContextForge needs 8 workers for ~800 RPS/pod. 
- **Node/TS is a ceiling, not a floor:** Kong's bench has Kong 228% over Portkey, 859% over LiteLLM; TrueFoundry caps ~350 RPS/vCPU.
- Go preserves the single-binary + easy-plugin story (embed **wazero** for pure-Go WASM plugin sandbox — no CGO; avoid Bifrost's Go-plugin .so toolchain-lock disaster); Rust wins raw numbers. Either clears the bar.

### Performance bar to beat (published numbers)
| Product | Claim |
|---|---|
| Bifrost (Go) | 11µs mean overhead @5k RPS (t3.xlarge); 0.99ms vs LiteLLM ~40ms; ~20MB binary, ~50-120MB RSS |
| TensorZero (Rust) | <1ms p99 @10k+ QPS on 4 vCPU (observability off); LiteLLM fails at 1k QPS same box |
| agentgateway (Rust) | <0.2ms p99 @30k QPS; ~500k QPS @512 conns (Gateway API Bench v2) |
| Helicone (Rust) | <1ms traced overhead, 3k RPS sustained, <100MB RAM, 15-30MB binary, ~100ms cold start |
| Kong | ~26k RPS, p95 8ms (12 CPU, mocked backend) |
| APISIX | ~18k QPS/core, <0.2ms |
| Hosted edges (anti-bar) | OpenRouter +25-150ms; Portkey 20-40ms real; Vercel ~200ms small-prompt |

**Target:** sub-100µs–1ms p99 self-overhead at 5-10k RPS on 4 vCPU, <100MB RSS, <50MB binary, ~100ms cold start, 100% success in 60s+ sustained runs — then differentiate by publishing what nobody does: fully-loaded (policies ON), streaming TTFT, MCP-path overhead, and multi-hour soak. Per-response overhead header makes the benchmark an always-on product feature.

### Storage & state
- **SQLite-default → Postgres-upgrade** for control-plane state (Bifrost, ContextForge pattern); embedded columnar (DuckDB-class) for logs/analytics to avoid LiteLLM's Postgres-bloat death and TensorZero/Langfuse's ClickHouse infra tax.
- **Redis optional, never required**: Bifrost's gossip/memberlist sync of budgets/limits across replicas is the Redis-free multi-instance model to copy; serve-from-cache degraded mode when DB is down (Portkey/Kong DP pattern).
- Async, batched telemetry writes off the hot path — synchronous logging is LiteLLM's documented production killer.
- Config: declarative file (JSON Schema published) + DB-backed runtime mutations reconciled through one diff/apply engine; hot reload (APISIX ms-level etcd-watch is the gold standard; file-watch suffices for single binary).

### Deployment & distribution
- Single static binary; npx + brew + curl installer + Docker + Helm; zero-config first run (Cloudflare's auto-provisioned default gateway, Envoy's `aigw run` from env vars).
- Stateless horizontal scaling; stateless-first MCP (2026 RC) so no sticky sessions; K8s optional (offer GIE EPP + Gateway API conformance later, never require CRDs day one).
- License: **Apache-2.0 whole repo, DCO, no license keys, public "OSS features never shrink" covenant**; keep the entire CLI/MCP control plane and basic SSO in OSS; gate only org-scale SCIM/multi-team RBAC/compliance/managed cloud.
- Supply chain: cosign-signed images, SBOM+provenance, isolated publish creds (LiteLLM's PyPI backdoor + CVSS-10 RCE chain made gateway supply-chain a buying criterion).

---

## 6. Risks & Cautions

### Bloat / quality death spirals (don't become LiteLLM)
- LiteLLM is the cautionary tale at every layer: 1,350+ releases of breaking churn, 1,000+ open issues, "worst code I've ever read" HN thread, recurring streaming-translation regressions in the same subsystem, budget-bypass bugs (fail-open enforcement), unauthenticated /metrics leaking tenant PII, config split brain, perf collapse — yet it still leads on feature surface. Lesson: **breadth without invariants + spec/conformance tests + perf budget = reputation debt that compounds.** Adopt "first principles over whack-a-mole": pure translation core with golden fixtures per agent client, CI conformance, fuzzed SSE.
- Feature areas that died from over-owning: Helicone deprecated its own Experiments product and first prompts package; MLflow's gateway rotted for 2.5 years under owner conflict. Own the thin runtime layer (registry/labels/%-split/replay); integrate for eval UIs.
- Kong/APISIX show config sprawl risk: 23 plugins with separate schemas / 300+ env vars (ContextForge) are hostile to humans and agents alike. One coherent config beats plugin confetti.
- K8s-first = adoption ceiling (Envoy AI GW, kgateway, Higress all hit it). Single binary first, CRDs later.

### Licensing & business traps
- **Relicensing is fatal** (Terraform→OpenTofu, Redis→Valkey); **clawing back shipped OSS features is equally toxic** (MinIO admin-UI removal, LiteLLM moving Prometheus metrics behind the paywall = most-resented move in the category). Publish the covenant up front.
- AGPL/GPL kills adoption here: Helicone's Rust gateway died at GPL relicense; new-api's AGPL blocks embedding; LLM Gateway's AGPL+ee/ split draws complaints. Apache-2.0 or lose.
- The "SSO tax" and enterprise nagware in the OSS UI (LiteLLM) generate durable resentment — free basic SSO is a cheap trust win.
- Zero-markup pricing is being raced to zero by Vercel/Cloudflare loss-leaders; OpenRouter's ~5% works only at consumer scale. Monetize org-scale governance + managed cloud, never tokens.
- Acquisition churn is the market's trust wound and our window: Portkey→Palo Alto, Helicone→Mintlify (maintenance mode), Metronome→Stripe, OpenMeter→Kong, Natoma→Snowflake, Katanemo→DigitalOcean, Pydantic AI Gateway archived with zero data portability. "Independent, foundation-friendly, Apache-2.0, no rug-pulls" is a positioning asset — and migration tooling from Portkey/Helicone/Pydantic is cheap distribution.
- Venture-dead patterns: standalone routing (Unify dead, LangDB pivoted, Martian repositioned); registries rug-pulling hosting (Smithery Mar 2026). Routing intelligence is a feature, not a company; consume OSS routers (RouteLLM/Arch-Router/CARROT) behind a seam.

### Security (the gateway is the vault door)
- Gateways hold all provider keys: LiteLLM's PyPI backdoor + CVE-2026-42271 (admin-UI MCP test-connection → RCE, CISA KEV) + 36h-exploited SQLi; Smithery's path traversal nearly leaked 3,000 hosted servers' keys; Composio's breach exposed ~5,241 API keys (best self-host sales argument — use it, and don't repeat it).
- Admin UI/API must be authed by default (ContextForge shipped permissive defaults until RC; APISIX default admin-key was exploited in the wild); panics must not crash the API server (ToolHive #3107).
- Guardrail holes to not inherit: streaming outputs unguarded everywhere; per-endpoint guardrail coverage matrices (Lakera-on-chat-only) are a trap; MCP/tool traffic mostly unguarded while agents act through tools.
- Cost-accounting correctness is a security-grade trust issue: cached-token 10x overcharges, streaming usage inflation, aborted-stream usage loss — publish invoice-reconciliation accuracy.

### Standards churn
- MCP 2026-07-28 RC removes the handshake (breaking); OTel GenAI semconv still unstable; A2A adoption contested — implement stateless-first with a compat shim, dual-emit telemetry, keep A2A incremental.
- DCR is broken in the wild (Entra/Cognito refuse it); support CIMD-first with DCR fallback and pre-registration.

---

*Companion files: per-competitor and per-dimension reports listed in each entry's `file` field; `gap-check.md` for category-coverage audit; `naming-{1,2,3}.md` for name candidates (Yumen / Bascule / Trunkline / Diolkos lead).*
