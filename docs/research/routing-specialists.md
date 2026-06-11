# Competitive Intelligence: Model-Routing Specialists — Martian, Not Diamond, Unify

**Category:** LLM gateway / learned model routing ("pick the best model per prompt")
**Researched:** 2026-06-10
**Relevance to us:** These three are the "routing intelligence" specialists. None of them is a full open-source gateway; all three treat routing as the product. One (Unify) already abandoned the market — a cautionary tale. The lesson set here is mostly about what routing intelligence looks like at SOTA, what routing APIs look like, and where pure-play routing fails as a business/product.

---

## 1. Martian (withmartian.com)

### Status & positioning (as of mid-2026)
- Founded 2022 (Shriyash Upadhyay & Etan Ginsberg, ex-UPenn LLM researchers). Invented the term "LLM router."
- Funding: ~$32M total; $9M seed led by NEA (Prosus, General Catalyst, Carya); later round including **Accenture Ventures** (2025). An April 2026 Medium report claims Martian is "nearing a $1.3B valuation" (unverified, single source).
- Notably repositioned: withmartian.com now presents as a **research lab** ("a team of researchers... understanding machine intelligence") with three pillars — Measurement (ARES, Code Review Bench, mech-interp), Explanation (feature geometry, K-Steering), Application (commercial products). The router/gateway product lives on separate subdomains (docs.withmartian.com, gateway-docs.withmartian.com, route.withmartian.com, work.withmartian.com).
- Core thesis: **"Expert Orchestration Architecture"** — "judge" models evaluate expert-model capabilities; "router" systems direct queries to the most trustworthy expert based on those evaluations. Routing framed as an interpretability problem ("model mapping": predict a model's behavior on a prompt without running it).
- Claimed usage: engineers at 300+ companies incl. Amazon and Zapier; powers **Accenture's "AI Switchboard"** multi-LLM platform servicing >$1B of Accenture GenAI deployments.

### Product surface
**Martian Gateway** (api.withmartian.com)
- Drop-in, OpenAI-compatible endpoint: `https://api.withmartian.com/v1/chat/completions`; also **Anthropic API compatibility** (dual-protocol — notable, most gateways are OpenAI-only).
- Unified access to **200+ models** from OpenAI, Anthropic, Google, etc.; pick model via the `model` field or let the router decide.
- **Smart routing controls**: specify **max cost per request** and a **cost-vs-quality tradeoff ("willingness to pay")** — the router promises Pareto-frontier performance at your cost preference.
- Automatic **failover across providers** (uptime/reliability routing — route around provider outages).
- Benchmarking reports for models; "Available Models" catalog in API reference.
- Auth: bearer API key, managed in the Martian Dashboard.
- Dashboard: real-time usage tracking, request history, model performance metrics.
- **Integrations explicitly documented for agents/coding tools**: HTTP client, OpenAI SDK, Anthropic SDK, Vercel AI SDK, **Aider, Claude Code, Cline, Codex (CLI), Cursor, LiteLLM, OpenCode**. They treat agent harnesses as first-class gateway consumers.
- **Airlock** (launched with Accenture partnership): **LLM compliance automation** — companies define policies and automatically vet/approve models for use in their applications (model governance layer on top of the router).

### Pricing
- Free developer tier: 2,500 requests; then **$20 per additional 5,000 requests** (routing fee), plus model usage billed at model rates.
- Enterprise: custom router trained on your data/tasks, SLA, **VPC deployment**, dedicated support.

### Open source
- The platform is **closed source**. Public GitHub (github.com/withmartian, 28 repos): `routerbench` (the RouterBench paper code/dataset — 405k+ inference outcomes, the de-facto academic standard for router evaluation), `deimos-router` (a rule-based configurable LLM routing system, Python), `ares` (agentic research/eval suite, MIT, 97★), `code-review-benchmark` (175★), a fork of **TensorZero** (Rust gateway), `martian-ai-sdk-provider` (JS). Research-heavy, product-light.

### Performance claims
- "Cuts costs 20%–97% while beating GPT-4 on key benchmarks" (original launch claim, judged against OpenAI's own evals).
- RouterBench paper (arXiv 2403.12031) is theirs — they literally wrote the benchmark the field uses.

### Weaknesses / complaints
- Long-standing HN skepticism of the category: "people tune prompts to specific models; people who switch models all the time aren't building serious AI apps."
- Product story is fragmented across 4+ subdomains; main site no longer even mentions the router — confusing buyer journey, signals a pivot toward research/enterprise-services.
- No published routing-latency numbers; no self-serve custom router (enterprise-only).
- Closed source in a category where RouteLLM/vLLM Semantic Router/RoRF give the core capability away free.

---

## 2. Not Diamond (notdiamond.ai)

### Status & positioning (as of mid-2026)
- The most alive pure-play router. Repositioned from "route every chat query" to **"Model Routing for Coding Agents"** — the wedge is the agent cost problem (agents multiply inference spend).
- Architecture stance worth noting: Not Diamond **does not want to be your gateway anymore** — the router sits *between your prompt harness and your existing AI gateway*, returns a recommendation, and execution happens in **your** infrastructure. (The old hosted "model gateway" docs page now 404s.) **OpenRouter is listed as a customer** — i.e., the gateway companies themselves buy the routing intelligence.
- Customers cited: Hugging Face, Dropbox, IBM, OpenRouter, Replicated. SOC-2 and ISO 27001 certified.

### Product surface
**1) Model routing API ("meta-LLM")**
- `select_model()` call: send messages + a candidate list of LLM configs → returns `session_id` + chosen provider/model; you then call the model yourself with your own SDK/keys. Clean separation of *recommendation* from *execution* (prompt privacy + no key custody).
- **Pre-trained routers**: one for **chat** (cross-domain) and one for **code** (optimize coding-agent cost). Start in <5 minutes.
- **Custom routers**: train on your own data — CSV of {input, candidate-model responses, evaluation scores} (any eval metric, `maximize` flag), 15 samples minimum, up to 10k samples/5MB per job; training takes minutes→1 hour; returns a `preference_id` used in subsequent `select_model()` calls; retrain/update with `override=True`. Can route to **arbitrary custom targets** — fine-tunes, agentic workflows, any inference endpoint.
- **Tradeoff controls**: `tradeoff="cost"|"latency"` (default quality-first), plus a continuous **`cost_quality_tradeoff` 0–10 dial** (Pareto optimization across quality/cost/latency).
- **Routing latency: 10–100 ms** (published; sub-100ms inference confirmed by founder on HN).
- Session IDs enable feedback/debugging of routing decisions.
- SDKs: Python (`pip install notdiamond`), TypeScript (`npm install notdiamond`), REST. SDKs are open source (MIT).

**2) Prompt optimization / "Prompt Adaptation" (GA 2025)**
- Design-time agentic system: give it a static prompt + ≥3 validated input/output examples + eval metric + target models; it rewrites/tests thousands of prompt variants per target model (RL-guided loop), returns a **per-model optimized prompt** + accuracy report; runs 5–25 min, deployable in ~30 min. Directly attacks the "prompts are tuned to one model" objection to routing — adapt the prompt per model, then route freely.

