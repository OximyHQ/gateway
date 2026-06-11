# Dimension deep-dive: Prompt management & experimentation

**For:** new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane)
**Question:** prompt templates/versioning/deployment labels, A/B tests, experiments, evals integration, dataset capture from traffic, replay — should a gateway own this or integrate?
**Researched:** 2026-06-10 (web search + official docs)

---

## 1. Market map — who owns prompt management today

Two distinct camps, with the boundary actively blurring:

**LLMOps / observability platforms that own prompt management as a core product:**
- **Langfuse** (OSS, MIT core) — the open-source reference implementation; prompts + traces + evals + datasets in one product.
- **LangSmith** (closed, LangChain) — Prompt Hub with commit hashes + movable tags.
- **PromptLayer** (closed SaaS) — the most prompt-centric product; Prompt Registry + dynamic release labels for traffic-split A/B.
- **Braintrust** (closed SaaS, free tier) — eval-first; playground/datasets/scorers/experiments unified, "Loop" AI assistant that rewrites prompts and generates datasets/scorers.
- **Agenta** (OSS, MIT) — prompt management + evals + observability, explicitly built for dev + non-dev collaboration.
- **Promptfoo** (OSS, MIT) — CLI/CI eval + red-teaming harness; declarative YAML, no hosted registry needed.
- **Humanloop** — the category pioneer; **acqui-hired by Anthropic Aug 2025, platform shut down Sept 8, 2025** (no IP acquired; customers force-migrated to W&B/Agenta/PromptLayer). Strong market signal: standalone prompt-management vendors are consolidation targets, and customers got burned by a closed SaaS disappearing.

**Gateways:**
- **Portkey** — the gateway that most fully owns prompt management ("Prompt Engineering Studio"): templates, partials, versioning, labels, playground, prompt API. Widely cited as its standout differentiator vs LiteLLM.
- **Helicone** — rebuilt prompts (old Experiments feature deprecated Sept 1, 2025; old prompt-formatter package deprecated) so that prompts now live **inside the AI gateway request path**: `prompt_id` + `inputs` in a chat-completion call; the gateway compiles and forwards.
- **LiteLLM** — deliberately does NOT own a prompt store; it owns the **integration seam**: `prompt_id` in the request resolves through pluggable backends (dotprompt files, Langfuse, BitBucket, GitLab, generic HTTP API, custom hooks).
- **Cloudflare AI Gateway** — no prompt templates, but owns **dataset capture from traffic + evaluations** (datasets = saved log filters; evals score cost/speed/performance over them) and Dynamic Routing (visual flow: conditions, quotas, model choice, % splits).
- **Kong AI Gateway** — config-level plugins only: `ai-prompt-template` (centrally managed fill-in-the-blank templates, enforce approved templates), `ai-prompt-decorator` (inject system/steering messages invisible to users), `ai-prompt-guard` (regex allow/deny). No versioning/labels/experiments product.
- **Bifrost (Maxim), OpenRouter** — no built-in prompt management; Maxim sells it as a separate platform; TrueFoundry bundles prompt management with its gateway as an enterprise suite.

**Pattern:** every serious gateway has converged on at least a `prompt_id`-in-the-request mechanism; full experimentation (evals, experiments, scorers) remains the territory of LLMOps platforms, and gateways that tried to own experimentation natively (Helicone Experiments) retreated.

---

## 2. Feature taxonomy — full surface observed across products

### 2.1 Prompt templates & storage
- Central registry/library decoupled from code; folders + access controls (Portkey, PromptLayer, Langfuse).
- Variables: mustache `{{var}}` (Portkey), typed variables `{{hc:name:type}}` string/number/boolean/custom usable in messages **and tool schemas / JSON response schemas** (Helicone — dynamic schema generation is notable), `{variable}` (LiteLLM generic).
- **Prompt partials / composability**: reusable sub-templates (instruction sets, schemas, few-shot examples) with their own version+publish lifecycle (Portkey Prompt Partials; Helicone partials via `{{hcp:prompt_id:index:environment}}`; Langfuse composability).
- Model config travels with the prompt (model, params, tools) — LangSmith commits capture prompt + variables + model config; LiteLLM `.prompt` files carry model + params.
- Prompts-as-code option: dotprompt files in repo, BitBucket/GitLab-backed prompts, GitOps flow (LiteLLM native GitOps; promptfoo YAML).
- Multimodal templates (images in prompts) — Portkey playground.

