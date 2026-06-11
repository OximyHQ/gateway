# Oximy Gateway — Design Doc

**Date:** 2026-06-10
**Status:** Design / approved-architecture, pre-plan
**Repo:** `github.com/oximyhq/gateway`
**Binary / CLI:** `oximy-gateway`
**License:** Apache-2.0 (whole repo, no license keys)
**Supersedes:** key calls in `2026-06-09-llm-gateway-scope.md` (see §0).

---

## 0. What this supersedes (and why)

The 2026-06-09 scoping doc proposed a **Go** data plane evolved from `otel/auth-proxy`, a hard control-plane/data-plane split reusing the existing TS/Mongo/Next stack, MCP as a low-priority "P3 nice," and an explicit *don't-compete-on-breadth* stance. This design deliberately changes four of those calls per updated direction:

| Decision | 2026-06-09 | This doc (2026-06-10) | Why |
|---|---|---|---|
| Core language | Go, evolve `auth-proxy` | **Rust, from scratch** | Own the "fastest" headline credibly (Helicone/TensorZero/agentgateway are Rust, sub-1ms p99); a clean core avoids inheriting auth-proxy's shape. |
| Topology | CP/DP split on TS+Mongo+Next | **Single static binary** (SQLite→Postgres), embedded dashboard | Single-command run; no infra tax; the thing a developer or agent boots in one command. |
| MCP | P3 "nice" | **Co-equal plane on a shared spine** | The whitespace is *unified* LLM+MCP governance; making it P3 forfeits the differentiator. |
| Breadth | "don't compete on breadth" | **Breadth IS a goal** ("nobody leaves for a small thing") | Comprehensiveness is the explicit product mandate; achieved cheaply via provider-adapters + a hot-reloading model registry + cost-tracked passthrough, not 1000 hand-written integrations. |

**What survives** from the prior doc as Oximy-specific differentiators: authoritative call-time cost (`pricing.ts` logic), the OTEL/ClickHouse substrate as a first-class export target, per-end-user identity + cost attribution, idempotency-key-reuse to prevent double-billing, atomic budget reserve/commit/refund, and the independence/trust posture. These are folded in below.

Research basis: 65-agent competitive sweep in `docs/superpowers/research/2026-06-10-ai-gateway/` (`SYNTHESIS.md` = master feature matrix, steal-list, whitespace).

---

## 1. Thesis

Every feature in this market exists *somewhere*, but **no product combines (a) LLM + MCP in one OSS single binary, (b) a control plane that agents can fully operate, and (c) compiled-language performance with batteries-included governance.** That triple intersection is empty. Oximy Gateway is that intersection.

Positioning assets, free from the market's state: it is mid-consolidation (Portkey→Palo Alto, Helicone→Mintlify maintenance mode, Pydantic's gateway archived with no data portability, Katanemo→DigitalOcean). **"Independent, Apache-2.0, no rug-pulls, with one-command migration from the dying ones"** is both a wedge and free distribution.

**Non-goals:** becoming LiteLLM (breadth without invariants → reputation debt); requiring Kubernetes; AGPL; paywalling any shipped OSS feature; building routing *intelligence* (consume OSS routers behind a seam).

---

## 2. The Shared Spine (the central bet)

One core that **every** request flows through, LLM call or MCP tool call alike. The spine is protocol-agnostic: *tokens in, dollars out, policy everywhere.* Adapters translate; the spine governs.

```
   OpenAI / Anthropic /        ┌──────────────────────────────────────────┐
   Gemini / Responses  ───────▶│ LLM Ingress Adapters ─┐                   │
                               │                        ▼                  │
   MCP clients (Claude         │                  ┌───────────────┐        │
   Code / Cursor / ...) ──────▶│ MCP Ingress ────▶│  SHARED SPINE │        │
                               │                  │               │        │
                               │  identity · virtual keys · budgets│       │
                               │  policy/guardrails · audit · telemetry│    │
                               │  pricing · cache · rate-limit      │       │
                               │                  └──────┬────────┘        │
                               │  LLM Egress: 30+ providers │              │
                               │  MCP Egress: federated servers            │
                               └──────────────────────────────────────────┘
                                          oximy-gateway (one binary)
```

**The payoff nobody else has:** one virtual key's USD budget covers model tokens *and* tool calls; one audit log spans both; one guardrail policy applies to prompts *and* tool I/O; one telemetry store answers "what did this agent session cost" across LLM + MCP.

