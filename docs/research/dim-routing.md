# Dimension Deep-Dive: Routing & Reliability in AI Gateways

Competitive-intelligence report for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

Researched 2026-06-10. Sources: official docs of LiteLLM, Portkey, OpenRouter, Bifrost (Maxim), Kong AI Gateway, Envoy AI Gateway, Cloudflare AI Gateway, Helicone AI Gateway, TensorZero, TrueFoundry, Vercel AI Gateway, kgateway/Gateway API Inference Extension, Arch (katanemo), RouteLLM; plus third-party comparisons and complaint threads.

---

## 1. The state of the art, gateway by gateway

### 1.1 LiteLLM (Python, MIT core / enterprise tier)

The most feature-complete *config surface* for routing, and the de-facto reference everyone benchmarks against.

**Routing strategies** (`router_settings.routing_strategy`):
- `simple-shuffle` (default, recommended for prod) — random pick, becomes a **weighted pick** if `weight`, `rpm`, or `tpm` are set on deployments.
- `usage-based-routing-v2` (async, Redis-backed) — filters out deployments over their TPM/RPM limits, then routes to lowest-TPM-usage deployment. Rate-limit-aware routing.
- `latency-based-routing` — moving average of response time per deployment; args: `ttl` (averaging window seconds), `lowest_latency_buffer` (e.g. `0.5` = treat anything within 50% of the fastest as equal, prevents hot-spotting the fastest deployment).
- `least-busy` — fewest in-flight concurrent requests.
- `cost-based-routing` — picks lowest input/output cost from the model cost map; `input_cost_per_token`/`output_cost_per_token` overridable per deployment.
- Custom strategy via `CustomRoutingStrategyBase` plugin class.
- **Routing groups** (newer): different strategies per model group in one router (`routing_groups: [{group_name, models, routing_strategy, routing_strategy_args}]`).
- **Tag-based routing** (enterprise-flavored): route by request tags / keys / teams; per-key and per-team routing-strategy overrides.

**Retries:** `num_retries`, `retry_after` (minimum wait). Exponential backoff automatically for `RateLimitError`, immediate retry for generic errors. **Per-error-class retry policy** (`RetryPolicy`): e.g. `RateLimitErrorRetries=3`, `AuthenticationErrorRetries=0`, `TimeoutErrorRetries=2`, `ContentPolicyViolationErrorRetries=3`.

**Fallbacks — four distinct kinds** (this taxonomy is itself a differentiator most copy):
1. General `fallbacks: [{"gpt-4": ["claude-3"]}]` — triggers after `num_retries` exhausted.
2. `context_window_fallbacks` — triggers on `ContextWindowExceededError` (route long prompts to a long-context model).
3. `content_policy_fallbacks` — triggers on content-policy refusals (route to a more permissive model).
4. `default_fallbacks` — global catch-all list.
Plus: client-side per-request fallbacks in the request body (`"fallbacks": [...]`, can carry **different messages/params per fallback model**), `disable_fallbacks` per request or per key, fallback to a *specific deployment id* (`model_info.id`, skips cooldown checks), wildcard fallbacks (`azure/*`), `enable_weighted_failover` (retry within the model group respecting weights before cross-group fallback, capped by `max_fallbacks`, default 5), and `mock_testing_fallbacks: true` flags for chaos-testing each fallback type.

**Cooldowns / circuit breaker:** per-deployment (keyed by deterministic `model_id` hash): `allowed_fails` (default 3 per minute), `cooldown_time` (default 5s), `disable_cooldowns`, per-error-class `AllowedFailsPolicy` (e.g. tolerate 100 rate-limit errors but cool down fast on auth errors), per-deployment `cooldown_time: 0` opt-out. Auto-recovery after cooldown expiry.

**Pre-call checks** (`enable_pre_call_checks: true`): context-window validation *before* sending (uses `base_model` for Azure), region filtering (`region_name: "eu"` — auto-inferred for Vertex/Bedrock/WatsonX) for data-residency routing, per-deployment `max_parallel_requests` (auto-derived from RPM, or TPM/6).

