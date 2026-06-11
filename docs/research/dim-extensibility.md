# Dimension Deep-Dive: Extensibility (Plugin / Middleware Architectures in AI Gateways)

Research date: 2026-06-10. Question: across the major AI/LLM gateways, what extensibility architectures exist (hooks, plugins, custom providers, custom guardrails, webhooks), and which architecture lets a community contribute safely AND fast for a new open-source single-binary gateway (unified LLM gateway + MCP gateway + dashboard + agent-first CLI/MCP control plane)?

---

## 1. LiteLLM — Python in-process callbacks/hooks (the "just import Python" model)

**Architecture.** Everything extends Python base classes loaded in-process by the proxy. A registry maps string identifiers to logger classes; global callback lists fire at lifecycle points. Custom code is referenced from `config.yaml` (`litellm_settings.callbacks: custom_callbacks.proxy_handler_instance`) — a Python module path on the proxy's filesystem/PYTHONPATH.

**Extension surfaces (the broadest of any gateway):**
- **CustomLogger callbacks** — `log_success_event`, `async_log_success_event`, failure events, etc. → powers 20+ observability integrations (Langfuse, Prometheus, DataDog, OTEL, S3/GCS). A singleton `LiteLLMLoggingWorker` with an `asyncio.Queue` runs callbacks off the response path.
- **Proxy call hooks** (on the same CustomLogger class): `async_pre_call_hook` (modify/reject request; return string rejection message), `async_moderation_hook` (runs *in parallel* with the LLM call; reject-only, can't modify), `async_post_call_success_hook` (modify non-streaming response), `async_post_call_failure_hook` (transform errors), `async_post_call_streaming_hook` / `async_post_call_streaming_iterator_hook` (per-chunk stream filtering), `async_post_call_response_headers_hook` (inject response headers). Call types: completion, text_completion, embeddings, image_generation, moderation, audio_transcription.
- **CustomGuardrail** (subclass of CustomLogger) — simplest path is a single `apply_guardrail(text)` method; modes `pre_call` / `during_call` (parallel, lower latency) / `post_call` (audit-only for streams); declared in `guardrails:` config yaml; **per-request enablement** by passing `guardrails: [...]` in the request body, plus dynamic params via `extra_body`.
- **CustomLLM custom providers** — implement `completion/acompletion/streaming/astreaming/image_generation/...`, register in `litellm.custom_provider_map`, call as `"my-custom-llm/my-model"`; streaming via `GenericStreamingChunk`. Configurable from proxy config.yaml.
- Also: custom auth (`user_api_key_auth` override), custom routing strategies (CustomRoutingStrategyBase), custom pricing per model, pass-through endpoints for arbitrary provider routes, secret-manager backends, prompt-management hooks.

**Safety/sandboxing: none.** Plugins are arbitrary Python in the proxy process — full memory access, can block the event loop, crash the proxy, exfiltrate keys. "Safety" is convention (async hooks, don't block, handle exceptions).

**Distribution/registry: none.** No plugin marketplace; you mount a .py file. Community contributions land as PRs into the monolith (which is why litellm has 20+ vendor integrations baked into core — and a giant dependency tree).

**Known pain:** hooks bypassed on some endpoints (documented bug: `async_pre_call_hook` skipped on Anthropic `/v1/messages`, May 2026); `async_moderation_hook` chat-only; hooks didn't historically fire on pass-through endpoints (issue #4675); fragile interplay between callbacks; no interface-stability guarantee — internals churn with LiteLLM's famously fast release cadence.

**Verdict:** fastest authoring experience in the ecosystem (a guardrail is ~10 lines of Python), zero isolation, zero distribution story, weak interface stability.

## 2. Portkey — TypeScript plugin folder + webhook escape hatch

**Architecture.** The gateway (TypeScript/Hono, runs on Node/Workers) has a `plugins/` directory; each plugin = `manifest.json` (properties, credentials, functions) + `main-function.ts` + `test.test.ts`. Plugins attach to exactly **two hook points**: `beforeRequestHook` and `afterRequestHook`. Guardrails/mutators map onto them (inputGuardrails→beforeRequestHooks, outputGuardrails→afterRequestHooks, inputMutators/outputMutators likewise). Checks return verdicts; configs can `deny`, log feedback, or trigger retries/fallbacks based on guardrail verdict — the distinctive idea is **routing on guardrail verdicts** (fallback to another model if a check fails).

- ~50+ guardrail plugins (regex, JSON schema, PII, moderation, plus partner integrations: Patronus, Pangea, Aporia, etc.) contributed in-repo via PRs (issue → PR with tests).
- **Webhook guardrail ("bring your own")**: gateway POSTs request/response to your HTTP endpoint at beforeRequest/afterRequest; webhook can return verdict AND a `transformedData` body that **fully overrides** the request or response. This is the language-agnostic escape hatch.
- **Limitations:** afterRequest hooks are skipped for streaming responses (cannot apply to streams); only two hook points (no per-chunk, no connection-level, no MCP hooks); plugin = guardrail-shaped (check/mutate), not general middleware; several advanced features (semantic cache, prompt mgmt, managed guardrail execution) historically cloud-only. Entire gateway went Apache-2.0 open source ~March 2026; Portkey was acquired by Palo Alto Networks ~April 2026 → roadmap uncertainty for OSS plugin ecosystem.

**Safety:** in-process JS — same trust model as LiteLLM, though the manifest+verdict-object shape constrains what a plugin is supposed to do, and TS types + mandatory unit tests give a contribution baseline. On Cloudflare Workers deployments, V8 isolates give some incidental sandboxing.

**Verdict:** best-documented *contribution workflow* (manifest + tests + folder convention), weakest *hook surface* (2 points, no streaming).

## 3. Bifrost (Maxim) — native Go plugins, the richest hook taxonomy, the worst loading mechanism

**Architecture.** Go interfaces in `core/schemas/plugin.go`: `LLMPlugin`, `MCPPlugin`, `HTTPTransportPlugin`, `ObservabilityPlugin`. Lifecycle: `Init(config)` → hooks → `Cleanup()`. Hook taxonomy (v1.4–1.5+):
- `HTTPTransportPreHook` / `HTTPTransportPostHook` — raw HTTP in/out (PostHook NOT called for streaming).
- `HTTPTransportStreamChunkHook` — per-chunk streaming interception (typed chunk structs).
- `PreLLMHook` / `PostLLMHook` — provider-bound request/response; PreHook can short-circuit via `*LLMPluginShortCircuit` (cache hits, auth failures, rate limits).
- **MCP hooks (v1.5+, unique in market):** `PreMCPConnectionHook`, `PostMCPConnectionHook`, `PreMCPHook`, `PostMCPHook` — tool-call-level governance.
- Pre-hooks run in registration order, post-hooks in reverse (onion model). Typed plugin context: `ctx.SetValue()` cross-hook state, `ctx.Log()` scoped structured logging.
- First-party plugins ship compiled-in (governance/budgets, logging, telemetry, semantic cache, maxim, otel); custom plugins load as **Go `plugin` package `.so` files**.

**The .so problem (their Achilles heel, well known in the Go community):** must compile with the **exact same Go toolchain version** (docs literally say pin go 1.26.1), exact same `bifrost/core` version, same build tags/flags; CGO required; **no Windows**; no cross-compilation (build on target platform); every Bifrost upgrade forces rebuilding all plugins; mismatches fail at load with cryptic errors (debug via `go version -m`). Go's own docs warn the application and its plugins "must all be built together by a single person or component." They deprecated an earlier WASM plugin approach in favor of native .so for performance.

**Performance claims:** 11–100µs overhead at 5k RPS, "50x faster than LiteLLM"; plugins are in-process function calls so hook overhead is ~zero.

**Verdict:** the best *hook design* to steal (4 plugin interfaces incl. MCP + stream-chunk hooks + typed short-circuit), crippled for community contribution by Go plugin mechanics — in practice third-party plugins must be compiled into your own build (fork-and-recompile), which is exactly what a "community plugin" story shouldn't require.

## 4. Kong AI Gateway — Lua PDK + external plugin servers + proxy-wasm (the mature multi-tier reference)

**Architecture.** OpenResty/LuaJIT in NGINX workers. A plugin = `handler.lua` (phase handlers: `rewrite`, `access`, `header_filter`, `body_filter`, `log`, plus ws/stream phases) + `schema.lua` (typed config validation); optional `api.lua` (extend Admin API), `daos.lua` (custom DB entities), `migrations/`. Distribution via LuaRocks; enabled by name in config; explicit numeric plugin **priority ordering**. The Plugin Development Kit (PDK) is a **stable, versioned API contract** — the key institutional idea: plugins code against the PDK, never gateway internals.

**Three execution tiers, with measured tradeoffs (Kong's own docs):**
1. **In-process Lua** — fastest, native event loop.
2. **Proxy-Wasm filters** — run inside nginx via WasmX; portable across proxy-wasm hosts.
3. **External plugin servers** (Go, Python, JavaScript) — separate processes speaking RPC; every PDK call = IPC round trip, so performance degrades with PDK call count; Go can use multi-core, JS is single-core; clearly second-class.

**AI surface:** 60+ AI plugins (ai-proxy, ai-proxy-advanced w/ load balancing, ai-semantic-cache, ai-rate-limiting-advanced, ai-prompt-guard, ai-pii-sanitizer, ai-request/response-transformer) — all built as ordinary Kong plugins, proving the generic plugin chassis can carry the AI feature set. Custom AI logic typically composes existing plugins rather than new code.

**Pain:** Lua is a niche skill (the #1 community complaint); plugin enable/disable historically needs reload; Konnect/cloud tiers restrict custom plugin capabilities (e.g., no custom DAOs); AI plugins are increasingly Enterprise/Konnect-gated; the OSS edition has been progressively de-emphasized (2024–2026 licensing drama).

## 5. Higress / Envoy WASM — sandboxed Wasm plugins with OCI distribution (the "safe" pole)

**Architecture.** Envoy/Istio-based; all extensibility via **proxy-wasm plugins** (Go via `higress-group/wasm-go` SDK, also Rust/JS). Since Go 1.24, plugins compile with native `GOOS=wasip1 GOARCH=wasm go build -buildmode=c-shared` (previously TinyGo — a major DX upgrade). Four HTTP phases (request headers/body, response headers/body) with flow-control return codes (`HeaderContinue`, `HeaderStopIteration`, ...); **true streaming body processing** (SSE-aware per-chunk handling, low memory in high-bandwidth AI streaming); plugins can make external HTTP calls and Redis calls from inside the sandbox (no raw TCP).

**Safety + ops story (the differentiator):**
- Each plugin runs in its own Wasm sandbox: memory-safe, a crashing plugin cannot take down the gateway.
- Plugins are packaged as **OCI/Docker images**, pulled by the control plane, **hot-reloaded with zero traffic loss**, independently versioned per plugin.
- Config in YAML auto-delivered to the plugin as JSON; per-route/per-domain plugin config granularity.
- Official plugin library covers "90%+ of scenarios": ai-proxy (all major providers as a *plugin*), ai-quota, ai-search, ai-transformer, ai-security-guard, etc.
- **MCP servers as Wasm plugins**: Higress lets you author an MCP server with a Go SDK, compile to Wasm, and hot-load it into the gateway — gateway-hosted MCP tools with shared auth/rate-limit/observability, plus an "all-in-one" image bundling multiple MCP servers in one binary. Closest thing in the market to "extensible MCP gateway."

**Pain:** Wasm DX is the hardest of all (compile toolchain, Docker-compose local debug loop, opaque ECDS/OCI fetch failures breaking gateway startup — multiple GitHub issues); ~10–20% CPU overhead vs native, request/response copied into the VM; docs/community largely Chinese-first; K8s/Envoy operational footprint is heavy vs a single binary.

## 6. Envoy AI Gateway / ExtProc (the out-of-process pole)

Envoy proper offers 4 mechanisms: native C++ filters (recompile), Lua, proxy-wasm, and **ext_proc** — a gRPC service that receives request/response headers+body events and can mutate them or terminate the request. Envoy Gateway exposes this via the `EnvoyExtensionPolicy` CRD attachable to Gateway/HTTPRoute. Pros: any language, full process isolation, independent scaling. Cons: extra network hop per intercepted phase, you operate another service, streaming-body processing over gRPC is complex. APISIX has the analogous "plugin runner" sidecars (Java/Go/Python/JS over unix-socket RPC) and the same verdict applies: external runners are universally documented as the slow path (per-PDK-call IPC).

## 7. Cross-cutting: webhooks

Portkey webhook guardrails (verdict + full body override, pre/post), LiteLLM generic webhook alerts, Kong post-function/HTTP-log patterns. Webhooks are the universal zero-toolchain extension surface — every gateway that lacks them gets asked for them. None of the surveyed gateways offers a *signed/replayable* webhook contract for mutation with good streaming semantics.

---

## Comparative matrix

| | LiteLLM | Portkey | Bifrost | Kong | Higress | ExtProc/APISIX-runner |
|---|---|---|---|---|---|---|
| Language | Python (in-proc) | TypeScript (in-proc) | Go (.so, in-proc) | Lua native; Go/Py/JS ext; Wasm | Wasm (Go/Rust/JS) | any (gRPC/RPC) |
| Hook points | 6+ proxy hooks + logger + guardrail + provider + auth + routing | 2 (before/after) | 9+ incl. HTTP, LLM, stream-chunk, 4×MCP | 5+ nginx phases | 4 HTTP phases, streaming-native | header/body events |
| Streaming hooks | yes (iterator + per-chunk) | no (skipped) | yes (StreamChunkHook) | body_filter chunks | yes (best-in-class SSE) | yes but complex |
| MCP hooks | no | no | **yes (v1.5)** | MCP plugins exist, no MCP hook class | MCP servers AS plugins | no |
| Short-circuit | yes (reject/string) | yes (deny verdict) | yes (typed ShortCircuit) | yes (kong.response.exit) | yes (SendHttpResponse) | yes (immediate response) |
| Sandbox/isolation | none | none (V8 isolate on Workers) | none | none (Lua) / Wasm tier | **full Wasm sandbox** | full process isolation |
| Hot reload of plugins | restart | redeploy | restart | reload | **zero-loss hot swap via OCI** | independent deploy |
| Distribution | none (mount .py) | in-repo PR | none (fork/recompile) | LuaRocks + Plugin Hub | **OCI registry images** | n/a |
| Config validation | none formal | manifest.json | Init(config any) | **schema.lua typed** | JSON→struct, per-route | CRD |
| Interface stability | weak (internals churn) | medium | weak (exact-version lock) | **strong (versioned PDK)** | strong (proxy-wasm spec) | strong (proto) |
| Authoring speed | **minutes** | hours | hours–days | days (Lua learning curve) | days (Wasm toolchain) | days (run a service) |
| Perf overhead | Python tax overall | low | ~0 (claims 11–100µs total) | ~0 Lua; IPC tax ext | 10–20% CPU, mem copies | network hop/phase |

## Performance numbers found
- Bifrost: "<100µs overhead @ 5k RPS", "50x faster than LiteLLM" (vendor claim, README).
- Proxy-wasm: 10–20% overhead vs native for network filtering; <2x slowdown CPU-bound (proxy-wasm spec); request payload copied into/out of VM.
- Kong: external (Go/Py/JS) plugin cost scales with PDK-call count (each = IPC); Lua fastest, Wasm middle, external slowest (Kong's own performance doc).
- Go .so plugins: in-process call speed, but build-matrix constraints (CGO, exact toolchain).

## What architecture lets community contribute safely + fast? (synthesis)

No incumbent has both. The market splits into:
- **Fast, unsafe**: LiteLLM Python / Portkey TS — minutes to write, runs with full gateway privileges, no distribution or stability contract.
- **Safe, slow**: Higress Wasm / ExtProc — sandboxed, hot-swappable, OCI-distributed, but toolchain-heavy and 10–20% overhead.
- **Fast for the vendor, hostile to community**: Bifrost Go .so — superb hook taxonomy, but exact-toolchain .so loading means third parties effectively can't ship plugins.

**Recommended design for a new Go single-binary gateway (steal list):**
1. **Steal Bifrost's hook taxonomy** — separate `HTTPTransport` / `LLM` / `StreamChunk` / `MCP(connection+tool-call)` plugin interfaces, typed short-circuit values, registration-order/reverse-order onion, cross-hook context store. The 4 MCP hooks are the only tool-call governance hook set in the market and directly serve an MCP gateway.
2. **Reject Go `plugin` .so loading.** Tier the extension surface instead:
   - **Tier 0 (built-in, Go interfaces)**: first-party + vetted community plugins compiled in, behind a Kong-style **versioned, stable PDK** — never expose internals.
   - **Tier 1 (sandboxed, community)**: embed a Wasm runtime (wazero = pure-Go, no CGO, keeps single-binary) speaking proxy-wasm-style ABI; plugins distributed as **OCI artifacts**, hot-reloaded, per-route config — Higress's safety/ops story without Envoy/K8s.
   - **Tier 2 (zero-toolchain)**: **webhook hooks** with Portkey's verdict+body-override contract but fixed: signed payloads, timeout budgets, and an explicit streaming contract (LiteLLM-style post-stream audit mode + optional chunk sampling).
3. **Steal Kong's schema.lua idea**: every plugin ships a typed config schema the gateway validates and the dashboard auto-renders into a settings UI.
4. **Steal LiteLLM's per-request guardrail enablement** (`guardrails: [...]` in request body + dynamic params) and its `apply_guardrail` single-method easy mode — the on-ramp should be one function.
5. **Steal Higress's "MCP server as plugin"**: let the community add MCP tools/servers as sandboxed plugins that inherit gateway auth/rate-limit/audit — this collapses "MCP gateway extension" and "plugin" into one concept.
6. **Streaming is the differentiator-by-default**: Portkey skips stream hooks, LiteLLM's post_call stream mode is audit-only, Bifrost only added StreamChunkHook recently. Design chunk hooks (inspect/redact/abort mid-stream) as a first-class contract from day one.
7. **Agent-first extensibility (AX)**: none of the incumbents lets an agent author/install a plugin via API — Kong needs LuaRocks+reload, Higress needs an OCI push, Bifrost needs a rebuild. A `gateway plugin new/test/install` CLI + MCP tools (list hooks, scaffold plugin, dry-run a hook against a recorded request, install Wasm/webhook plugin via API) would be unique. Notable precedent: Bifrost ships AGENTS.md in-repo; Higress ships a Claude skill for writing its Wasm plugins — vendors already expect coding agents to be the plugin authors.

## Weaknesses observed across the field (gap list)
- Hooks silently not firing on specific endpoints/protocols (LiteLLM /v1/messages bug) — need a hook-coverage invariant + conformance tests per route type.
- No gateway offers plugin-failure isolation *policy* (fail-open vs fail-closed per plugin) as first-class config except guardrail verdict actions.
- No marketplace with signing/verification anywhere in this space (Kong Plugin Hub lists, doesn't sandbox; Higress OCI images are unsigned by default).
- Version-lock pain (Bifrost .so, Kong custom plugins vs gateway upgrades) — a stable ABI (Wasm or webhook) is the only escape.
- Wasm debugging DX is universally bad — invest in a local harness (replay recorded requests through a plugin with verbose traces).

## Sources
- LiteLLM: docs.litellm.ai/docs/proxy/call_hooks, /docs/proxy/guardrails/custom_guardrail, /docs/providers/custom_llm_server, /docs/observability/custom_callback; github.com/BerriAI/litellm issues #27518, #4675; dev.to (yigit-konur) LiteLLM plugins guide
- Portkey: github.com/Portkey-AI/gateway plugins/Contributing.md + wiki "Guardrails on the Gateway Framework"; portkey.ai/docs bring-your-own-guardrails; TensorZero comparison; ChatForest 2026 review (open-sourcing + Palo Alto acquisition)
- Bifrost: docs.getbifrost.ai/plugins/getting-started, /enterprise/custom-plugins, /plugins/writing-go-plugin; github.com/maximhq/bifrost README + AGENTS.md; pkg.go.dev bifrost plugins
- Kong: developer.konghq.com/custom-plugins/reference, plugin-development pluginserver performance doc, konghq.com proxy-wasm blog, developer.konghq.com/ai-gateway
- Higress: higress.ai/en/docs wasm-go; github.com/alibaba/higress (plugins/wasm-go/extensions, mcp-servers); higress-group/wasm-go; Alibaba Cloud blog on MCP server hosting; GitHub issues #2977, HiClaw #429/#493 (Wasm fetch/debug pain)
- Envoy/ExtProc: envoyproxy.io ext_proc docs; gateway.envoyproxy.io ext-proc task; tetrate.io "4 Envoy Extensibility Mechanisms"
- APISIX: apisix.apache.org external-plugin + Wasm blog
- Go plugin pitfalls: pkg.go.dev/plugin warnings; golang/go issues #51955, #31354, #19569; alperkose.medium.com on Go plugin drawbacks
- proxy-wasm overhead: proxy-wasm/spec WebAssembly-in-Envoy.md; oneuptime Istio Wasm post
