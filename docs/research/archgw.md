# Arch Gateway (Katanemo archgw) — now "Plano" — Competitive Intelligence Report

Date: 2026-06-10
Subject: Arch Gateway / archgw by Katanemo — Envoy-based "intelligent prompt gateway" for agents
Researcher note: **archgw has been rebranded to "Plano"** (github.com/katanemo/archgw now redirects to github.com/katanemo/plano; docs moved from docs.archgw.com to docs.planoai.dev). CLI renamed `archgw` → `planoai`. **Katanemo Labs was acquired by DigitalOcean in April 2026**; CEO Salman Paracha is now DigitalOcean SVP of AI. Plano is being positioned as part of DigitalOcean's "inference cloud for the agentic era."

---

## 1. Identity & Positioning

- **What it is:** An AI-native, out-of-process proxy server and "data plane for agents." Built on **Envoy** by core Envoy contributors (Salman Paracha was Envoy/Istio-adjacent at Lyft/AWS). The core thesis: prompts are nuanced, opaque "requests" that deserve the same treatment as HTTP traffic — secure handling, intelligent routing, observability, API integration — handled **outside business logic**.
- **Evolution:** Started (Oct 2024 Show HN) as an "intelligent prompt gateway" with function-calling-at-the-edge. Scope expanded through 2025 into "delivery infrastructure for agentic apps": agent orchestration/handoff, guardrail filter chains, zero-code traces, and unified LLM access. Renamed Plano in late 2025/early 2026.
- **Differentiating bet:** purpose-built **small LLMs (1–4B) live inside the gateway's request path** — for routing, intent classification, function calling, and jailbreak detection — rather than rules or config-only routing. "Models-native proxy."
- **Architecture components:** Envoy (proxy core) + WASM plugins (Rust) + **"Brightstaff"** (Rust controller/sidecar process that runs/calls the small LLMs) — bundled into one container/binary set managed by supervisord; CLI in Python; some TypeScript (demos/dashboard bits).
- **Languages:** Rust ~70%, Python ~18%, TypeScript ~10%. License: **Apache-2.0**. ~6.6k GitHub stars, 67 releases, latest 0.4.24 (June 2026).

## 2. Feature Surface (full enumeration)

### 2.1 Unified LLM access / provider layer
- Single OpenAI-compatible `/v1/chat/completions` ingress; works unmodified with OpenAI Python SDK, **Anthropic Python SDK** (native `/v1/messages` handling), cURL/any HTTP client — just point `base_url` at the gateway.
- **15+ first-class providers:** OpenAI, Anthropic, DeepSeek, Mistral, Groq, Google Gemini, Together AI, xAI, Moonshot/Kimi, Zhipu (GLM), Xiaomi MiMo, Azure OpenAI, Amazon Bedrock (Converse API), Qwen/DashScope, Ollama, DigitalOcean (post-acquisition), plus generic **OpenAI-compatible custom providers** via `provider_interface` + `base_url`. Recent additions: **OpenRouter, Vercel AI Gateway, "ChatGPT subscription" provider, Kimi Code API (for routing Claude Code traffic)**.
- **Wildcard model config** (`openai/*`) auto-registers all known models of a provider.
- **Passthrough auth**: forward client `Authorization` header instead of gateway-stored keys.
- **Multiple instances** of one provider with different credentials (named providers).
- Built on Envoy's cluster subsystem: retries, automatic failover/cutover between upstream LLMs, resilient upstream connection management ("traffic management").

### 2.2 LLM routing (the signature feature)
Three strategies, mixable in one config:
1. **Model-based** — explicit `provider/model` (e.g. `openai/gpt-5.2`).
2. **Alias-based** — semantic aliases (`fast-model`, `reasoning-model`) decoupling app code from providers; swap targets in YAML only.
3. **Preference-aligned routing** — the headline differentiator. **Arch-Router-1.5B** (paper: arXiv 2506.16655, "Arch-Router: Aligning LLM Routing with Human Preferences") / successor **Plano-Orchestrator** infers `domain` (subject) + `action` (operation type) from the prompt and matches it to **user-written, human-readable routing policies** (`routing_preferences` with candidate model pools + fallback chains). Decouples "how to choose" (policy) from "what to run" (model assignment). Claimed ~**50 ms routing decisions**; the 1.5B model claims to beat top proprietary LLMs on routing benchmarks. Self-hostable via Ollama/vLLM, or use Katanemo's free hosted US-central inference.
- Documented router limitations: **no multi-modality, no function-calling awareness, no system-prompt dependency** — semantic preference matching only.
- Redis-backed session cache for **cross-replica model affinity** (0.4.19+).