### 2.2 Versioning & deployment labels (the universal core)
- Immutable auto-incrementing versions on every save (Langfuse v1,2,3…; LangSmith commit hashes; Helicone commit histories with who/when).
- **Update vs Publish** distinction (Portkey): edits create versions; publish promotes the default served version.
- **Labels/tags as movable pointers**: Langfuse labels (code pins a label, not a version; protected labels = only admins can move — governance feature), Portkey default labels `production/staging/development` (non-removable) + custom labels, PromptLayer release labels, LangSmith commit tags, Helicone environments (production/staging/dev/custom; request can pass `environment`).
- Instant rollback without redeploy; fetch-by-version or fetch-by-label APIs.
- Webhooks on prompt change (Langfuse) for CI/CD or cache invalidation.

### 2.3 Serving & runtime
- **Client-side SDK fetch with caching + fallback prompt** (Langfuse pattern: cached client-side, zero-latency after first fetch; fallback if service unreachable).
- **Gateway-side resolution** (Helicone/LiteLLM/Portkey pattern): request carries `prompt_id` + `inputs`/`prompt_variables`; gateway fetches/compiles template, replaces `messages` entirely, forwards to provider. Dashboard edits take effect immediately with no app deploy.
- Render-only endpoint (Portkey Render API: get the compiled prompt without executing) — useful for clients that want to call providers themselves.
- Runtime dependency risk is the canonical complaint: dynamic fetch adds an external networked service to the hot path (mitigations: SDK caching, fallbacks, gateway-local store).

### 2.4 Playground
- Side-by-side multi-model comparison, params, token/cost display (everyone: Langfuse, Portkey, LangSmith, Braintrust, Helicone).
- Load real production traces into the playground / replay a prompt version against historical inputs before rollout (Braintrust, Langfuse, LangSmith).
- Tool/function definition testing in playground (Portkey, Helicone).
- Real-time collaborative playgrounds, shareable URLs (Braintrust).
- AI-assisted prompt improvement in the editor (Portkey AI-assist; Braintrust Loop generates better prompt versions + datasets + scorers).

### 2.5 A/B testing & experiments
- **Label-based A/B** (Langfuse: label variants `prod-a`/`prod-b`, app splits traffic, compare via linked traces) — simple, app does the routing.
- **Traffic-split release labels** (PromptLayer Dynamic Release Labels: overload "prod" with % splits or user-segment routing — the most complete in-product A/B implementation; gateway-shaped feature implemented in an LLMOps tool).
- **Offline experiments**: run prompt version × model matrix against a dataset, score, diff side-by-side (Braintrust experiments = immutable snapshots promoted from playground; Langfuse prompt experiments; LangSmith experiments; Agenta).
- Regression gating: auto-run regression tests / eval pipelines when a new version is created (PromptLayer backtesting; promptfoo in CI).
- **Cautionary tale:** Helicone built a full Experiments product (test prompt changes against production data) and **deprecated it (removed Sept 1, 2025)** — heavy experimentation UIs inside a gateway/observability product didn't earn their keep; they rebuilt around lightweight prompts-in-gateway instead.

### 2.6 Evals integration
- LLM-as-judge, heuristic/custom code scorers (Python/TS), human annotation queues (Langfuse — all MIT-licensed since the June 2025 "Doubling Down on Open Source" re-licensing; Braintrust scorers; Helicone evaluators).
- Evals run on production traffic (online) or on datasets (offline experiments).
- Trace↔prompt-version linking so production metrics (cost, latency, scores) slice by prompt version (Langfuse, Portkey prompt observability, Helicone).
- Gateways generally **integrate** here rather than own: LiteLLM → Langfuse/Braintrust/Arize via callbacks/OTel; Kong → external; Cloudflare is the exception with native-but-shallow evals (cost/speed/perf averages over datasets — not LLM-as-judge grade).

