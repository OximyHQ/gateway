# Dimension Deep-Dive: Performance Engineering in AI Gateways

Research date: 2026-06-10. Subject: how the fast AI gateways (LLM gateways + MCP gateways) get fast, what numbers they publish, how they measure them, and what a new single-binary open-source gateway must do to compete and to publish credible benchmarks.

---

## 1. The published numbers (the leaderboard as marketed)

All numbers below are **gateway-added overhead** (not end-to-end LLM latency) unless noted. Every vendor benchmarks against a **mock provider** to isolate gateway overhead.

| Gateway | Language | Headline claim | Conditions | Source |
|---|---|---|---|---|
| **Bifrost** (Maxim AI) | Go | **11µs** mean overhead @ 5,000 RPS (t3.xlarge); 59µs on t3.medium; README now says "<100µs @ 5k RPS" | sustained 60s+, mocked OpenAI endpoint, 100% success | getmaxim.ai/bifrost/resources/benchmarks |
| **TensorZero** | Rust | **<1ms p99** @ 10,000 QPS — measured: mean 0.37ms, p50 0.35ms, p90 0.50ms, p95 0.58ms, p99 0.94ms, 100% success | AWS c7i.xlarge (4 vCPU/8GB), Ubuntu 24.04, observability **disabled**, mock provider, load gen + gateway + mock all on same box; TensorZero 2025.5.7 vs LiteLLM 1.74.9 (2025-07-30) | tensorzero.com/docs/gateway/benchmarks |
| **Helicone AI Gateway** | Rust | p95 **<5ms** overhead (some marketing says p50 ~2ms / sub-1ms isolated overhead); ~3,000 RPS/instance; ~64MB RAM; ~30MB binary; ~100ms cold start | k6, Fly.io performance-2x 4GB machines, mock provider at 60ms median, auth on / logging off, 2-min sustained runs, zero errors | github.com/Helicone/ai-gateway + benchmarks/README.md |
| **agentgateway** (solo.io / Linux Foundation) | Rust (tokio+hyper+tonic) | sub-ms overhead @ 10k+ QPS; marketing: ~500k QPS w/ 512 connections; "300× memory, 35× throughput, 122× latency" vs peers | proxy-level benchmark vs peer proxies | solo.io blog "Designing agentgateway" |
| **Envoy AI Gateway / Envoy** | C++ (Go control plane) | ~1–3ms overhead; MCP routing 1–2ms/session tuned; avg difference vs agentgateway ~0.2ms ("negligible") | Tetrate + envoyproxy.io MCP perf posts | aigateway.envoyproxy.io, tetrate.io |
| **LiteLLM** | Python | v1.78.5: **8ms median / 45ms p99** overhead @ 1K concurrent — but that's **with 4 instances**. Roadmap target: 4ms median / 22.5ms p99 by end of 2025. aiohttp migration (v1.71.1): "200 RPS per instance at 40ms median overhead" | own docs; earlier independent tests far worse (below) | docs.litellm.ai, GitHub discussion #15933 |
| **Portkey** | TS (Cloudflare edge workers, hosted) | marketing "sub-1ms core"; real-world consistently **20–40ms** added (edge hop + features) | own docs + third-party measurements | portkey.ai docs |
| **OpenRouter** | hosted (Cloudflare Workers) | "~15ms once cache is warm" (employee on HN, Dec 2025); docs say "<50ms typical routing overhead"; one 2026 third-party benchmark found OpenRouter TTFT 70ms *faster* than OpenAI direct (provider routing wins can exceed gateway tax) | HN id=46232006; openrouter.ai docs; opper.ai router benchmark | |
| **Kong AI Gateway** | C/Lua (OpenResty/Nginx) | own benchmark: >200% throughput of Portkey OSS, >800% of LiteLLM; p95 65% lower than Portkey, 86% lower than LiteLLM; baseline mock was 29,005 RPS @ p95 24ms | EKS c5.4xlarge, WireMock backend, k6, 400 VUs, 1000-token prompts, 12 CPU per gateway, pure pass-through (no policies) | konghq.com engineering blog |
| **Cloudflare AI Gateway** | (Rust infra; FL2 core rewrite) | no isolated gateway-overhead number published; rides Cloudflare's FL2 Rust rewrite (-10ms response time, +25% perf, ½ CPU, <½ memory vs FL1) | blog.cloudflare.com "20-percent-internet-upgrade" | |
| **LLMProxy** (OSS, Go) | Go | "<1ms first-token overhead, zero-buffer io.Copy splice" | README claims, niche project | github.com/aiyuekuang/LLMProxy |

