# TrueFoundry AI Gateway — Competitive Intelligence Report

Date: 2026-06-10
Subject: TrueFoundry AI Gateway (LLM Gateway + MCP Gateway + Agent Gateway)
Researcher context: input for a new open-source AI gateway (unified LLM + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

---

## 1. Positioning & Product Shape

TrueFoundry started as a Kubernetes-native ML deployment platform (HN 2023) and has pivoted its flagship to an **enterprise AI Gateway** marketed as three layered products on one control plane:

- **LLM Gateway** — unified OpenAI-compatible proxy to 1,000+ models / 250+ providers-models marketing claims (OpenAI, Anthropic, Bedrock, Vertex/Gemini, Azure OpenAI, Cohere, Mistral, Groq, self-hosted vLLM/SGLang/KServe/Triton, custom endpoints).
- **MCP Gateway** — centralized MCP server registry, OAuth token brokerage, tool-level RBAC, MCP guardrails (launched as a major pillar; heavily marketed for "Enterprise AI in 2026").
- **Agent Gateway** — launched early 2026; control plane for agent identity, service accounts, agent registry, workflow execution governance, A2A protocol mention.

Recognized as a Representative Vendor in Gartner's Market Guide for AI Gateways (Feb 2026). Compliance posture: SOC 2, HIPAA, GDPR; targets regulated industries (healthcare, finance, insurance).

**Not open source.** The gateway and control plane are closed-source commercial software (a recurring procurement objection). Open-source artifacts are peripheral: `truefoundry/models` (MIT community registry of model configs across 1,000+ models / 19 providers), `tfy-gateway-skills` (MIT, agent skills), SDKs, Helm charts, archived Cognita RAG framework, llm-locust benchmark tool.

---

## 2. Architecture (Gateway Plane / Control Plane)

Source: docs "Gateway Plane Architecture" + "Why the TrueFoundry LLM Gateway Is Blazing Fast" blog.

- **Three components**: (1) Gateway Plane — multiple stateless gateway instances handling traffic; (2) Control Plane — config for models/users/teams/rate-limits/routing; (3) Global Authentication Server — licensing + auth, hosted by TrueFoundry (always phones home, even self-hosted — relevant for air-gap claims).
- **Implementation**: Node.js with the **Hono** framework ("ultra-fast, minimalistic, designed for the edge"). Not a single binary — containerized pods on Kubernetes, scaled horizontally; CPU-bound by design.
- **Config distribution**: control plane publishes config changes over **NATS**; gateways hold everything **in-memory** — auth, authz, rate-limiting, load-balancing decisions all in-memory, **zero external calls in the request hot path** (except cache lookups). HTTP fallback to control plane if NATS is down; full config republication every 10 minutes for eventual consistency.
- **Telemetry path**: metrics/logs flow async to queues → aggregated into **ClickHouse** (blob-storage backed); aggregates (e.g., rate-limit counters) flow back to gateways via NATS.
- **Deployment modes**: SaaS, hybrid (gateway in customer VPC, control plane SaaS), fully self-hosted (Helm on EKS/GKE/AKS), on-prem, air-gapped, multi-region.

### Published performance numbers
- ~**3–4 ms added latency** vs direct OpenAI call (73 ms direct → 76–77 ms via gateway), benchmarked against LiteLLM (+15–26 ms, capped ~40–50 RPS on same hardware).
- **250 RPS on 1 pod with 1 CPU / 1 GB RAM**; +7 ms overhead at 200–220 RPS; +12 ms with full tracing; degradation starts ~**350 RPS per vCPU**; 270 MB peak memory.
- ~3,000 RPS claimed on a t2.2xlarge ($43/mo spot); "tens of thousands of RPS" via replicas.
- Marketing site: "sub-3ms internal latency", "99.99% uptime", "10B+ requests/month processed", "30% average cost optimization".
- "Sub-millisecond" claims apply to **policy enforcement decisions** (in-memory auth/authz/rate-limit checks), not end-to-end overhead.
- Benchmark caveat: tests used a fake OpenAI endpoint (no real token generation).

---

## 3. LLM Gateway Feature Surface (from docs)

### Unified API
- OpenAI-compatible Chat Completions (tools, JSON mode, structured output/schema mode, prompt caching, reasoning tokens), Embeddings, Rerank, Moderation, Images (generation/edit/variations), TTS, STT/transcription, audio translation, **Realtime/Live API**, Files, Batch, Fine-tuning, **Anthropic Messages API** (native), raw **Proxy API** passthrough, Responses/Compaction API.
- Multimodal: chat/completion/embedding/rerank model types; TTS/STT providers (e.g., Smallest AI Waves added in changelog); Vertex multimodal embeddings (text/image/video).

### Routing & Reliability
- **Virtual Models**: route across models by weight, latency, or priority with automatic retries; weighted load balancing, latency-based routing, fallback chains, geo-aware routing for regional compliance.
- **TrueFailover** (Jan 2026 product): outage resilience across model/region/provider failures.
- Automatic retries, timeouts; provider error classification + token accounting normalization.

### Cost & Quota Governance
- Rate limits per user / per model / per application / per endpoint / per team.
- Budget controls: token-based and cost-based quotas; daily, monthly, and (new) **quarterly budget windows**; budget alerts with **PagerDuty** notification target; cost attribution by team/agent/environment via metadata tags.

### Caching
- Exact-match caching + **semantic caching**.

### Guardrails (deep integration matrix — best-in-class breadth)
- Built-in (TrueFoundry-managed; first 3 are SaaS-only, self-hosted uses BYO providers): Content Moderation (Azure AI Content Safety), PII/PHI detection (Azure AI Language), Prompt Injection (Azure Prompt Shield), **Secrets Detection** (AWS keys, API tokens, private keys), **Code Safety Linter** (eval/exec/os.system/subprocess), **SQL Sanitizer** (DROP/TRUNCATE/unsafe DELETE-UPDATE), Regex pattern matching/redaction, **Cedar policy guardrails** for MCP tool access, **OPA guardrails**.
- External vendor integrations: OpenAI Moderations, AWS Bedrock Guardrails, Azure suite, Enkrypt AI, Palo Alto Prisma AIRS, Fiddler, CrowdStrike, Patronus AI, Google Model Armor, GraySwan Cygnal, Akto, TrojAI, Pillar Security; deployable HTTP wrappers for NeMo Guardrails, Guardrails AI, Lasso, Arthur, Verra; fully custom via "Custom Guardrail contract" (HTTP/Python).
- Four application points: LLM input (validate+mutate), LLM output (validate+mutate, non-streaming only), MCP pre-tool, MCP post-tool.
- Configured per-request via `X-TFY-GUARDRAILS` JSON header (+ `X-TFY-GUARDRAILS-SCOPE`), or gateway-level policy rules matched on requestor/model/tool in the dashboard.

### Auth & Access Control
- RBAC for users/teams/applications; scoped API keys; OAuth 2.0; SSO/SAML; Personal Access Tokens (PAT) and **Virtual Account Tokens (VAT)** for service accounts/agents; secrets vault integration; full audit logging (model usage, user access, config changes).

### Observability
- OpenTelemetry-compliant metrics, traces, request logs; full request/response storage for compliance; P50/P90/P99 + TTFT; filtering by model/team/geography/metadata; analytics dashboard; custom dashboard API; agentic-workflow tracing; latency graphs, token-level traces, centralized error logs.

### Prompt & Developer Tooling
- Prompt registry: versioned prompts + integrated playground; code-snippet generation (OpenAI client, LangChain); Agent App publication for stakeholder testing; request-level debugging.

---

## 4. MCP Gateway Feature Surface

- **Central MCP registry**: register public and self-hosted MCP servers in the control plane; searchable directory for dynamic agent tool discovery.
- **MCP Server Groups**: environment isolation, permission management, config control per team/use case.
- **Virtual MCP servers**: compose tools from multiple servers into one logical server.
- **OAuth token brokerage**: gateway stores/refreshes OAuth tokens per user per server; single credential for agents across many servers; federated IdP support.
- **Tool-level RBAC and ABAC**; Cedar-language policies for fine-grained tool access.
- **MCP guardrails**: pre-execution validation, real-time blocking of suspicious operations, post-execution output inspection (PII/secrets), **human approval workflows for high-risk actions**.
- Full tool-call audit logging for compliance.
- **Agent Playground**: build/test agents in-browser with live streaming of the agentic loop + exportable code snippets.
- Enterprise tool connectors marketed: Slack, GitHub, Confluence, Datadog, databases, internal APIs.

## 5. Agent Gateway (2026)

- Centralized agent identity/service-account management; OAuth2 + metadata-based policies on tool invocations.
- Agent registry; framework-agnostic (LangChain, CrewAI, custom); A2A protocol mention.
- Workflow execution through the gateway: retries, timeouts, controlled execution paths.
- Per-agent/per-workflow token- and cost-quotas; per-agent cost attribution; end-to-end traces spanning agent steps + model calls + tool calls.
- Positioning: LLM gateway = stateless LLM requests; MCP gateway = tool calls; Agent gateway = stateful agent orchestration governance.

---

## 6. Agent Experience (AX) — how agents use it

This is TrueFoundry's most notable forward-looking surface:

- **`tfy-gateway-skills` (GitHub, MIT, shell)**: skills per the **agentskills specification** so coding agents (Claude Code, Codex CLI plugins) can configure and operate the gateway in natural language — "scan this codebase and migrate all LLM calls to the gateway", "add a PII guardrail". Skills cover: onboarding/CLI setup, gateway config (routing, providers, guardrails, rate limits, budgets), codebase migration, observability/cost analysis, platform management (workspaces, secrets, access control), MCP server registration, prompt registry, agent registry, skills-registry publishing. Includes enforced workflows, credential checks, secret scanning, validation scripts. Early-stage (13 stars, no releases).
- **Config-as-code/GitOps**: all gateway controls declarable in YAML, applied via `tfy apply` CLI (>=0.14.2); Git as the declarative source of platform state; praised in G2 reviews for CI/CD integration. Terraform support is **not native** for the gateway itself (only for cloud infra provisioning; gateway Terraform "reach out to us").
- **Coding-agent governance content**: dedicated docs for Claude Code (route via settings.json env vars through the gateway; works with Claude Agent SDK `setting_sources=["project"]`) and OpenCode; heavy blog series on Claude Code enterprise governance — per-developer rate limits, MCP tool scoping, audit trails, cost tracking.
- **MCP registry as agent discovery surface**: agents dynamically discover tools via the registry; gateway handles per-user OAuth so agents never hold raw credentials.
- Per-request control via headers (guardrails JSON in `X-TFY-GUARDRAILS`) is an agent-friendly, no-dashboard-needed mechanism.

---

## 7. Pricing & Licensing

- Tiers: **Developer** (individual), **Pro** (~$499/mo for 2M requests + 5 API keys), **Enterprise** (quote-only; usage + signed-up users + product depth: AI Gateway vs MCP Gateway etc.).
- SaaS: no hosting cost; self-hosted enterprise: marginal infra cost cited at ~$600–$1,000/mo.
- Closed source; commercial license; AWS Marketplace listing exists.
- Criticism: enterprise-quote-only pricing makes pilots hard versus hosted free tiers; "pricing can be a concern for smaller teams" (G2).

---

## 8. Weaknesses & Complaints

- **Closed source** — fails procurement where OSI-licensed data path is required; no community inspection of the hot path. (Primary opening for an OSS competitor.)
- **Kubernetes-heavy footprint** — control plane + gateway + NATS + ClickHouse + Postgres; Helm, autoscaling, cluster upgrades land on the platform team; G2: "difficult setup… especially without prior cloud or Kubernetes experience". No single-binary or laptop-friendly mode.
- **Global Authentication Server stays in TrueFoundry infra** (licensing/auth) — friction with true air-gap claims.
- Node.js runtime, CPU-bound ~350 RPS/vCPU — fine, but a compiled single binary can beat both footprint and density.
- **No native eval/prompt-optimization loop** — standalone gateway; production failures don't feed evaluation; needs separate tooling (cited by FutureAGI comparison).
- Output guardrails don't apply to streaming responses (docs: "non-streaming only").
- Three best built-in guardrails (content moderation, PII, prompt-shield) are **SaaS-only**; self-hosted must BYO providers.
- Several built-ins lean on Azure services — extra vendor dependency.
- G2: self-service packaging immature, advanced features have a learning curve, telemetry/playbooks "still industrializing", complex pipelines need effort.
- Benchmarks are self-published against a faked provider endpoint; "sub-millisecond" marketing conflates policy-check time with request overhead.
- No native Terraform provider for gateway config (YAML/CLI only).
- Little organic community presence (almost no Reddit/HN discussion of the gateway itself); enterprise sales-led motion.

---

## 9. Implications for a New OSS Gateway (steal / counter)

**Steal:**
1. In-memory hot path with zero external calls per request; pub/sub config push + periodic full-state reconcile (their NATS pattern) — but collapse it into one binary with embedded state.
2. Four-point guardrail model (LLM in/out + MCP pre/post tool) with a simple HTTP custom-guardrail contract and per-request header overrides.
3. MCP OAuth token brokerage + virtual MCP servers + Cedar/OPA tool policies — clear enterprise differentiator.
4. Agent-skills repo pattern: ship official agentskills/Claude Code plugin so agents can self-configure the gateway (their version is early and shell-based — easy to leapfrog).
5. YAML config-as-code with `apply` semantics from day one; reviewers explicitly praise it.
6. Quota model breadth: token + cost budgets, daily/monthly/quarterly windows, PagerDuty alerting, per-agent attribution.
7. Marketing math: publish honest RPS/vCPU and added-latency-vs-direct numbers with reproducible harness (they win deals on the LiteLLM comparison).

**Counter / differentiate:**
- OSI-licensed end-to-end data path (their #1 procurement failure).
- Single binary, no K8s/NATS/ClickHouse dependency for the default deployment; sub-ms overhead from a compiled language.
- No phone-home licensing server.
- Streaming-capable output guardrails.
- Built-in guardrails that work self-hosted without Azure SaaS dependencies.
- Transparent pricing / free self-serve tier vs quote-only enterprise.

---

## Sources
- https://www.truefoundry.com/ai-gateway
- https://www.truefoundry.com/docs/platform/gateway-plane-architecture
- https://www.truefoundry.com/blog/truefoundry-llm-gateway-is-blazing-fast
- https://www.truefoundry.com/docs/ai-gateway/intro-to-llm-gateway
- https://www.truefoundry.com/docs/ai-gateway/guardrails
- https://www.truefoundry.com/docs/ai-gateway/mcp/mcp-overview
- https://www.truefoundry.com/agent-gateway
- https://www.truefoundry.com/mcp-gateway
- https://www.truefoundry.com/pricing
- https://www.truefoundry.com/docs/changelog
- https://github.com/truefoundry
- https://github.com/truefoundry/tfy-gateway-skills
- https://www.truefoundry.com/docs/setup-gitops-using-truefoundry
- https://www.truefoundry.com/docs/ai-gateway/claude-code
- https://futureagi.com/blog/truefoundry-alternatives-2026/
- https://www.g2.com/products/truefoundry/reviews
- https://www.businesswire.com/news/home/20260220396246/en/ (Gartner Market Guide recognition)