### 2.7 Dataset capture from traffic & replay
- **Datasets from logs by filter** (Cloudflare: dataset = saved log filter, logs flow in continuously; Helicone: add requests to datasets).
- **Add trace → dataset item** one click / API (Langfuse, LangSmith, Braintrust "convert production traces back into test cases").
- Replay: re-run dataset/historical inputs against new prompt/model in playground or experiment (Braintrust, Langfuse, LangSmith); this is the loop that makes traffic capture valuable.
- This is the area where **the gateway has a structural advantage**: it already sees every request/response; capture is free; the eval loop on top is the optional integration.

### 2.8 Governance around prompts
- Protected labels (Langfuse: only admin/owner can move `production`).
- RBAC on prompt library/folders (Portkey).
- Audit: who changed what when (Helicone, Langfuse versions are immutable history).
- Enforced approved templates at the edge (Kong `ai-prompt-template`: clients may only use sanctioned templates — a compliance angle unique to gateway placement).
- Prompt firewalling adjacent: Kong `ai-prompt-guard` regex allow/deny, decorator-injected hidden system prompts.

### 2.9 Agent experience (AX) — how AGENTS use these systems
- **Langfuse is the AX leader**: native hosted **MCP server built into the product** at `/api/public/mcp` (streamableHttp, Nov 2025) for managing prompt versions, labels, datasets, annotation queues, scores; plus an Agent Skill + CLI for Claude Code/Codex/Cursor; "every feature has an endpoint" — agents can read traces, write scores, manage prompts, run dataset experiments programmatically.
- LiteLLM's generic prompt-management API (`GET /beta/litellm_prompt_management`) = a minimal contract any system (or agent-maintained store) can implement; auto-detection of the backing system from `prompt_id` so callers just pass `model` + `prompt_id`.
- Prompts-as-code (dotprompt files, GitLab/BitBucket-backed) is the most agent-native storage: coding agents already excel at editing files + opening PRs; review/diff/rollback come from git for free.
- Braintrust Loop points the other direction: the platform's own agent writes prompts/datasets/scorers — prompt optimization is becoming an agent task, which favors machine-readable prompt stores with full CRUD APIs.

---

## 3. Product snapshots (condensed)

| Product | Prompt templates | Versions/labels | A/B | Experiments/evals | Dataset-from-traffic | Where it runs | OSS |
|---|---|---|---|---|---|---|---|
| Langfuse | Yes + composability | Versions + labels + protected labels + webhooks | Label-based (app-side split) | Prompt experiments, LLM-judge, annotation — all MIT now | Trace→dataset | SDK fetch (cached) or via LiteLLM | MIT core (EE = SCIM/audit/retention) |
| Portkey | Templates + partials + multimodal | Update/Publish + 3 default + custom labels | Via labels | Evals integrations; prompt observability | Logs linked to prompt versions | Gateway-native (prompt API + render) | Gateway OSS; prompt studio is paid SaaS |
| Helicone | Templates + typed vars in tool schemas + partials | Commit history + environments | — | Old Experiments **deprecated 9/2025**; evaluators remain | Requests→datasets | Gateway-native (`prompt_id`+`inputs` compile in gateway) | OSS platform (Apache-2.0 repo); prompts paid-tier gated (free = public prompts only) |
| LiteLLM | No store — pluggable | `prompt_version` param; backend-dependent | — | Via Langfuse etc. callbacks | Via integrations | Gateway resolves `prompt_id` via 5+ backends | MIT core / enterprise features |
| PromptLayer | Registry, visual editor | Release labels | **Dynamic release labels: % traffic split, user segments** | Eval pipelines, backtesting, regression on new version | Request history → datasets | SDK/API fetch | Closed SaaS |
| LangSmith | Prompt Hub + public community hub | Commit hashes + movable tags | — | Datasets + experiments + evaluators | Trace→dataset | SDK fetch | Closed (self-host = enterprise) |
| Braintrust | Prompts inside playground | Experiment = immutable snapshot | Playground diff | Best-in-class loop + **Loop AI assistant** | Traces→test cases automatic | SDK/proxy | Closed SaaS (free tier) |
| Agenta | Yes, non-dev friendly UI | Environments | — | Integrated evals | Yes | SDK/API | MIT |
| Promptfoo | YAML configs in repo | git | — | CLI evals + red-team, CI/CD | Import logs | Local CLI | MIT |
| Cloudflare AI GW | No templates | — | Dynamic Routing % splits | Native shallow evals (cost/speed) | **Datasets = saved log filters** | Edge gateway | Closed |
| Kong AI GW | `ai-prompt-template` plugin (enforced approved templates) | Config-versioned only | — | — | — | Gateway plugin config | OSS plugins |
| Bifrost / OpenRouter | None | — | — | — | — | — | Bifrost Apache-2.0 |