### 2.3 Agent orchestration & handoff
- Declare agents in YAML (`id`, `url`, rich natural-language `description`); a listener with `type: agent` + `router: plano_orchestrator_v1` routes each turn to the best agent. Agents are plain **OpenAI-compatible HTTP chat-completion endpoints** in any framework/language.
- Default orchestrator model: **Plano-Orchestrator-30B-A3B** (MoE), claimed "foundation-model performance at 1/10th the cost"; 4B / 4B-FP8 / 30B-A3B-FP8 variants for self-hosting.
- Intent re-analysis on every follow-up turn → automatic **agent handoff** mid-conversation; agents never call each other directly. Add agents without touching app code.
- Routing models are constrained: they **don't generate responses; fall back to static policies on failure**.

### 2.4 Function calling / prompt targets (legacy, deprecated v0.4.22)
- `prompt_targets`: map intents to backend API endpoints with typed parameters (type, description, enum, required); the **Arch-Function** model family (1.5B/3B/7B, Qwen-2.5-based; later Arch-Function-Chat, Arch-Agent up to 32B) extracts parameters and invokes APIs directly at the edge. Supported single/parallel/multiple/combined function calls, automatic parameter validation, default targets.
- Published perf: **Arch-Function-3B ≈ 12x GPT-4 throughput, 44x cheaper**, on par with GPT-4 quality for function calling (VentureBeat/MarkTechPost coverage, BFCL-class benchmarks).
- **Deprecated in favor of agents + standard tool definitions** as of v0.4.22 — a notable strategic retreat from "function calling at the edge."

### 2.5 Guardrails / safety
- `prompt_guard`: centralized jailbreak detection (Katanemo Arch-Guard model) applied to ingress prompts with zero app code.
- **Filter Chains** (newer model): ordered request/response filters attached to listeners/agents; filters are **in-process MCP filters or external HTTP filter services**; raise `ToolError` → 400 with explanatory message. Used for jailbreak protection, moderation policies, domain enforcement, and **memory hooks**.

### 2.6 Observability
- **Zero-code**: every request traced end-to-end with **OpenTelemetry**, W3C Trace Context propagation; compatible with existing OTEL tooling. Configurable sampling. Custom span attributes.
- **"Agentic Signals™"** (trademarked, unique): model-free, O(messages) behavioral detectors run on live trajectories — 7 categories across 3 layers: Interaction (misalignment, stagnation, disengagement, satisfaction), Execution (tool failures, loops/parameter drift/oscillation), Environment (API errors, timeouts, rate limits, context overflow). Emitted as span attributes (0–100 quality scores, severity), span events (confidence + matched text), and 🚩 markers on span names. Uses: trace triage, smart sampling (claimed 82% informativeness vs 54% random), fine-tuning dataset construction, dashboards, alerting. **On by default** (`overrides.disable_signals: true` to disable).
- **Prometheus metrics endpoint + shipped Grafana dashboard** (0.4.22): latency, token usage, error rates.
- Structured **access logging**.
- **`planoai obs`** — live LLM observability TUI; **`planoai trace`** — CLI trace inspection with `--filter`, `--where`, `--since 5m`, JSON output, interactive + non-interactive modes.

### 2.7 State / memory
- Conversational state guide; Redis-backed session cache; "memory hooks" via filter chains. (Thin compared to dedicated memory products.)

### 2.8 CLI & developer experience
- `pip install archgw` → now `planoai`. Commands: `up` (start from YAML / `--path` dir / `--foreground`), `down`, `build` (Docker image from source), `logs`, `init` (interactive config generation/templates), `trace`, `obs`, `cli_agent` (interactive CLI agent session), `--with-tracing` (spins up local OTLP collector, port 4317).
- **Zero-config proxy mode** with auto-detected providers from env vars (0.4.20).
- **Agent skills framework + rule set** for the CLI agent (0.4.20).
- Docs ship an **llms.txt** page (agent-readable docs surface).