### Spine invariants (tested to death, the anti-LiteLLM discipline)
- **Fail-closed budgets**: hard block *before* the upstream call; never fail-open (LiteLLM's bypass bugs are the cautionary tale).
- **No double-billing**: a single idempotency key is reused across all retries/failovers of one logical request; cost is committed once from provider-reported usage.
- **No overspend under concurrency**: atomic reserve → commit (true-up) → refund across the budget hierarchy.
- **Auth-by-default**: admin API/UI/metrics authenticated out of the box (LiteLLM leaked PII on `/metrics`; APISIX's default admin key was exploited).
- **Cost correctness is security-grade**: cached-token math, streaming usage, aborted-stream usage are all reconciled; publish invoice-reconciliation accuracy.

---

## 3. Module decomposition (Cargo workspace in `oximyhq/gateway`)

Each crate has one purpose, a documented interface, and is independently testable.

| Crate | Responsibility | Key invariant |
|---|---|---|
| `gateway-spine` | identity, virtual keys, budgets, RBAC, audit log, policy engine, **pricing/capability registry** | the governance invariants in §2 |
| `gateway-llm` | LLM ingress adapters + egress provider transports + **translation core** | golden-fixture conformance per provider pair |
| `gateway-mcp` | MCP federation, transport bridging, virtual servers, OAuth broker, tool ACL/audit | stateless-first; spec-conformant |
| `gateway-route` | fallback / retry+backoff / hedging / weighted+latency LB / cache-affinity stickiness / circuit breakers | fallback only before first token |
| `gateway-cache` | exact-match + semantic + provider-prompt-cache passthrough | only cache 200s; correct cached-token accounting |
| `gateway-telemetry` | embedded columnar store (DuckDB-class) for logs/spend; OTel GenAI + MCP semconv; Prometheus | telemetry writes off the hot path (async/batched) |
| `gateway-guard` | guardrail hook stages (PII/injection/moderation), **WASM host** for plugins | pre/post/stream stages; observe-only + dry-run modes |
| `gateway-config` | one schema'd config source of truth; dump/diff/apply; hot reload; env interpolation | UI = API = CLI = MCP = Git, one engine |
| `gateway-control` | admin REST API + **admin-MCP server** + AXI-grade CLI surface | full API/UI/CLI/MCP parity |
| `gateway-dash` | embedded web dashboard (thin client of the REST API) | no capability the API lacks |
| `oximy-gateway` (bin) | wires everything; `oximy-gateway up` → boots + opens dashboard | single-command run, zero-config first boot |

Storage: **SQLite default → Postgres upgrade** for control-plane state; **embedded columnar** store for logs/analytics (escapes LiteLLM's Postgres-bloat death and TensorZero/Langfuse's ClickHouse infra tax). **Redis optional, never required** — gossip/memberlist (Bifrost model) syncs budgets/limits across replicas; serve-from-cache degraded mode when the DB is down. Provider keys encrypted at rest; Vault/AWS/GCP/Azure secret refs supported.

---

## 4. Provider & model strategy (1000+ models, the right way)

Two clean axes, never conflated:

1. **Providers = API shapes.** ~30 hand-written egress transports cover the wire formats essentially every model speaks: OpenAI, Anthropic `/v1/messages`, Google `generateContent`, Bedrock (SigV4), Vertex/Azure (AD), Cohere, Mistral, Groq, Together, Fireworks, DeepSeek, xAI, plus OpenAI-compatible self-hosted (Ollama/vLLM/SGLang/TGI).
2. **Models = a hot-reloading registry.** 1000+ models are rows in the pricing/capability registry (consume `models.dev` / LiteLLM price JSON + local overrides), not code. Adding a model is data, available day-0.
3. **Escape hatch = cost-tracked passthrough routes.** A brand-new provider endpoint works *immediately* — proxied with cost tracking — before a typed adapter exists; you only lose translation niceties, never availability.

This is strictly cleaner than "100+ providers" framing and yields effectively every model in existence with new ones live the day they ship.

---

## 5. Unified API surface (the superset)

### LLM ingress (any client, base-URL swap)
- OpenAI-compatible `/v1/chat/completions`, `/v1/responses` (**exact SSE event sequence** — strict clients reject partial sequences), `/v1/embeddings`, images, audio TTS/STT, `/v1/rerank`, batch + files passthrough, `/v1/models` (machine-readable: pricing, context, supported params, live per-provider endpoints — OpenRouter-grade for agent model selection), unified `count_tokens`.
- **Anthropic `/v1/messages` ingress** with `anthropic-beta`/`anthropic-version` header forwarding (Claude Code hard-requires it).
- **Gemini `generateContent` ingress.**
- Translation core: tool/function calling (incl. parallel) both directions; structured outputs (`json_schema` → Anthropic/Gemini equivalents) with forced-tool-call emulation fallback; vision/multimodal parts; unified `reasoning_effort` knob (Envoy-style) across providers; provider-agnostic `cache_control` translation with cached-token accounting; **explicit `UnsupportedOperationError` + dropped-param warnings** instead of silent degradation (Bifrost model); a published per-pair fidelity matrix.
- Streaming: normalized chunk/finish_reason semantics; tool-call deltas preserved; usage in final chunk (`stream_options.include_usage`); **never lose usage on aborted streams**.
- Per-request routing controls in the body (OpenRouter-style): `models[]` fallback array, provider prefs (order/sort/only/max_price), `:nitro`/`:floor` style suffixes — agents self-serve reliability per call.
- Response metadata headers: `x-served-by`, `x-fallback`, cache `HIT/MISS/age`, `usage.cost`, **`x-overhead-duration-ms`** (the always-on benchmark feature).

### MCP plane (the other half of unified)
- Federate N upstream servers behind one endpoint; **stdio ⇄ SSE ⇄ streamable-HTTP** bridging both directions; session lifecycle + **2026-07-28 stateless-core RC** readiness with a compat shim for 2025 session-ful clients.
- **Virtual/composite servers**: curated tool/prompt/resource subsets with rename/redescribe/annotation overrides (`readOnlyHint`, etc.).
- Tool namespacing (`server__tool`); per-server enable/disable; **per-tool allowlists per key/team** (the unified governance — only Bifrost does this in OSS today).
- **Inbound OAuth 2.1** resource server (PRM, `WWW-Authenticate`, PKCE, CIMD-first with DCR fallback); **outbound credential injection + OAuth brokering** with refresh (secrets never reach the client).
- **Tool-call audit log** (who/tool/args/outcome) on the same spine audit stream; health checks; OpenAPI→MCP conversion; registry import in the **official MCP Registry API** shape (`updated_since` sync); tool-description pinning/hashing (rug-pull alert); semantic on-demand tool discovery (`find_tool`/`call_tool`) to fight context bloat.
- Client auto-config emit for Claude Code/Desktop, Cursor, VS Code, Codex, Windsurf, goose.
- **MCP tool calls metered in dollars** and counted against the same budget as LLM tokens — zero prior art.

---

## 6. Request lifecycle (one spine, both planes)

**LLM call:** ingress adapter normalizes → spine authenticates key (local cache; miss → one control lookup) → atomic budget reserve + RPM/TPM/parallel check (429 + `Retry-After` on breach) → guard pre-hook (deny short-circuits) → cache lookup → router picks deployment (cache-affinity sticky to preserve provider prompt-cache discounts) → egress transport (streaming, byte-faithful, idempotency key) → guard post-hook on the stream → spine commits actual cost from provider usage (true-up/refund) → async telemetry write → response with `usage.cost` + overhead header. **Fallback fires only before the first token**; after first token, mid-stream failover/resume is attempted without double-charge.

**MCP tool call:** same spine. Ingress (OAuth 2.1 resource server) → spine auth + per-tool ACL + budget → guard pre-tool (block/secrets-scan) → outbound credential injection → federated upstream → guard post-tool result scan → spine meters the call in dollars → same audit log + telemetry store.

---

## 7. Agent-first control plane (the moat)

Three surfaces, **one API underneath** (the dashboard, CLI, and admin-MCP are all thin clients of the same REST API; config is the declarative projection of the same state).

- **Admin-MCP server** — Kong's **3-meta-tool** pattern (`search` → `get_schema` → `execute`), never hundreds of flat tools (Cloudflare's code-mode shows ~1K context tokens vs ~244K flat). An agent can install/configure servers, set guardrails, mint **self-provisioning attenuated sub-keys** (budget ≤ parent remaining, models/tools ⊆ parent, auto-expiring), and **query its own telemetry** ("why was p95 slow yesterday?", "what did this session cost?") with query-safety caps. Most of these are greenfield (no OSS gateway ships an admin-MCP server).
- **AXI-grade CLI** — `--json` everywhere, token-lean output, **semantic exit codes** (distinct "already exists"), idempotent mutations, `--dry-run`, definitive empty states, next-step hints. (`oximy-gateway up`, `oximy-gateway keys create`, `oximy-gateway config apply`, `oximy-gateway mcp add`.) Benchmarks favor a tuned CLI over MCP on cost/reliability; we ship both.
- **Config-as-code** — one JSON-Schema'd file; **decK-style `dump`/`diff`/`apply`**; hot reload; env interpolation; `validate`/`--dry-run`. Kills LiteLLM's yaml-vs-DB split brain.
- **Official "operate this gateway" Agent Skill** + `llms.txt`/`llms-full.txt` + every docs page as `.md` + `AGENTS.md` in-repo + OpenAPI for the admin API.

---

## 8. Differentiators (whitespace → features)

Mapped from `SYNTHESIS.md §4`. These are what nobody does well:

1. **Unified LLM+MCP governance** — one key, one budget, one policy, one audit, one telemetry store across both planes.
2. **Agent-operable control plane** — admin-MCP + AXI CLI + config diff/apply; agents install, configure, govern, debug end-to-end.
3. **Provider prompt-cache-aware sticky routing** — naive LB destroys Anthropic/OpenAI cache discounts; route to preserve them and alert on cache-affinity loss.
4. **LLM-aware request hedging** (fire backup at ~p95 TTFT, take first, token-bucket-capped) + **mid-stream failover/resume** — unsolved across all gateways.
5. **Guardrail/policy CRUD + test/simulate/dry-run as MCP tools and CLI verbs**, normalized cross-provider verdict schema.
6. **Agent-queryable own-telemetry over MCP** + **budget/cost introspection as agent calls** ("what will this cost", "cheapest model that fits", "what's left in my budget") + **MCP tool-call dollar accounting**.
7. **Honest fully-loaded benchmarks** — policies/auth/logging/guardrails ON, streaming TTFT overhead, MCP-path overhead, multi-hour soaks, open-loop load gen. The whole market benchmarks pass-through mode against mocks; we publish what they won't, with an in-repo reproducible harness.
8. **Conformance-tested translation** — golden fixtures against real agent clients (Claude Code, Codex CLI, Gemini CLI, AI SDK) + MCP conformance suite (SEP-2484) in CI + per-pair fidelity matrix. Stops the eternal streaming-regression whack-a-mole.
9. **Oximy-native cost & substrate** (carried from the prior doc) — authoritative cache-aware call-time USD as a primitive; per-end-user identity + cost attribution; first-party clean `gen_ai.*`/MCP-semconv emit into the OTEL/ClickHouse substrate as a first-class *export adapter* (not a hard dependency).
10. **One-command migration tooling** from LiteLLM/Portkey/Pydantic configs — cheap distribution off the consolidation churn.

---

## 9. Performance target & engineering

**Bar to beat** (published): Bifrost 11µs mean overhead @5k RPS (Go); TensorZero <1ms p99 @10k QPS / 4 vCPU (Rust); agentgateway <0.2ms p99 @30k QPS (Rust); Helicone <1ms, <100MB RAM, ~100ms cold start (Rust).

**Our target:** sub-100µs–1ms p99 self-overhead at 5–10k RPS on 4 vCPU, <100MB RSS, <50MB binary, ~100ms cold start, 100% success in 60s+ sustained runs — **then differentiate by publishing fully-loaded numbers (policies ON), streaming TTFT, MCP-path overhead, and multi-hour soak.** The `x-overhead-duration-ms` header makes the benchmark an always-on product feature.

Techniques: connection pooling + HTTP/2; zero-copy streaming; tokenizer/registry caches; async batched telemetry off the hot path; zero synchronous control-plane calls on the hot path (local cache, ≤1 optional Redis round-trip); **WASM (wasmtime) plugin sandbox** so a slow/crashing community plugin can't breach the latency SLA or crash the server (panic isolation).

---

## 10. Phased build (each phase ships something usable)

> Full phase breakdown lands in the implementation plan (writing-plans). Summary:

- **P1 — Spine + LLM core.** `gateway-spine` invariants; OpenAI/Anthropic/Responses ingress; ~6 Tier-1 egress transports + passthrough + model registry; virtual keys + USD budgets + rate limits; exact cache; authoritative cost + logs; single binary + embedded dashboard + zero-config first boot; conformance suite green. *→ a standalone best-in-class LLM gateway.*
- **P2 — MCP plane on the same spine.** Federation, transport bridging, virtual servers, inbound OAuth 2.1 + outbound brokering, per-key tool ACL, tool-call audit + dollar metering. *→ unified budgets/audit become real.*
- **P3 — Agent-first control plane.** Admin-MCP (3 meta-tools), AXI CLI, config dump/diff/apply, self-provisioning attenuated sub-keys, agent-queryable telemetry, Agent Skill.
- **P4 — Differentiators.** Hedging, mid-stream failover, semantic cache, guardrail dry-run/simulate, prompt-cache-affinity routing, OTEL/ClickHouse export adapter, full conformance + honest fully-loaded benchmark suite.
- **P5 — OSS ops + breadth completion.** Remaining providers/modalities (images/audio/rerank/video/realtime), guardrail plugin ecosystem (adopt a partner-compatible manifest+handler contract over the WASM/sidecar host), signed releases, nightly e2e vs live providers, Helm/brew/npx/Docker, migration tooling.

---

## 11. OSS operations & licensing

- **Apache-2.0**, DCO, **no license keys**, public "OSS features never shrink" covenant (LiteLLM moving Prometheus behind the paywall = most-resented move in the category; AGPL killed Helicone's gateway).
- **Supply chain = buying criterion** (the gateway is the vault door): cosign-signed artifacts, SBOM + provenance, isolated publish creds, auth-by-default everywhere, panics never crash the API server. (LiteLLM's PyPI backdoor + CVSS-10 admin-UI RCE are the cautionary tales.)
- Release engineering: nightly → RC → stable; **cargo-dist** one-tag fan-out (brew/deb/rpm/Docker/installer); 12-hour load-test gate before stable; **in-repo reproducible benchmark harness**; docs served as an MCP server.
- Distribution: `curl | sh` installer, `brew`, `npx` wrapper of the Rust binary (Helicone pattern), Docker, optional Helm (K8s never required).
- **Monetize org-scale governance + managed cloud, never tokens, never the control plane.** Basic SSO free (undercut the "SSO tax"); SCIM/org-scale RBAC/compliance can be the eventual commercial line — but nothing shipped as OSS is ever clawed back.

---

## 12. Top risks & mitigations

| Risk | Mitigation |
|---|---|
| **Becoming LiteLLM** (breadth without invariants → reputation debt) | Pure translation core with golden fixtures per agent client + CI conformance + fuzzed SSE; spine invariants in §2 are non-negotiable, tested first. |
| **Streaming + tool-call-delta correctness** (where every clone breaks) | Tier-1 concern from P1; fallback only before first token; idempotency-key reuse; synthesized usage chunk; conformance gates merges. |
| **Latency erodes the Rust advantage** (agents do N sequential calls) | Zero sync control-plane calls; ≤1 Redis round-trip; guardrails in WASM with timeouts + async mode; continuous benchmark, regressions block release. |
| **Overspend / double-billing under concurrency + failover** | Atomic reserve/commit/refund at every level; constant idempotency key across pre-first-token retries; fail-closed hard budgets. |
| **Provider-dialect maintenance treadmill** | Keep native scope to ~30 shapes; long tail via passthrough + registry; nightly conformance vs live sandboxes; budget permanent upkeep. |
| **Security (the vault door)** | Auth-by-default; signed artifacts + SBOM; WASM plugin isolation; SSRF allowlists on custom-host/passthrough; never log secrets; `--block-secrets` default-on for MCP payloads. |
| **Standards churn** (MCP 2026-07-28 RC removes handshake; OTel GenAI semconv unstable; A2A contested) | Stateless-first with a compat shim; dual-emit telemetry; A2A incremental behind a seam. |
| **Bloat hostility** (Kong/APISIX plugin confetti; ContextForge 300+ env vars) | One coherent schema'd config, not plugin sprawl; humans and agents read the same config. |

---

## 13. Open questions for plan stage

1. **Embedded analytics engine** — DuckDB vs an embedded ClickHouse-lite vs a custom columnar log. (DuckDB leans recommended for single-binary fit.)
2. **Dashboard tech** — embed a prebuilt SPA (React/Svelte) as static assets in the Rust binary vs server-rendered. (Static SPA leans recommended for API-parity discipline.)
3. **Guardrail plugin host** — WASM-only vs WASM + optional out-of-process sidecar for heavy JS/Python partner plugins. (WASM-first, sidecar as P5 escape hatch.)
4. **Oximy substrate coupling** — confirm the OTEL/ClickHouse emit ships strictly as an optional export *adapter* (default-off), preserving the standalone/independent posture.
5. **Phase-1 cut line** — is the P1 provider set (~6 Tier-1) + exact-cache + single-tenant→multi-tenant keys the right "smallest real gateway," or pull any P2/P4 item earlier?

---

*Companion research: `docs/superpowers/research/2026-06-10-ai-gateway/SYNTHESIS.md` (master feature matrix, steal-list of 42 attributed best-in-class ideas, whitespace), plus 45 per-competitor/per-dimension reports and naming analyses.*