### Head-to-head data points worth keeping
- **Bifrost vs LiteLLM @ 500 RPS, t3.medium (60s, 500 VUs):** p50 804ms vs 38.65s; p99 1.68s vs 90.72s (54×); throughput 424 vs 44.8 req/s; success 100% vs 88.78%; peak RAM 120MB vs 372MB. Gateway-only overhead on a 60ms mock: 0.99ms vs 40ms.
- **TensorZero vs LiteLLM:** LiteLLM p99 39.69ms at 500 QPS and **total failure (timeouts) at 1,000 QPS**, while TensorZero ran 10,000 QPS at p99 0.94ms on the same 4-vCPU box.
- **Kong's framing:** at pure pass-through, an Nginx-derived gateway still beats both Python (LiteLLM) and Node/edge-runtime (Portkey OSS) by large multiples on identical 12-CPU allocations.

### Reality check on "does it matter"
A single chat completion takes 500ms–30s, so 11µs vs 8ms is invisible for one call. The honest arguments for microsecond/millisecond overhead are: (1) **agentic chains** — agentgateway's own pitch: "50ms × 20 tool calls = 1 full second of gateway tax"; (2) **throughput-per-dollar** — LiteLLM needs 4 instances + workarounds to hit numbers TensorZero hits on one small box; (3) **tail behavior under saturation** — the Python gateways don't degrade gracefully (28–90s p99s, 11% error rates, outright collapse), which is an availability problem, not a latency problem; (4) **TTFT on streaming** — buffering/parsing in the stream path directly delays first token.

---

## 2. How the fast ones get fast (technique inventory)