## 3. Config & Deployment Model

- **Everything is one declarative YAML** (`plano_config.yaml` / formerly `arch_config.yaml`): `listeners`, `llm_providers`, `model_aliases`, `routing_preferences`, `agents`, `filters`/`filter_chain`, `prompt_targets` (legacy), `tracing`, `overrides`. No dashboard-driven config; no hot-reload control API documented.
- **Native deployment (default):** `planoai up` on the host; pre-compiled Envoy + WASM plugins + Brightstaff binaries cached in `~/.plano/`. Linux x86_64/aarch64 + macOS Apple Silicon.
- **Docker:** official `katanemo/plano:<ver>` image, docker-compose example; ports **10000 (ingress)** and **12000 (egress/LLM gateway)**.
- **Kubernetes:** single stateless container; ConfigMap-mounted YAML + Secret-injected keys; internal supervisord runs Envoy/WASM/Brightstaff. **No Helm chart, no operator, no documented horizontal-scaling guidance.**
- **Model serving:** the small LLMs (router/orchestrator/guard) are the catch — either use Katanemo's **free hosted US-central inference** ("great first-run experience"; production = run locally on Ollama/vLLM/SGLang **or contact us on Discord for API keys") or self-host with GPU. This is the hidden operational cost of the models-in-the-path design.

## 4. API surface

- OpenAI-compatible chat completions (+ streaming) at the egress listener; Anthropic /v1/messages compatibility; agent-listener ingress for orchestration. OTLP trace export; Prometheus scrape endpoint. No documented admin/management REST API, no virtual-key/team management API, no budget API.

## 5. Pricing / commercial

- 100% open source, Apache-2.0; no paid tiers ever published. Monetization path was the hosted model inference; company took seed funding then **sold to DigitalOcean (April 2026)** — expect Plano to become DigitalOcean's managed agent-gateway / Gradient platform component. Community support via Discord.

## 6. Published performance claims

- Arch-Router-1.5B: routing decision ~**50 ms**, beats proprietary foundation models on preference-routing accuracy (arXiv 2506.16655).
- Arch-Function-3B: **~12x GPT-4 throughput, ~44x cost reduction**, GPT-4-level function-calling quality; similar vs GPT-4o/Claude 3.5 Sonnet.
- Plano-Orchestrator-30B-A3B: "foundation-model performance at 1/10th the cost."
- Envoy-grade proxy performance implied but **no end-to-end gateway throughput/latency benchmarks published** (no req/s, no P99 overhead numbers).

## 7. Weaknesses & criticisms observed

- **Brand chaos:** Arch → archgw → Plano; docs/links split across docs.archgw.com and docs.planoai.dev; founders admitted the "Arch" name was an SEO disaster on HN. Community mindshare reset twice.
- **Acquisition risk:** DigitalOcean ownership creates uncertainty about neutral, community-driven roadmap; free hosted model endpoint could be folded into DO billing.
- **Models-in-the-path operational burden:** preference routing/orchestration/guardrails all require serving 1.5B–30B models — free hosted dev tier, but production = self-host on GPU or "ask on Discord for API keys." Heavy vs config-only gateways.
- **Multi-process heavyweight "single deployment":** Envoy + WASM + Brightstaff + supervisord in one container — not a true single binary; harder to embed.
- **Function-calling-at-edge (original wedge) deprecated** in v0.4.22 — signals the edge-function-calling thesis didn't survive contact with the agent/tool-definition world.
- Router model explicitly **can't handle multimodality, function-calling-aware routing, or system-prompt-dependent routing**.
- **No dashboard/UI** for config, keys, costs, or analytics (Grafana dashboard + TUI only). No virtual keys, no per-team budgets/rate limits, no cost-tracking ledger — the whole FinOps/governance layer that LiteLLM/Portkey/Kong sell is absent.
- **No documented MCP *gateway*** capability (MCP appears only as in-process filter plumbing) — no MCP server federation/tool governance.
- HN skepticism: "several dozen AI gateways already exist; Kong is ahead in features/adoption — what's the differentiator?"; "why bring all of Envoy's complexity?"; Envoy AI Gateway (Bloomberg/Tetrate) overlaps on its home turf.
- Small community (~6.6k stars, sparse third-party production reports); HN commenter openly asked how the static-policy fallback "holds up in prod" — unanswered.
- K8s story is thin: single container, no Helm/operator/autoscaling guidance; cross-replica affinity needs Redis.