**3) Supported models**: OpenAI (incl. GPT-5 series), Anthropic (Opus/Sonnet/Haiku), Google Gemini 2.x + Gemma, Mistral/Codestral, xAI Grok, DeepSeek, Qwen, Perplexity, Cohere, Together, Replicate, Inception — with a capability table for function-calling and structured-outputs support per model.

**4) Open source**: `Not-Diamond/RoRF` — **Routing on Random Forests** (MIT): pairwise routers over query embeddings (Jina/Voyage/OpenAI embeddings), 12 pre-trained pairs (e.g., Llama-3.1-405B vs GPT-4o), threshold parameter controls traffic split; claims routing between two *strong* models can outperform both. Also maintains `awesome-ai-model-routing`.

### Pricing
- Pay-as-you-go: **10K free routing recommendations/month, then $10 per 10K**; 3 free custom routers; prompt optimization 10 free/month then **$20 per successful optimization** (4 target models per run).
- (Older published tiering: free to 100K requests/mo + 1 custom router; $100/mo plan with $0.001/request overage — pricing has shifted toward lower free volume.)
- Enterprise ("Necessity"): VPC deployment, bring-your-own-models, custom eval metrics, agent optimization, priority queue, custom zero-day model policies, 24/7 support. Also on AWS Marketplace.

### Performance claims
- For coding agents: **+5% accuracy, −30% cost, 2× faster development cycles**.
- Launch claim: "beats every foundation model on every major benchmark" via routing (HN 41108787).
- Counterpoint — **RouterArena (arXiv 2510.00202)** independently ranked Not Diamond #12 overall: high accuracy but **frequently selects expensive models**, losing on cost-adjusted metrics to open-source routers.

