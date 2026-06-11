# TensorZero — Competitive Intelligence Report

Date: 2026-06-10
Subject: TensorZero (open-source LLMOps platform / LLM gateway)
Researched for: a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane)

## 1. Identity & Positioning

- **What it is:** Open-source LLMOps platform that unifies five products in one stack: **LLM gateway, observability, optimization, evaluations, experimentation** — built around a "data & learning flywheel" (production metrics + human feedback → smarter/faster/cheaper models and agents).
- **Company:** TensorZero Inc., raised a **$7.3M seed** (FirstMark, Bessemer, Bedrock, DRW, Coalition, angels). Claims usage "from frontier AI startups to Fortune 10" and that it powers "~1% of global LLM API spend."
- **License:** Apache 2.0. Fully self-hosted; no SaaS tier today (FAQ: not generating revenue yet; future plan = managed enterprise service + the paid Autopilot product, OSS stays core).
- **Implementation:** Rust (~79% of repo), TypeScript (~15%, the UI), Python (~4%, SDK/recipes).
- **Release cadence:** Weekly-ish CalVer releases (2025.8.0 → 2026.6.0, latest June 4, 2026). Roadmap is public via GitHub milestones/labels.

Positioning vs. category: TensorZero deliberately frames itself NOT as "just a gateway" but as a feedback-loop platform. Its own comparison docs (vs LiteLLM, OpenRouter, Portkey, Kong, Langfuse, LangChain, DSPy, OpenPipe) concede traditional-gateway ground while claiming the optimization/experimentation flywheel as unique.

## 2. Architecture & Deployment Model

- **Gateway as a standalone Rust proxy** (not a library) — language/stack-agnostic; one Docker container. There's also an embedded gateway in the Python SDK, but it is **deprecated for removal in 2026.6+** — converging on proxy-only.
- **Datastores:**
  - **ClickHouse** — the observability/analytics backbone (inferences, feedback, datasets, evals). "Optional" in deploy docs (gateway runs without observability), but everything interesting requires it.
  - **Postgres** (optional) — added 2026.3.x as an observability backend option, and used for operational state (auth/API keys, experimentation state).
  - **Valkey/Redis** (optional) — caching/rate-limiting support infra.