### 2.1 Language & runtime
- **Rust (TensorZero, Helicone, agentgateway, Cloudflare FL2/Infire/Pingora):** no GC pauses, predictable tails, tiny RSS (Helicone 64MB; agentgateway claims 300× memory efficiency). Standard stack is **tokio + hyper/axum/tower + tonic** — battle-tested async networking, which is the explicit reason solo.io chose it. Cloudflare's FL2 rewrite: ½ CPU, <½ memory, −10ms.
- **Go (Bifrost, LLMProxy, many in-house gateways):** native code + goroutines; low-latency GC is "good enough" — Bifrost's 11–59µs shows Go can play in the microsecond league with discipline. Faster to build/extend than Rust; plugin ecosystems easier (Go plugins, yaegi, WASM).
- **Python (LiteLLM):** GIL, asyncio overhead, dynamic typing, GC churn, 500MB+ images, ~500MB/worker RSS (≈200MB just from Prisma import per LiteLLM's own roadmap). LiteLLM's improvement path is telling: orjson, aiohttp everywhere, removing imports, and "components may be moved to Rust if required" (fast-litellm is a Rust-acceleration sidecar project). Python gateways scale **horizontally by necessity** — every published LiteLLM number is multi-instance.
- **C/C++ heritage (Kong/Nginx, Envoy):** decades-tuned event loops; the AI features are filters/plugins on a proxy that was already fast. Envoy's 1–3ms is higher than Rust-native claims partly because of filter-chain architecture and ext_proc hops.

### 2.2 HTTP server & client hot path
- **Bifrost:** `valyala/fasthttp` for both server and provider calls (avoids net/http allocation patterns), `bytedance/sonic` for JSON in hot paths, **sync.Pool object pooling** for buffers/requests, "request routing layer avoids allocation in the critical path", parsing+validation budget ~2µs.
- **Provider-isolated worker pools:** Bifrost pre-spawns per-provider worker pools fed by channels — per-provider isolation means one slow provider can't starve others; includes circuit breakers. This is the Go-idiomatic version of bulkheading.
- **Connection pooling / keep-alive to providers:** table stakes. New TLS handshakes are brutal (asymmetric crypto can eat 30–40% of edge CPU at peak); target >70% TLS session-resumption ratio, TLS 1.3, tuned keep-alive (15–60s+). Gateways hold warm pools per provider host. HTTP/2 multiplexing to providers where supported reduces connection count, but note most gateways still run HTTP/1.1 pools upstream because provider support/intermediaries vary; HTTP/2 matters more on the client-facing side (many concurrent app streams over fewer connections).
- **JSON strategy:** the biggest hidden cost in an OpenAI-compatible gateway is parse→transform→re-serialize on every request/chunk. Fast gateways avoid full-fidelity re-serialization: Bifrost does "streaming byte manipulation instead of format conversion round-trips"; LLMProxy's design rule is "main request path doesn't parse the JSON response body" — passthrough bytes, extract usage asynchronously. When you must transform (Anthropic↔OpenAI schema), do it field-targeted (sonic/simd-json/serde with borrowed slices), not via generic DOM.

### 2.3 Streaming (the TTFT war)
- **Zero-copy / zero-buffer SSE passthrough:** forward provider chunks byte-for-byte as they arrive; never accumulate the stream. LLMProxy uses `io.Copy` (kernel splice path) for <1ms first-token overhead. Disable any intermediate buffering (the classic failure is Nginx `proxy_buffering on` killing SSE).
- **Flush-per-chunk** with no re-chunking; keep SSE because it traverses all HTTP infra, needs no sticky sessions, and reconnects natively.
- **Usage extraction off-stream:** parse only the final `usage` chunk (or `[DONE]` boundary) — or tee the stream to an async task — rather than JSON-decoding every delta on the hot path.
- **Stream metrics to track:** TTFT, inter-token cadence, stalled-chunk detection, stream error rate, completion duration. Production gateways measure these per provider and feed latency-aware routing (Helicone routes on measured latency).

### 2.4 Async everything off the hot path
- **Observability/logging is the #1 hot-path killer.** Evidence: TensorZero had to *disable* observability to publish its <1ms number (it normally writes inferences to ClickHouse asynchronously in batches); LiteLLM's documented production failure mode is the gateway slowing down as its Postgres log table grows (1M rows in 10 days at 100k req/day — their fix: ship logs to blob/DynamoDB and disable DB logging); Helicone benchmarks run "logging disabled."
- Pattern for a new gateway: hot path emits a fire-and-forget event into a bounded in-memory channel/ring; a background task batches into ClickHouse/OTLP/file; **drop or spill under backpressure rather than block requests**; never do per-request synchronous DB writes, and never let the request path *read* the analytics store.
- Same applies to cost computation, spend counters, webhook/alert evaluation: compute async, enforce limits from cached/atomic counters.

### 2.5 Token counting & tokenizer caching
- Gateways count tokens for rate limiting (token-based limits), cost pre-checks, and routing. Tokenization (tiktoken/BPE) is **computationally expensive** and per-model.
- Techniques: load tokenizers once at startup and cache per model (never per request); cache token counts for repeated prompt prefixes/system prompts; prefer **provider-reported usage from the response** for billing (exact) and use local estimation only for pre-flight admission; for streaming, use the final usage chunk (`stream_options: {include_usage: true}`) instead of re-tokenizing output. Envoy AI Gateway extracts input/output/total token metadata from responses and feeds the Global Rate Limit API rather than tokenizing inline. Rate limiting done right adds <4ms; done wrong (tokenize-everything-inline) it dominates overhead.

### 2.6 Caching & rate limiting on the fast path
- Response/exact cache (Redis or in-process moka/ristretto-style) and semantic cache: cache hits turn seconds into milliseconds and cut spend 30–75% — caching is a *performance feature* as much as a cost feature.
- Rate limit state: in-process atomics for single-node; Redis with local token-bucket smoothing for cluster mode. Bifrost ships an adaptive load balancer + cluster mode; Helicone does per-user/team/global limits by requests, tokens, or dollars.
- Auth lookups (API keys, org config) must be cached in memory with TTL — OpenRouter's "15ms once the cache is warm" wording reveals exactly this: edge-cached key/config lookups are the difference between 15ms and a cold-path DB roundtrip.

### 2.7 Deployment-shape effects
- Single static binary (Helicone ~30MB, ~100ms cold start; Bifrost ~80MB) vs LiteLLM's 500MB+ image and ~2s cold starts — matters for serverless/sidecar/per-node deployment and for "run it next to the agent."
- Sidecar/same-host deployment eliminates a network hop entirely; sub-ms gateways make sidecar-per-node viable.
- Edge-hosted (Portkey/OpenRouter/Cloudflare) trades self-host control for geographic proximity; their 15–40ms includes a WAN hop that a self-hosted binary doesn't pay.

---

## 3. Benchmark methodology: how to publish credibly

What the credible publishers all do (synthesis of TensorZero, Bifrost, Kong, Helicone):

1. **Mock the provider.** Everyone uses a mock OpenAI-compatible endpoint (WireMock, custom mock at fixed 60ms median) to isolate gateway overhead from provider variance. State the mock's latency distribution.
2. **Publish the harness.** TensorZero ships benchmark code in-repo (`benchmarks/` dir); Bifrost open-sourced its suite; Kong published artifacts; Helicone has `benchmarks/README.md` with full k6 + OTel + Fly.io setup. Unreproducible numbers get torn apart.
3. **Sustained, not burst.** Bifrost explicitly: "burst tests are misleading — a gateway might handle 5,000 RPS for 10 seconds but fall apart after a minute"; run 60s–3min+ at steady state. (Also catches LiteLLM-style degradation-over-time bugs — there's an open issue where perf decays over 2–3 hours until restart.)
4. **Pin everything:** exact instance type (t3.medium / c7i.xlarge / c5.4xlarge), OS, both software versions, date, CPU/memory allocations equalized across contenders (Kong gave every gateway 12 CPUs).
5. **Report full percentile ladder + success rate + memory:** mean/p50/p90/p95/p99 (p99.9 if you can), error rate, RSS, and the no-gateway baseline (Kong: mock alone did 29k RPS @ p95 24ms).
6. **Disclose what's off.** TensorZero says "observability disabled"; Helicone says "auth on, logging off." Credibility requires both the proxy-only number **and** a "with logging/auth/rate-limits enabled" number — Kong's own stated limitation is that pass-through-only doesn't reflect policy overhead. Publishing the *featured* config is an open differentiation opportunity: nobody leads with it.
7. **Known credibility holes to avoid:** colocating load generator + gateway + mock on one box (TensorZero does this; it understates contention), single workload shape (Kong used only 1000-token prompts), no variance/multiple-run reporting, comparing your tuned config vs competitor defaults (the recurring accusation in LiteLLM-vs-X fights — LiteLLM's docs respond with `x-litellm-overhead-duration-ms` so users can measure overhead themselves), and ignoring **coordinated omission** (use open-loop load gen like k6 arrival-rate executors, not closed-loop VU loops, when claiming RPS at latency).
8. **Benchmark streaming separately:** TTFT overhead, inter-chunk added latency, and concurrent-streams ceiling. Almost nobody publishes streaming-path overhead (LLMProxy's "<1ms first-token" is the rare example) — second open opportunity.
9. **Benchmark the MCP path too:** Envoy AI Gateway and agentgateway publish MCP routing overhead (1–3ms; ~0.2ms delta between them). A combined LLM+MCP gateway should publish both planes.

---

## 4. Weaknesses & complaints observed in the field

- **LiteLLM:** 1,000+ open GitHub issues; production reports of 1.7–4× throughput loss through the proxy; perf degradation over hours fixed only by restart (#6345); DB-logging-induced slowdown at modest volume; ~500MB/worker memory; needed an entire public roadmap to get overhead from ~40ms to 8ms median (multi-instance). Its ubiquity + features keep it dominant despite this.
- **Bifrost:** headline drifted from "11µs" to "<100µs" in the README; 11µs was the best-case instance (t3.xlarge) and is mean overhead vs a mock — marketing-forward; HN traction modest. Acknowledged limitation: mocked endpoints don't capture real provider variance.
- **TensorZero:** flagship number requires observability disabled; all components colocated on one instance; gateway is one piece of a heavier LLMOps platform (ClickHouse dependency for full value).
- **Helicone AI Gateway:** young (≈600 stars, 29 releases); license ambiguity reported (Apache-2.0 badge vs GPL-ish footer — verify before depending); benchmark run with logging off, and Helicone's value prop is logging.
- **Portkey/OpenRouter (hosted edge):** 15–50ms real-world adds; "sub-1ms" marketing vs 20–40ms reality gap is a recurring complaint; latency-sensitive users self-host instead.
- **Kong/Envoy:** fast cores but heavy operational machinery (control planes, CRDs, Kubernetes assumption) — overhead numbers are good but "single binary in 5 minutes" they are not.
- **Universal gap:** nobody publishes overhead **with the full policy/observability stack enabled**, streaming TTFT overhead, or long-duration (multi-hour) soak results.

---

## 5. Implications for a new single-binary OSS gateway (LLM + MCP)

**Table stakes (you will be benchmarked on these by default):**
- Sub-ms p99 self-overhead at thousands of RPS on a small instance; <100MB RSS; <100MB binary; ~100ms cold start; 100% success under sustained load; graceful degradation (shed load, never 90s p99s).
- Provider connection pooling + TLS reuse, per-provider isolation/circuit breaking, zero-buffer SSE passthrough, async batched logging that cannot block the hot path, in-memory auth/config cache, token counting from provider usage not inline tokenization.

**Differentiation opportunities (nobody owns these yet):**
- Publish **"fully-loaded" benchmarks** (auth+rate-limit+logging+guardrails on) alongside proxy-only — and a one-command reproducible harness in-repo.
- Publish **streaming TTFT overhead** and **MCP tool-call overhead** as first-class numbers (agent-chain framing: overhead × N tool calls).
- Multi-hour soak results (directly weaponizes LiteLLM's degradation issue).
- An LiteLLM-style **`x-overhead-duration` header on every response** plus Prometheus histograms of self-overhead — let users verify your claims continuously in their own prod (turn benchmarking into an always-on product feature).
- Rust (tokio/hyper/axum) is the credibility default for new entrants in 2026 (TensorZero, Helicone, agentgateway, Cloudflare all converged there); Go is defensible (Bifrost proves µs-class is reachable) and faster to extend — the decision should weigh plugin story and team velocity, since both languages clear the performance bar that actually matters.

---

## Sources
- https://github.com/maximhq/bifrost · https://www.getmaxim.ai/bifrost/resources/benchmarks · https://docs.getbifrost.ai/architecture/core/concurrency · https://github.com/maximhq/bifrost/blob/main/AGENTS.md
- https://www.tensorzero.com/docs/gateway/benchmarks · https://github.com/tensorzero/tensorzero
- https://github.com/Helicone/ai-gateway (+ benchmarks/README.md)
- https://konghq.com/blog/engineering/ai-gateway-benchmark-kong-ai-gateway-portkey-litellm
- https://docs.litellm.ai/docs/aiohttp_benchmarks · https://github.com/BerriAI/litellm/discussions/15933 · https://github.com/BerriAI/litellm/issues/21046 · https://github.com/BerriAI/litellm/issues/6345 · https://docs.litellm.ai/docs/troubleshoot/latency_overhead
- https://www.solo.io/blog/designing-agentgateway-a-unified-high-performance-gateway-for-ai-and-api-traffic · https://agentgateway.dev
- https://aigateway.envoyproxy.io/blog/mcp-in-envoy-ai-gateway/ · https://tetrate.io/blog/envoy-ai-gateway-mcp-performance
- https://openrouter.ai/docs/guides/best-practices/latency-and-performance · https://news.ycombinator.com/item?id=46232006 · https://opper.ai/blog/llm-router-latency-benchmark-2026
- https://portkey.ai/docs/introduction/what-is-portkey · https://github.com/portkey-ai/gateway
- https://blog.cloudflare.com/20-percent-internet-upgrade/ · https://developers.cloudflare.com/ai-gateway/features/
- https://github.com/aiyuekuang/LLMProxy · https://dev.to/pranay_batta/how-we-benchmarked-bifrost-against-litellmand-what-we-learned-about-performance-c1o · https://www.haproxy.com/blog/http-keep-alive-pipelining-multiplexing-and-connection-pooling · https://www.truefoundry.com/blog/rate-limiting-in-llm-gateway