### Weaknesses / complaints
- RouterArena finding: over-reliance on strongest/most-expensive models; poor at recognizing when a cheap model suffices.
- No multimodal/vision routing (founder: "on the roadmap") as of launch-era HN thread.
- Recommendation-only architecture means an extra network hop per request (10–100 ms) and you still need a separate gateway for execution, key management, retries, logging.
- Custom routers require you to already have eval data with per-model scores — a heavy lift most teams haven't done.
- Per-recommendation pricing ($1/1K) is hard to justify vs. free heuristic routing for high-volume agent traffic.

---

## 3. Unify (unify.ai) — DEAD as a router; cautionary tale

### What happened
- YC-backed (W23-era), $8M raised (TechCrunch, May 2024) to "help developers find the best LLM for the job."
- **Pivoted entirely**: unify.ai now sells "Droid" — AI teammates/workers for business tasks (40+ tool integrations, voice/chat/email, 0.9s median response). The LLM router/unified-API business is abandoned. (Don't confuse with unifygtm.com, a separate sales-tech company.)

### What the router was (for the record — some ideas worth keeping)
- Single API/key over OpenAI, Anthropic, Google, Mistral, Cohere, Meta/Llama endpoints across multiple inference providers; OpenAI-compatible; Python SDK was open source (`unifyai/unify`).
- **Dynamic per-prompt routing** with explicit user-set weights over **quality, cost, and speed** — you tuned the objective function, not just a binary tradeoff flag.
- **Neural scoring function** predicted each model's response quality for a given prompt *before* the call (trained with GPT-4-as-judge over their benchmark corpus).
- **Public, daily-refreshed benchmarks** of every model/provider's runtime (throughput, TTFT, latency) and quality — transparency as the differentiator vs. black-box routers; benchmark on *your own prompts* in their dashboard.
- **Custom router training on your data** for additional performance over the pre-trained router.
- Load balancing, fallback across providers, cost-control tools.

### Lesson
- The most architecturally transparent of the three (public benchmark data, open SDK, tunable objective) still couldn't make standalone routing a business. Routing is a feature; the gateway (traffic, keys, governance, observability) is the product. Both survivors moved up-stack: Martian → enterprise compliance/orchestration with Accenture; Not Diamond → coding-agent cost optimization sold *to* gateways.

---

## 4. What SOTA routing intelligence looks like (2025–2026)

Techniques (from RouterBench/RouterArena/awesome-list literature and these vendors):
- **Predictive scoring ("model mapping")**: train a model to predict per-model quality on a prompt without running it (Martian's model mapping; Unify's neural scoring function). The frontier framing: routing = interpretability.
- **Embedding classifiers**: query embedding → random forest / kNN / matrix factorization picks model (Not Diamond RoRF, EmbedLLM). Cheap, ~ms latency, MIT-licensed and replicable.
- **Preference-data routers**: RouteLLM (LMSYS) — strong/weak binary router trained on Chatbot-Arena preference data with augmentation; threshold dial = cost/quality knob.
- **Cascades**: FrugalGPT — try cheap model, escalate on low confidence.
- **Domain-policy routing**: **Arch-Router** (Katanemo, arXiv 2506.16655) — 1.5B model maps queries to human-written domain/action policies ("code generation → model X"); SOTA at matching human preferences; routing decisions are *explainable and operator-controlled* rather than learned-opaque. This is the most agent-gateway-compatible approach.
- **GNN / contrastive / IRT routers**: GraphRouter, RouterDC, IRT-Router — academic, cost-efficient.
- **Built-in-product routers**: GPT-5's internal router, Azure Model Router — routing is being absorbed into model products themselves.

Benchmarks/metrics to care about (RouterArena's 5 dimensions): answer accuracy, answer cost, routing optimality (vs. oracle), routing robustness under noisy input, routing latency overhead.

Key empirical findings (RouterArena, Oct 2025):
- **Every router underperforms the oracle**, mainly by failing to recognize when small/cheap models suffice.
- Commercial routers (Not Diamond, GPT-5, Azure) buy accuracy with cost; open-source CARROT and vLLM Semantic Router hit **~35% cost reduction at <2% accuracy loss** — the practical efficiency frontier is available for free.
- Latent (embedding) representations are more robust to prompt noise than explicit feature representations.

---

## 5. Agent-experience (AX) observations

- **Martian** is the only one of the three documenting agent harnesses as first-class gateway clients: Claude Code, Cursor, Cline, Aider, Codex, OpenCode integration pages. A coding agent can point its base URL at Martian and get routed + failover + cost caps with zero code. Dual OpenAI+Anthropic protocol compatibility matters specifically for Claude Code-style agents.
- **Not Diamond** redefined the integration point for agents: the router is a *sidecar recommendation API* for the harness/gateway, not a proxy. This is the right shape for an agent-first control plane — the agent (or gateway) asks "which model for this step?" and keeps execution local. Their entire 2026 positioning is per-step routing inside agent loops (different models for plan vs. edit vs. test steps).
- **None of the three has MCP support, an agent-facing CLI, or a machine-readable config/control plane.** Configuration is dashboard + API key + REST. No declarative config files, no GitOps story, no MCP server to let an agent inspect/modify routing policy. This is open space.
- Routing-latency budget is the AX constraint: Not Diamond publishes 10–100 ms per recommendation; for multi-step agents that hop is paid per LLM call.

## 6. Implications for our gateway (synthesis hooks)

1. Routing should be a **pluggable policy inside the gateway**, not a separate paid hop — the open-source frontier (RouteLLM, RoRF, Arch-Router, CARROT, vLLM Semantic Router) is good enough to embed, all MIT/Apache.
2. The two routing knobs users actually understand: a **cost/quality dial** (continuous, ND's 0–10) and a **max-cost-per-request cap** (Martian). Ship both.
3. **Arch-Router-style policy routing** (human-readable domain→model rules, optionally backed by a small classifier) beats opaque learned routing for trust + agent control; pair with optional learned routing behind a flag.
4. **Per-model prompt adaptation** (ND's Prompt Adaptation) is the answer to "my prompts are tuned to one model" — the #1 objection to routing.
5. **Failover/uptime routing** is the routing feature with undisputed value (HN consensus: cost routing is debated, reliability routing is not).
6. Custom-router training UX: CSV of {prompt, score-per-model} + one train call + a `preference_id` — copy this shape if we ever offer learned routing.
7. Governance angle is monetizable: Martian's Airlock (policy-based model approval/compliance) is what Accenture paid for — fits an enterprise dashboard.
8. Watch the graveyard: Unify died doing exactly "unified API + router + benchmarks" standalone. Routing only survives attached to a bigger surface (gateway, agents, compliance).

---

## Sources
- https://withmartian.com/ , https://docs.withmartian.com/gateway , https://gateway-docs.withmartian.com/ , https://withmartian.com/post/martian-partners-with-accenture-launches-airlock-compliance-for-enterprises , https://github.com/withmartian , https://venturebeat.com/ai/why-accenture-and-martian-see-model-routing-as-key-to-enterprise-ai-success , https://techcrunch.com/2023/11/15/martians-tool-automatically-switches-between-llms-to-reduce-costs/ , https://medium.com/@sarawgiapoorvwork347/martian-the-san-francisco-based-startup-that-invented-the-first-llm-router-is-reportedly-nearing-4211dd768296 , https://finance.yahoo.com/news/martian-invents-model-router-beats-190000381.html
- https://www.notdiamond.ai/ , https://www.notdiamond.ai/pricing , https://docs.notdiamond.ai/docs/what-is-not-diamond , https://docs.notdiamond.ai/docs/quickstart-routing , https://docs.notdiamond.ai/docs/router-training-quickstart , https://docs.notdiamond.ai/docs/key-concepts , https://docs.notdiamond.ai/docs/llm-models , https://github.com/Not-Diamond/RoRF , https://github.com/Not-Diamond/awesome-ai-model-routing , https://www.notdiamond.ai/blog/prompt-optimization-is-now-generally-available , https://news.ycombinator.com/item?id=41108787
- https://unify.ai/ , https://techcrunch.com/2024/05/22/unify-helps-developers-find-the-best-llm-for-the-job/ , https://dev.to/danlenton/we-built-a-dynamic-router-improving-llm-quality-cost-and-speed-4dlf , https://www.samsungnext.com/blog/why-we-invested-in-unify
- https://arxiv.org/html/2510.00202v1 (RouterArena) , https://arxiv.org/abs/2403.12031 (RouterBench) , https://arxiv.org/abs/2506.16655 (Arch-Router) , https://arxiv.org/pdf/2406.18665 (RouteLLM) , https://news.ycombinator.com/item?id=42340287 , https://news.ycombinator.com/item?id=40450539