Pricing notes: Helicone free 10K req/mo, Pro $79/mo, Team $799/mo (free-tier prompts are public-only — a complained-about upsell); Langfuse generous OSS self-host + cloud tiers; PromptLayer per-seat SaaS; Braintrust free tier then usage-based; Portkey prompt studio on paid plans.

Performance: no vendor publishes hard numbers for prompt-resolution latency. Langfuse's answer is client-side SDK caching (≈0 added latency after first fetch + fallback prompts); Helicone/Portkey compile in-gateway (claimed negligible, unquantified). No published A/B-at-the-edge latency benchmarks found.

---

## 4. Complaints & failure modes observed

1. **Runtime dependency dread** — fetching prompts from a networked control plane puts a third-party in the request path; teams demand caching + fallback + the ability to pin versions in code (recurring in Reddit/HN threads and vendor-comparison posts).
2. **Vendor mortality** — Humanloop's shutdown stranded prompt workflows, evals, and logs with a 4-week migration window. Strong argument for OSS + exportable formats (dotprompt/YAML) as a differentiator.
3. **Feature churn in gateways** — Helicone deprecated Experiments AND its first prompts package; users had to migrate twice. Lesson: ship the durable thin layer (versioned templates + labels + id resolution), not a heavyweight experimentation UI you'll abandon.
4. **A/B testing is mostly DIY** — only PromptLayer does true managed traffic-splitting; Langfuse's "A/B" is just two labels and app-side coin-flip; users on GitHub discussions ask for faster variant workflows (langfuse#11868). A gateway can do % splits natively in the routing layer — clear gap.
5. **LiteLLM prompt integrations are rough**: Langfuse prompt management only works with global credentials (no team-level, issue #20872); reported breakages (#12097); config-loaded prompts immutable without restart.
6. **Pricing gates**: Helicone private prompts paywalled on free tier; LangSmith pricing confusion (trace retention mapping); Langfuse self-hosting infra burden (ClickHouse+Redis+Postgres+S3) cited by teams that churn.
7. **Ecosystem lock-in**: LangSmith heavily optimized for LangChain; PromptLayer cloud-control-plane-only data policies block strict-compliance orgs.
8. **Prompt-in-registry vs prompt-in-code religious war** — many engineering teams insist prompts live in git next to code (review, atomic deploys with code changes); non-engineer iteration demands a UI. Winning products serve both (LiteLLM GitOps + UI backends; Langfuse webhooks→git sync).

---

## 5. Own vs integrate — recommendation for the new gateway

**Own the thin runtime layer (high leverage, gateway-native):**
1. **Prompt registry with versions + labels** — table stakes everywhere; small surface; store in the gateway's own DB; expose `prompt_id`/`label`/`version` resolution in the OpenAI-compatible request (`prompt_id` + `inputs`, Helicone/LiteLLM-style), plus a render-only endpoint (Portkey-style).
2. **Pluggable prompt backends** (LiteLLM's smartest move): local files/dotprompt + git-backed + Langfuse + generic HTTP contract. Lets teams keep prompts in git while non-devs use the dashboard; avoids the lock-in objection.
3. **Traffic-split deployment** — % rollout / canary between prompt versions (and model configs) at the label level. The gateway already owns routing; this out-does Langfuse (app-side) and matches PromptLayer's dynamic release labels with less machinery. Nobody in the OSS gateway space has this today — differentiator.
4. **Dataset capture from traffic** — datasets as saved log filters (Cloudflare model) + tag-request-into-dataset API. Free given the gateway sees everything; this is the raw material every eval tool needs.
5. **Replay primitive** — re-execute a captured request set against a different prompt version/model via API (results as logs). Cheap to build on the proxy core; powerful for agents.
6. **Governance**: enforced/approved templates and injected system-prompt decorators per route/team (Kong's plugins, but with versioning); protected labels; audit trail.

**Integrate, don't own (low leverage, deep product wells):**
- **Evals/scoring (LLM-as-judge, scorers, annotation queues), experiment UIs, prompt optimization** — Langfuse/Braintrust/promptfoo do this full-time; Helicone's deprecated Experiments shows the cost of half-owning it. Ship: OTel/webhook export of datasets+replay results, a promptfoo-compatible export, and first-class Langfuse prompt-backend + trace-link support.

**AX requirements (given agent-first positioning):**
- Every prompt operation (CRUD, label move, dataset create, replay run) available via CLI + MCP tools — match Langfuse's built-in `/api/public/mcp` and beat it by also exposing traffic-split + replay as MCP tools.
- Prompts serializable to files (dotprompt-compatible) so coding agents can manage them through git PRs; webhook on label change for cache-bust/CI.
- Deterministic, machine-readable diffs between prompt versions (structured JSON diff, not just text) — nobody does this well today.

---

## Sources (primary)
- https://langfuse.com/docs/prompt-management/overview, /features/a-b-testing, /features/prompt-version-control, /features/mcp-server, https://langfuse.com/blog/2025-06-04-open-sourcing-langfuse-product, https://langfuse.com/changelog/2025-11-20-native-mcp-server, https://github.com/langfuse/langfuse
- https://portkey.ai/docs/product/prompt-engineering-studio, https://portkey.ai/docs/product/prompt-engineering-studio/prompt-versioning, https://docs.portkey.ai/docs/product/prompt-library/prompt-partials
- https://docs.helicone.ai/features/advanced-usage/prompts, https://docs.helicone.ai/features/experiments (deprecation), https://github.com/Helicone/helicone
- https://docs.litellm.ai/docs/proxy/prompt_management, https://docs.litellm.ai/docs/proxy/native_litellm_prompt, BerriAI/litellm PR #16834, issues #20872/#12097
- https://docs.promptlayer.com/features/prompt-registry/release-labels, https://blog.promptlayer.com/you-should-be-a-b-testing-your-prompts/
- https://docs.langchain.com/langsmith/manage-prompts, https://changelog.langchain.com/announcements/prompt-tags-in-langsmith-for-version-control
- https://www.braintrust.dev/docs/evaluate/playgrounds, https://www.braintrust.dev/blog/collaborative-evals-loop
- https://developers.cloudflare.com/ai-gateway/evaluations/, /features/dynamic-routing/
- https://developer.konghq.com/plugins/ai-prompt-decorator/, /plugins/ai-prompt-guard/, Kong AI Gateway announcement
- https://techcrunch.com/2025/08/13/anthropic-nabs-humanloop-team-as-competition-for-enterprise-ai-talent-heats-up/, https://agenta.ai/blog/humanloop-sunsetting-migration-and-alternative
- https://github.com/promptfoo/promptfoo, https://agenta.ai/blog/top-open-source-prompt-management-platforms
- https://www.truefoundry.com/blog/bifrost-vs-litellm, https://klymentiev.com/blog/llm-gateway-guide, https://particula.tech/blog/ai-gateway-decision-litellm-portkey-kong-ai-gateway