- **Two-tier "relay gateway" topology:** edge gateways (per team) forward to a central relay gateway that enforces org-wide auth, rate limits, and credentials — teams run their own deployments without holding provider keys. This is their enterprise/governance story.
- **Configuration model:** GitOps-friendly **TOML files** (`tensorzero.toml`), splittable via glob (`--config-file path/**/*.toml`). Core abstractions: **functions** (your app's interface) → **variants** (prompt+model+params combos) → **models** → **providers**. Prompt templates (MiniJinja) + JSON Schemas for inputs/outputs live in config, not in app code.
- **Docs sections:** Gateway, Observability, Optimization, Evaluations, Experimentation, Operations, Deployment, Integrations, Comparisons; ships an `llms.txt` docs index for LLM consumption.

## 3. Gateway Feature Surface (full enumeration)

### Inference API
- Native TensorZero inference API (`/inference`) with functions/variants/episodes semantics.
- **OpenAI-compatible chat-completions endpoint** — use any OpenAI SDK (Python, Node, Go…) by changing the base URL; supports `raw_text` content blocks.
- **OpenAI Responses API** support (incl. reasoning content blocks while streaming).
- Streaming; structured outputs (JSON mode with schema enforcement); tool use / function calling (incl. provider-native tools like Anthropic web search, added 2026.1.6); multimodal inputs (images & files); embeddings generation; batch inference (provider batch APIs for cost reduction — a feature OpenRouter lacks); inference caching (read/write controls); `include_raw_response` escape hatch.

### Providers (~20 native + generic)
Anthropic, AWS Bedrock, AWS SageMaker, Azure OpenAI, DeepSeek, Fireworks, GCP Vertex AI (Anthropic + Gemini), Google AI Studio (Gemini), Groq, Hyperbolic, Mistral, OpenAI, OpenRouter, SGLang, TGI, Together, vLLM, xAI — plus **any OpenAI-compatible endpoint** (Ollama etc.). Notably fewer than LiteLLM's 100+; they rely on the OpenAI-compatible catch-all.

### Reliability & routing
- Retries with configurable policies; **static fallback chains** across providers, models, and even entire variants; load balancing; granular timeouts (per provider/variant/non-streaming/streaming TTFT).
- **No dynamic routing** (latency/cost/rate-limit-aware) — explicitly conceded vs LiteLLM. No request prioritization/queuing. No budgets.

### Governance / auth / limits (newer, 2026)
- Gateway API-key auth (`[gateway] auth.enabled = true`); API key management with per-session browser keys (2026.3.x).
- **Custom rate limits with granular scopes** (docs: "Enforce custom rate limits"); usage & cost tracking; centralizing credentials so clients never see provider keys; relay architecture for org-wide enforcement.
- Provider credentials: static (`env::VAR`), or **dynamic per-request** (`tensorzero::credentials` in the payload — caller supplies the provider key at inference time).

### Performance claims (published)
- **<1ms p99 gateway overhead at 10k+ QPS** on a c7i.xlarge (4 vCPU/8GB), 100% success rate, vs LiteLLM failing at ~1k QPS on the same box ("25–100× lower latency than LiteLLM under high throughput"). Benchmarks page + reproducible setup (mock OpenAI provider). LiteLLM has since published its own sub-millisecond-overhead rebuttal blog, so the gap is contested.
- 2026.4.1 added **async observability writes** to cut latency — implying observability writes previously sat on the hot path.

## 4. Observability

- All inference traces + feedback + downstream metrics stored **in your own ClickHouse** (or Postgres backend) — "your data never leaves your infra" is a core differentiator vs OpenRouter/Portkey SaaS.
- Structured data model (documented "Data Model" page): inferences, model inferences, episodes, feedback, datasets/datapoints — designed for SQL access, not just UI.
- Query historical inferences via UI and programmatically; replay historical inferences with new prompts/params; build datasets from production traffic for optimization/evals.
- **OpenTelemetry (OTLP) trace export**; **Prometheus metrics export** (incl. token tracking + prompt-caching statistics, 2026.4.0).
- Planned (roadmap): AI-assisted debugging/root-cause analysis, data labeling.

## 5. The Flywheel: Optimization, Evals, Experimentation (their moat)

- **Feedback API:** metrics (boolean/float), demonstrations, comments — attachable at **inference or episode level**. Episodes = first-class multi-step workflow grouping, enabling end-to-end credit assignment for compound/agentic systems. This is the structural bet competitors lack.
- **Optimization recipes:** Supervised fine-tuning (OpenAI/Fireworks/Together/GCP), RLHF, **GEPA** automated prompt engineering (2026.3.x), **Dynamic In-Context Learning (DICL)**, inference-time optimizations (best-of-N, mixture-of-N sampling), planned synthetic data generation. All driven by the data the gateway already collected.
- **Evaluations:** inference evaluations (heuristics + LLM judges, "unit tests"), workflow evaluations ("integration tests", API-driven), LLM-judge alignment to human preferences, CLI + UI runners, TypeScript evaluators (2026.4.1).
- **Experimentation:** **adaptive A/B tests** (sequential testing) + static A/B tests, variant traffic-splitting at the gateway, episode-consistent variant assignment for multi-turn systems, **experiment namespaces** for scoping. `weight`-based variant config being replaced by richer experimentation semantics in 2026.6+.
- **TensorZero Autopilot (paid, private beta, launched 2026.4):** "automated AI engineer" that consumes your observability data, sets up evals, optimizes prompts/models, runs A/B tests; demo claim: halved errors in <5 min by analyzing hundreds of traces. Self-serve launch planned. This is the monetization layer on top of OSS.

## 6. UI / Dashboard

Separate Node/TypeScript **TensorZero UI** (own deploy): observability browsing (per-inference + aggregates), playground ("try with variant", outputs collectable as demonstrations), dataset management, supervised fine-tuning workflows, evaluation visualization, settings/API-key management. The UI is an optional add-on to the gateway, not a single binary with it.

## 7. Agent/AX (Agent Experience) Surface

- **MCP server built into the gateway at `/mcp` (2026.4.0)** — exposes the TensorZero API over MCP so agents can drive the gateway. Early/minimal, but it exists.
- `for-agents/plugins/tensorzero` directory in the repo — agent-facing plugin packaging (e.g., for coding agents).
- **`llms.txt` full docs index** published for LLM consumption.
- Config-as-TOML + "manage thousands of prompts/LLMs entirely programmatically" + direct SQL access to all data = a fairly machine-operable surface, though there is **no admin REST API or CLI control plane** comparable to LiteLLM's admin API; ops are file/config + DB driven.
- Episode/feedback model is explicitly designed for **agentic workflows** (multi-step credit assignment); Autopilot is itself an agent operating the platform.
- Evaluations have a CLI; headless evaluations are on the roadmap.

## 8. Extensibility

- Philosophy: **escape hatches, not plugins.** `extra_body` (override provider payload at variant/provider/inference level), `extra_headers`, `include_raw_response`, dynamic credentials, direct SQL on your own DB. No plugin framework, no custom-provider SDK, no middleware/hook system, no guardrails integration framework (conceded vs LiteLLM/Kong).
- OpenAI-compatible generic provider is the de-facto "custom provider" mechanism.

## 9. Pricing

- OSS: free, BYO keys, self-hosted, no usage fee (vs OpenRouter's 5% BYOK fee).
- Autopilot: paid, private beta, pricing unpublished.
- Future managed enterprise service planned; none today (conceded vs LiteLLM's hosted option).

## 10. Weaknesses & Complaints (observed + conceded)

1. **Steep learning curve / heavy conceptual model** — functions/variants/episodes/TOML schemas must be adopted before value; third-party roundups consistently flag "learning curve is steep; most teams don't need ML-optimized routing."
2. **Infra weight:** real deployments want ClickHouse + Postgres + Valkey + gateway + separate UI container — not a single binary; "single Docker container" is only the no-observability degenerate case.
3. **No dynamic routing** (cost/latency/rate-limit aware), no request prioritization/queuing, no budgets — self-admitted vs LiteLLM.
4. **No guardrails/policy integrations** (PII, content moderation) — manual.
5. **Fewer native providers (~20)** than LiteLLM (100+); relies on OpenAI-compatible catch-all.
6. **No managed/hosted option** — self-host or nothing (until Autopilot/enterprise ships).
7. **Config-file-centric control plane** — no full admin API/CLI for runtime mutation (keys/limits arrived only in 2026 and are still maturing); prompt changes are config deploys.
8. **Benchmark skepticism** — HN questioned the <1ms p99 claim; LiteLLM published a competing sub-ms claim; numbers use a mock provider.
9. **Auth/rate-limiting/cost-tracking are late additions (2026)** — historically the biggest gap vs Portkey/Kong/LiteLLM enterprise features; per-user virtual-key spend governance is thinner than LiteLLM's.
10. **2026.6.0 shipped a fix for a "high-risk security vulnerability"** in the gateway — recent, worth tracking.
11. UI is a separate service, OpenAI-compat endpoint historically lagged native API features (Responses API reasoning fixes still landing through 2026.5).
12. Low community-noise footprint: Show HN got 2 comments; mindshare trails LiteLLM/OpenRouter despite funding.

## 11. What to Steal (best-in-class ideas)

1. **Episode abstraction** — first-class multi-step workflow ID with feedback at inference OR episode level; the cleanest end-to-end credit-assignment primitive in the category.
2. **Functions/variants indirection** — app calls a named function; prompt/model/params live in the gateway → gateway-level A/B testing, fallback across *prompts* not just providers, and zero-code-change optimization.
3. **Adaptive (sequential) A/B testing at the gateway** with episode-consistent assignment — nobody else has experimentation as a gateway primitive.
4. **Observability in the customer's own DB** (ClickHouse) with a documented schema and direct SQL as a supported interface — sovereignty + analyst-friendly.
5. **Feedback API as a gateway endpoint** (metrics/demonstrations/comments) feeding fine-tuning/DICL/prompt-optimization recipes — the flywheel.
6. **Inference-time optimizations** as config: best-of-N, mixture-of-N, DICL.
7. **Escape hatches** (`extra_body`/`extra_headers`/raw response/dynamic per-request credentials) — pragmatic alternative to a plugin treadmill.
8. **MCP server at `/mcp` exposing the gateway's own API** to agents (their 2026.4.0 move) and `llms.txt` docs index.
9. **Relay-gateway topology** for org-wide auth/limits/credentials over team-owned edge gateways.
10. **Published, reproducible benchmark methodology** vs the incumbent (LiteLLM) as marketing.
11. **Autopilot** — an agent that operates the platform itself (evals, prompt optimization, A/B tests) as the paid layer over OSS.
12. **Replay historical inferences with new params** and "collect playground outputs as demonstrations" loops.

## 12. Key Sources

- https://github.com/tensorzero/tensorzero (README, releases)
- https://www.tensorzero.com/docs (+ /docs/llms.txt full index)
- https://www.tensorzero.com/docs/gateway/benchmarks
- https://www.tensorzero.com/docs/comparison/litellm , /comparison/openrouter
- https://www.tensorzero.com/docs/operations/* (auth, rate limits, extend, credentials)
- https://www.tensorzero.com/blog/automated-ai-engineer/ (Autopilot)
- https://news.ycombinator.com/item?id=41557020 (Show HN)
- https://www.tensorzero.com/blog/tensorzero-raises-7-3m-seed-round-to-build-an-open-source-stack-for-industrial-grade-llm-applications/
- Third-party roundups: techsy.io, getmaxim.ai, agenta.ai gateway comparisons