**Other:** `request_timeout`, per-deployment timeouts, health-check endpoint that actually calls providers, alerting hooks (Slack/webhook on exceptions + slow responses).

**Weaknesses:** Python performance ceiling (Bifrost markets "50x faster than LiteLLM"; ~hundreds of µs–ms overhead claims vs LiteLLM's tens of ms at load); config sprawl — the same knob exists in `litellm_settings`, `router_settings`, per-deployment, per-key, per-request, which users find confusing; repeated critical CVEs in 2026 (CVE-2026-42208 SQLi CVSS 9.3 exploited within 36h; CVE-2026-42271 command-injection chain to RCE) — a serious trust problem for a security-sensitive chokepoint; routing strategies other than simple-shuffle need Redis to work correctly multi-instance.

### 1.2 Portkey (TS, gateway now fully Apache-2.0 as of March 2026; managed platform on top)

**The composable config-object model** — the cleanest routing DSL in the space:
- `strategy.mode`: `single` | `loadbalance` | `fallback` | `conditional`.
- `targets[]`: each target = provider/virtual-key + optional `weight`, optional `on_status_codes` override, **or a nested strategy object** — strategies nest recursively. "Load-balance across two providers, each of which is its own fallback chain, inside a conditional router" is one JSON document.
- `retry`: `{attempts, on_status_codes: [429,500], use_retry_after_headers: true}` (respects provider `Retry-After`).
- `request_timeout` (ms), per-target `override_params` (force model/temperature per branch), `cache: {mode: simple|semantic, max_age}` at any level of the tree.
- **Conditional routing:** `strategy.conditions[]` with a Mongo-like `query` over request metadata/params (user plan, geography, model param, custom metadata) + `then: target_name`, plus `default`. Targets fully composable.
- **Canary:** done with `loadbalance` weights (e.g. 95/5); their docs explicitly position 1–5% canary as a first-class pattern.
- Fallback triggers configurable by status code list (`on_status_codes`), so you can fallback on 429 only, or on any 4xx/5xx set.
- Circuit breaker exists on the managed platform; multi-region is a managed-platform feature (deploy gateway in multiple regions, latency-based DNS).

**Weaknesses:** the OSS gateway historically lagged the hosted product (guardrails/observability gated); conditional `query` operators are limited; no adaptive/live-error-rate routing in OSS — weights are static; JS runtime overhead vs Go/Rust competitors.

### 1.3 OpenRouter (closed SaaS marketplace — the routing-behavior benchmark)

- **Default algorithm (publicly documented, worth stealing):** filter out providers with significant outages in the last 30s (live health), then weighted-random among the rest with weight = **inverse square of price** ($1/M provider gets 9x the traffic of $3/M). Remaining providers ordered as fallbacks.
- `provider` object per request: `order` (explicit provider sequence), `allow_fallbacks` (default true), `sort` (`price` | `throughput` | `latency`, with object form `{by, partition: model|none}` to sort across models globally), `ignore`, `only`, `require_parameters` (only providers supporting all request params), `quantizations` filter, `max_price`, `data_collection: deny`, `zdr` (zero-data-retention-only), `preferred_min_throughput` / `preferred_max_latency` **with percentile selectors (p50/p75/p90/p99)**.
- **Model-level fallbacks:** `models: [a, b, c]` priority array — auto-advance on error, rate limit, or moderation refusal.
- Slug shortcuts: `:nitro` (= sort by throughput), `:floor` (= sort by price) appended to any model slug — ergonomic, agent-friendly.
- Continuous health monitoring of every provider endpoint (latency, error rate, availability) feeding routing; public per-provider uptime/latency charts per model.

**Weaknesses:** itself a SPOF — three documented outages in 8 months (50 min DB outage Aug 2025; 38 and 35 min in Feb 2026, one returning misleading 401s); no SLA; 5.5% credit fee; +25–150ms overhead vs direct; routing decisions not fully auditable/controllable; closed source.

### 1.4 Bifrost / Maxim (Go, OSS core + enterprise) — the adaptive-routing benchmark

- Claims: <100 µs overhead at 5k RPS, "50x faster than LiteLLM"; 1000+ models.
- **Adaptive load balancing (enterprise):** scores every route every 5s: `Score = error_penalty*0.5 + latency_score*0.2 + utilization*0.05 − momentum`; weight = `Wmin + (1−score)*(Wmax−Wmin)` mapped to 1–1000. Latency scoring is **token-aware** ("MV-TACOS" algorithm — normalizes latency by tokens generated so long generations aren't punished). State machine: healthy (<2% error + receiving ≥50% expected traffic), degraded (≥2% error), failed (>5% error or throughput breach). **Momentum bias** gives recovering routes a 90% penalty reduction within 30s (fast re-warm). Selection = weighted random with 5% jitter band + **25% exploration probability** (keeps probing degraded routes).
- **Cluster mode:** peer-to-peer, every node equal, weights synchronized via **gossip protocol** — no Redis dependency for distributed routing state.
- Automatic fallbacks between keys, providers, and models; per-key weighted distribution in OSS.
- Also an MCP gateway (closest architectural analog to the product being built).

**Weaknesses:** the genuinely interesting routing (adaptive LB, cluster mode) is enterprise-gated; algorithm parameters not user-tunable/under-documented (MV-TACOS unexplained); younger ecosystem.

### 1.5 Kong AI Gateway (`ai-proxy-advanced` plugin; Lua/OpenResty; OSS + enterprise)

The most traditional-LB-literate option:
- **Seven balancer algorithms:** weighted round-robin; consistent-hashing (`hash_on_header` — sticky-by-header sessions); least-connections (in-flight tracking, v3.13+); lowest-usage (token-based, `tokens_count_strategy` = prompt/completion/total tokens); lowest-latency (`latency_strategy: tpot | e2e` — **time-per-output-token** option is notable); **semantic** (embeds the prompt, routes to the model whose configured description is most similar — v3.8+); **priority** (priority groups with weights inside each group, spillover to next group on failure, v3.10+).
- Retries: `retries` count, `failover_criteria` (default `error, timeout`; extensible to status classes — e.g. `http_429`), `connect_timeout` / `read_timeout` / `write_timeout`.
- Health/circuit breaking: `balancer.max_fails` + `balancer.fail_timeout` passive circuit breaker (v3.13+); inherits Kong's active health-check machinery for upstreams.

**Weaknesses:** AI features split across paid plugins (`ai-proxy` OSS vs `ai-proxy-advanced` enterprise); heavyweight to operate for AI-only use; config via Kong entities is verbose; no quality/cost-aware routing beyond token counting.

### 1.6 Envoy AI Gateway (Go control plane + Envoy; CNCF; k8s-native)

- **Provider fallback:** prioritized `backendRefs` in `AIGatewayRoute` — first is primary, rest are fallbacks; auto-shift as higher-priority endpoints become unhealthy (priority-based failover, built on Envoy aggregate clusters).
- Retries via `BackendTrafficPolicy`: exponential backoff **with jitter**, configurable triggers (5xx, connect-failure, retriable status codes incl. 429), counts, per-try timeouts.
- Inherits all of Envoy: outlier detection (ejection on consecutive 5xx — true circuit breaker), active health checks, locality/zone-aware LB, **request hedging exists in Envoy core** (hedge policy on per-try timeout), priority sets, multi-cluster/multi-region failover.
- Integrates with **Gateway API Inference Extension** for self-hosted model pools (see 1.10).

**Weaknesses:** k8s/CRD-only ergonomics — hostile to anyone not on Kubernetes; LLM-specific routing semantics (token-aware, cost-aware) still thin; no dashboard of its own.

### 1.7 Cloudflare AI Gateway (closed, edge SaaS)

- **Dynamic routing (Aug 2025 refresh):** visual flow-chart editor + JSON config; nodes for conditions (request attributes/metadata), **percentage split** (A/B, e.g. 50/50 — canary as a graph node), rate-limit node (quota → fallback branch), **budget-limit node (cost quota → fallback branch — unique)**, model nodes with per-model timeouts and retries.
- Classic array fallbacks on the universal endpoint; retries with configurable count/backoff; runs at 300+ PoPs → multi-region by default with no user effort.

**Weaknesses:** closed; routing logic capped at what the node editor supports; no self-host; observability tied to CF dash.

### 1.8 Helicone AI Gateway (Rust, OSS) — the latency-algorithm benchmark

- **Latency-based P2C + PeakEWMA** default: pick two random providers, route to the better by peak-decayed EWMA latency — prevents herd behavior, predicts slowdowns before they bite. (Borrowed from Twitter Finagle/linkerd lineage; built on Tower middleware.)
- Also: weighted distribution, cost-optimized strategy; **all strategies are health- and rate-limit-aware** (providers near rate limits removed from candidate set).
- Health-aware circuit breaking: auto-remove failing providers, periodic recovery probes, no manual intervention.
- Numbers: ~1–5 ms p95 overhead, ~10k RPS on one instance; two-tier state (in-memory + Redis) for multi-instance.

**Weaknesses:** Helicone pivoted focus back toward observability; gateway feature surface (conditional routing, canary, guardrails) thinner than Portkey/LiteLLM; fewer providers.

### 1.9 TensorZero (Rust, Apache-2.0) — the experimentation-native benchmark

- **Three-level reliability hierarchy:** (1) model-level `routing = ["openai", "azure"]` sequential provider failover; (2) variant-level `retries = {num_retries, max_delay_s}` — truncated exponential backoff **with jitter**; (3) function-level **variant fallbacks via experimentation config**: `candidate_variants` (weighted sampling, e.g. `{"gpt_5_mini" = 0.7, "claude_haiku_4_5" = 0.3}`) + `fallback_variants`, executed by **sampling without replacement** — A/B testing and failover are the *same* mechanism.
- **Best-in-class timeout granularity:** `timeouts = {non_streaming.total_ms, streaming.ttft_ms, streaming.total_ms}` at provider, model, or variant scope — explicit **TTFT timeout for streams** (most gateways can't time out a stream that connected but never emits tokens). Global `global_outbound_http_timeout_ms` ceiling.
- Adaptive experimentation (bandit-style variant sampling) on the roadmap/partially shipped; feedback/metrics loop can drive routing toward better-performing variants.

**Weaknesses:** no explicit load-balancing strategy (acknowledged in docs — emulate via equal-weight variants); TOML config + recompile-ish workflow is heavier; gateway is part of a bigger LLMOps opinion many don't want.

### 1.10 Gateway API Inference Extension / llm-d / kgateway (k8s, self-hosted model pools)

The frontier for *self-hosted* routing — different problem (pods, not providers) but algorithms worth stealing:
- **Endpoint Picker (EPP)** scores pods via pluggable scorers: `QueueScorer` (inverse queue depth), `KVCacheUtilizationScorer` (inverse KV-cache usage), **`PrefixCachePlugin`** (tokenize+hash prompt prefix, route to the pod already holding those KV blocks), LoRA-affinity scorer.
- **Session affinity / sticky sessions:** route a session's turns to the same pod for KV reuse — measured 99.92% requests pinned to warm pod, 87.4% cache hit rate; without affinity prefix-cache hit rate degrades as 1/N replicas.
- vLLM's SAAR (June 2026): router-owned session memory, hard locks during tool loops, "switch pricing" that accounts for lost prefix cache before re-routing a session to a different model — directly relevant to agent traffic.
- The same logic applies at the SaaS-provider layer: **Anthropic/OpenAI prompt-cache hit rates depend on hitting the same provider/key — a gateway that load-balances naively destroys prompt-cache discounts.** Almost no commercial gateway handles this today (cache-aware provider stickiness is a near-open niche).

### 1.11 Quality-aware routing layer (RouteLLM, Not Diamond, Martian, Arch-Router)

- **RouteLLM (lm-sys, OSS):** trained router (matrix-factorization / BERT classifier on preference data) picks strong vs weak model per prompt; 95% of GPT-4 quality at 26% of GPT-4 calls; up to 85% cost cut on MT-Bench. Drop-in OpenAI-compatible server.
- **Not Diamond / Martian:** commercial per-prompt model-selection APIs (predict best model per input); Martian markets "model mapping". Both are routers-as-a-service that a gateway can call as an oracle.
- **Arch / Arch-Router-1.5B (katanemo, OSS):** **preference-aligned routing** — users write domain/action policies in plain config ("code generation → model X, legal summarization → model Y"); a 1.5B local model maps each prompt to a policy with 93% accuracy at ~50 ms median, no retraining when models change. Shipped inside the archgw proxy (Envoy-based); even has a Claude Code router demo.
- Cloud-provider equivalents: AWS Bedrock Intelligent Prompt Routing, Azure Model Router, Vertex AI router — table stakes are forming at the hyperscaler level.

### 1.12 Others, briefly

- **TrueFoundry (K8s, enterprise):** weight-based + latency-based routing in declarative YAML; latency algorithm documented: avg per-token latency over last 20 min or last 100 calls (whichever fewer), <3 datapoints = assume fast (cold-start exploration), **1.2x-of-fastest eligibility band** to prevent flapping; per-target `retry_config` (attempts, delay, retryable codes 429/500/502/503) + separate `fallback_status_codes`; canary by weights; rate limits/budgets per team/user/model that can *downgrade to cheaper models* on breach.
- **Vercel AI Gateway (closed SaaS):** `providerOptions.gateway`: `order`, `only`, `sort` (`cost` | `ttft` | `tps`); BYOK with **automatic fallback to system credentials when your key fails** (charged to credits) — interesting reliability/billing hybrid; known bugs: BYOK overrides explicit `order` (GH #11644), gateway blocks all requests when credits depleted even with BYOK (#11280).
- **Higress (Alibaba, OSS, Envoy-based):** model-level fallback, token-based rate limiting, canary by header/weight; strong in CN ecosystem.

---

## 2. Algorithm cheat-sheet (concrete, stealable)

| Mechanism | Best-documented implementation | Algorithm |
|---|---|---|
| Default LB | OpenRouter | 30s-outage filter → weighted random by 1/price² |
| Latency LB | Helicone | P2C + PeakEWMA (decaying peak latency, pick-2) |
| Latency LB (flap-proof) | TrueFoundry | per-token latency, 20min/100-call window, 1.2x band, <3 samples = explore |
| Token-aware latency | Kong (`tpot`), Bifrost (MV-TACOS) | normalize latency by output tokens |
| Adaptive on live errors | Bifrost | score=0.5·err+0.2·lat+0.05·util−momentum, 5s recompute, 25% exploration, gossip sync |
| Circuit breaker | LiteLLM | per-deployment allowed_fails/min → cooldown_time, per-error-class thresholds |
| Circuit breaker (classic) | Envoy | outlier detection: consecutive-5xx ejection + active health checks |
| Retries | TensorZero / Envoy | truncated exponential backoff + jitter; per-try timeouts |
| Retry-After respect | Portkey | `use_retry_after_headers: true` |
| Fallback taxonomy | LiteLLM | general / context-window / content-policy / default, + per-request override |
| Streaming timeouts | TensorZero | separate `ttft_ms` and stream `total_ms` |
| Hedging | Envoy core (per-try timeout hedge); no LLM gateway ships LLM-aware hedging | fire backup at ~p95, take min(primary, hedge); for LLMs must hedge on **TTFT**, not headers; cap with token bucket (~9% overhead for −74% p99 in published non-LLM impls) |
| Canary | Portkey/Cloudflare/TrueFoundry | weight split 1–5% + metrics watch; CF makes it a visual node |
| Sticky sessions | Kong (hash_on_header), GIE/llm-d (session→pod affinity) | consistent hashing; KV/prefix-cache-aware scoring |
| Semantic routing | Kong v3.8+, Arch-Router | embed prompt vs model descriptions; or 1.5B policy-mapping model (93%, 50ms) |
| Quality/cost routing | RouteLLM | preference-data-trained classifier, strong/weak split |
| Multi-region | Cloudflare (300+ PoPs), Envoy (locality LB), Portkey managed | edge presence or locality-aware priorities |
| Distributed state | Bifrost (gossip, no Redis) vs LiteLLM/Helicone (Redis) | gossip wins operationally for single-binary story |

## 3. Gaps in the market (opportunities)

1. **Provider-side prompt-cache-aware routing.** Nobody routes to preserve Anthropic/OpenAI prompt-cache hits (sticky provider+key per prefix/session). Self-host world (llm-d) proves 87%+ hit rates from affinity; SaaS-gateway world ignores it. Direct money saved, easy to demo.
2. **LLM-aware hedging.** No gateway ships TTFT-based hedged requests with budget caps. "p99 TTFT cut 60% for +5% spend" is a killer headline metric.
3. **Adaptive routing in OSS.** Bifrost's adaptive LB is enterprise-gated; LiteLLM/Portkey OSS weights are static. Shipping Bifrost-grade live-error/latency-adaptive weights in the open core is a wedge.
4. **Stream-resume / mid-stream failover.** Streaming continuity across fallback (replaying or restarting a stream transparently after a mid-stream provider death) is the biggest unsolved reliability gap called out in 2026 comparisons.
5. **Unified taxonomy + chaos testing.** LiteLLM's 4 fallback types + `mock_testing_*` flags are the right idea, scattered across a messy config. A clean, nestable (Portkey-style) policy tree with built-in fault injection and a "test my failover" CLI command would be best-in-class.
6. **Agent-native control plane.** All config today is YAML/JSON/dashboards for humans. An MCP-controllable router ("add a fallback", "what's the health of provider X", "run a 5% canary of model Y and report in 1h") doesn't exist anywhere.
7. **Idempotency on retry** for tool-calling/agents (don't double-execute side-effectful calls) — called out in failover comparisons as broadly missing.

## 4. AX (agent experience) observations

- OpenRouter is the most agent-ergonomic today: routing expressed *in the request* (`models[]`, `provider{}`, `:nitro`/`:floor` slugs) — an agent can self-serve reliability per call without touching server config. Per-request routing should be a first-class API everywhere.
- LiteLLM exposes per-request `fallbacks` in the body and returns `x-litellm-model-id` headers — request/response metadata that tells the caller *what actually happened* (which deployment served, was it a fallback) is essential for agents; most gateways hide this.
- Cloudflare's visual flow editor is the anti-pattern for agents; Portkey's pure-JSON config tree and TensorZero's TOML are machine-writable and validate well — a JSON-schema'd routing policy + CRUD API + MCP tools is the right surface.
- Arch shows a small local model can *be* the router under agent traffic (Claude Code router demo); vLLM SAAR shows sessions need hard locks during tool loops — router must be conversation/agent-session aware, not request-stateless.

## 5. Table stakes vs differentiators (summary)

**Table stakes (everyone has):** OpenAI-compatible unified API; priority fallback chains across models/providers/keys; retries w/ exponential backoff + jitter, configurable counts and retryable status codes; weighted load balancing across keys/deployments; timeouts; per-deployment cooldown/passive circuit breaking; health tracking that removes bad targets and auto-recovers; canary via weight split; multi-instance state sharing (Redis or equivalent).

**Differentiator bar (best-in-class to match or beat):** LiteLLM's 4-type fallback taxonomy + per-error-class retry/cooldown policies + pre-call checks; Portkey's recursive composable policy tree + conditional routing + Retry-After honoring; OpenRouter's 1/price² LB, percentile latency/throughput constraints, in-request routing controls; Bifrost's adaptive scoring with momentum recovery + gossip cluster; Helicone's P2C+PeakEWMA; Kong's tpot latency + semantic LB + consistent-hash stickiness; TensorZero's TTFT stream timeouts + experimentation-as-failover; TrueFoundry's flap-resistant latency banding; GIE/llm-d's prefix-cache & session affinity; Arch-Router's policy-based 1.5B local routing model.