## 8. Agent-experience (AX) notes — how agents are expected to use it

- Agents are first-class *clients and targets*: any OpenAI-compatible HTTP endpoint becomes a routable agent; gateway does intent analysis + handoff per turn so agents stay framework-agnostic.
- Docs publish **llms.txt**; config is pure declarative YAML (easy for an agent to generate/diff); `planoai init` templates.
- `planoai trace` has explicit **non-interactive/JSON modes "for automation"** — trace data is machine-consumable; Signals are structured span attributes/events designed for programmatic triage/sampling/dataset building.
- `planoai cli_agent` + agent-skills framework: the CLI itself ships an interactive agent with rules/skills.
- Zero-config proxy mode (env-var-detected providers) minimizes setup friction for agent harnesses; Claude Code traffic routing via Kimi Code API shows they target coding-agent traffic interception.
- Gaps: no MCP control plane, no admin API an agent could drive, no machine-readable config-mutation surface (must rewrite YAML + restart).

## 9. Ideas worth stealing for a new OSS gateway

1. **Preference-aligned routing as readable policy** (domain/action taxonomy mapped to model pools) — decouple routing policy from model assignment; the 1.5B-model-at-50ms pattern is genuinely novel and validated by a paper.
2. **Agentic Signals**: cheap, model-free behavioral quality detectors attached to OTEL spans (quality scores, loop/failure/disengagement detection, 🚩 markers) — superb triage/sampling story nobody else has.
3. Zero-code OTEL tracing + shipped Grafana dashboard + a **live TUI (`obs`)** and JSON-mode `trace` CLI for agents.
4. Wildcard provider config (`openai/*`), passthrough auth, multiple named instances of one provider.
5. Filter chains accepting **in-process MCP filters or external HTTP filters** — clean extensibility seam.
6. llms.txt in docs; `init` templating; zero-config env-var proxy mode.
7. Cautionary lessons: don't put mandatory GPU models in the hot path without a bundled-weights story; don't deprecate your wedge feature publicly; pick a searchable name once.

## Sources

- https://github.com/katanemo/plano (formerly katanemo/archgw)
- https://docs.planoai.dev/ (full doc tree: concepts/listeners, agents, filter_chain, llm_providers, prompt_target, signals; guides/orchestration, llm_router, function_calling, prompt_guard, observability, state; resources/cli_reference, configuration_reference, deployment, tech_overview)
- https://docs.planoai.dev/concepts/llm_providers/supported_providers.html
- https://docs.planoai.dev/concepts/signals.html
- https://docs.planoai.dev/guides/orchestration.html, guides/prompt_guard.html, guides/function_calling.html, guides/llm_router.html
- https://docs.planoai.dev/resources/cli_reference.html, resources/deployment.html
- https://arxiv.org/abs/2506.16655 (Arch-Router paper)
- https://planoai.dev/blog/arch-router-outperforming-foundational-models-in-llm-routing-with-a-1-5b-model
- https://huggingface.co/katanemo/Arch-Function-3B; https://github.com/katanemo/Arch-Function
- https://venturebeat.com/ai/arch-function-llms-promise-lightning-fast-agentic-ai-for-complex-enterprise-workflows
- https://news.ycombinator.com/item?id=41864014 (Show HN: Arch, Oct 2024)
- https://news.ycombinator.com/item?id=46517177 (Show HN: Plano)
- https://github.com/katanemo/plano/releases
- https://finance.yahoo.com/sectors/technology/articles/digitalocean-acquires-katanemo-labs-accelerate-130000617.html (DigitalOcean acquisition)
