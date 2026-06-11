# Hyperscaler AI Gateways — Competitive Intelligence Report

**Subject:** AWS (Bedrock + API Gateway patterns + AgentCore Gateway), Azure API Management GenAI gateway, GCP (Apigee + Vertex AI)
**Researched:** June 2026
**For:** Team building a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane)

---

## 0. Executive framing

None of the three hyperscalers sells "an AI gateway" as a single product the way LiteLLM/Portkey/Kong AI Gateway do. Each offers a **pattern assembled from primitives**:

| Cloud | The "AI gateway" actually is | Maturity as an LLM gateway |
|---|---|---|
| **AWS** | Three disjoint things: (1) API Gateway + Lambda fronting Bedrock (DIY), (2) the "Multi-Provider Generative AI Gateway" reference architecture — which is literally **LiteLLM on ECS/EKS deployed by Terraform**, (3) Bedrock-native platform features (inference profiles, guardrails, prompt routing) + **AgentCore Gateway** for MCP | Fragmented; AWS itself ships an OSS proxy rather than building one |
| **Azure** | **The most complete first-party offering**: APIM "AI gateway" = a set of LLM-aware policies + MCP server features layered on the existing APIM gateway. Not a separate SKU. | Most advanced cloud-provider AI gateway; only one with native token-based rate limiting + LLM policies built in (per third-party comparisons) |
| **GCP** | Apigee with LLM-specific policies (LLMTokenQuota, SemanticCache*, Model Armor integration) + an MCP proxy capability + API hub/Agent Registry; Vertex AI supplies the model-platform substrate (DSQ, Provisioned Throughput, Model Garden) | Strong policy catalog, enterprise-priced; MCP support newest (managed remote MCP servers announced Dec 2025) |

**The strategic takeaway:** hyperscalers win on governance/compliance gravity (the gateway lives where the enterprise's identity, network, and billing already live) and lose on developer experience, multi-cloud neutrality, config-as-code simplicity, and speed of iteration. All three have converged on the same enterprise feature checklist: token quotas, model failover/load balancing, semantic caching, content safety, token metrics per consumer, and (in 2025–2026) MCP tool governance.

---

## 1. AWS

AWS has **no single first-party multi-provider LLM gateway product**. Enterprises assemble one of three patterns.

### 1.1 Pattern A — Amazon API Gateway fronting Bedrock (DIY)

Reference: AWS Architecture Blog, "Building an AI gateway to Amazon Bedrock with Amazon API Gateway."

- **Flow:** client → API Gateway → (optional Lambda authorizer) → Lambda integration function that SigV4-signs and routes to Bedrock endpoints.
- **What it adds:** JWT/Cognito/custom auth via Lambda authorizers; **usage plans + API keys** for request-rate limiting (NOT token-aware); request throttling for noisy-neighbor control in multi-tenant setups; canary releases and stage-based lifecycle management; AWS WAF integration; private/regional endpoint types; API Gateway response streaming for model output.
- **Design nicety:** preserves original request shape so clients keep using standard AWS SDKs (boto3) unchanged — gateway is transparent.
- **What it lacks (critical):** no token counting, no token quotas (usage plans count *requests*), no semantic caching, no model failover logic (you write it in Lambda), no prompt/completion logging UI, payload/timeout constraints of API Gateway (29s default integration timeout historically; streaming workarounds needed), Lambda cold starts in the hot path. Everything LLM-specific is your code.

### 1.2 Pattern B — "Guidance for Multi-Provider Generative AI Gateway on AWS" (LiteLLM-based)

Repo: `aws-solutions-library-samples/guidance-for-multi-provider-generative-ai-gateway-on-aws` (MIT-0 license; Terraform 47% HCL, Python 41%, Shell 12%).

- **Architecture:** AWS WAF → ALB (optional CloudFront) → **LiteLLM proxy containers** on ECS Fargate *or* EKS (not both), multi-AZ VPC, NAT gateways for external provider egress, Secrets Manager for provider keys, RDS/ElastiCache backing LiteLLM.
- **Features (inherited from LiteLLM + AWS glue):**
  - Virtual API keys with per-user/team/org tracking; budgets; rate limits.
  - Retry/fallback routing across providers; prompt caching; **Bedrock Guardrails applied to ALL providers** (notable: AWS-native guardrails wrapped around OpenAI/Anthropic/Vertex traffic too); Bedrock Managed Prompts; chat history.
  - OpenAI-spec unified API across Bedrock, SageMaker, OpenAI, Anthropic, Vertex AI.
  - Observability: CloudWatch metrics/logs, S3 access logs, per-request usage persistence.
  - **LiteLLM Admin UI** for users/teams/keys/model-access restriction.
  - Auth: Okta OAuth2 JWT support; CloudFront secret-header validation.
  - Four deployment topologies: public+CloudFront, custom domain, direct ALB, private VPC-only.
- **What this signals:** AWS effectively concedes the unified-LLM-gateway layer to open source — the official answer is "run LiteLLM with our Terraform." Tenant isolation (per-tenant rate limits, API tokens, cost records) is the headline enterprise story in the Well-Architected "multi-tenant generative AI platform" lens.
- **Gaps/complaints:** it's a *guidance*, not a managed service — customer owns upgrades, scaling, LiteLLM version drift, and LiteLLM's own reliability quirks; no SLA; no MCP gateway in this guidance; UI is LiteLLM's, not AWS-branded.

### 1.3 Pattern C — Bedrock-native platform features (the "gateway-less" controls)

These are the controls enterprises actually use when they stay Bedrock-only:

- **Cross-region inference profiles:** geographic (US/EU) and **global** profiles auto-route to the optimal region; up to **2× the in-region default quota**; no routing surcharge; global CRI gives **~10% cost savings**.
- **Application inference profiles:** user-created inference profiles carrying **cost-allocation tags** — the Bedrock-native answer to per-app/per-team cost attribution. This is the closest thing to "virtual keys" in Bedrock itself.
- **Intelligent prompt routing:** Bedrock-managed routing between models in a family to trade cost vs quality automatically.
- **Prompt caching** (provider-level) and **Bedrock Guardrails** (content filtering, denied topics, PII, contextual grounding) applicable across models.
- **Model invocation logging** to CloudWatch/S3 (full prompts/completions for audit).
- **Pain points (well-documented on re:Post/Reddit):** default quotas frequently near-zero for new accounts ("429 Too many tokens per day" on first call), quota increases require support tickets; **throttling reserves quota on `max_tokens`, not actual generated tokens** — a notorious source of surprise 429s; 350-second idle-timeout connection resets through NAT/VPC endpoints/NLB for long-running inference.

### 1.4 AWS AgentCore Gateway (the MCP gateway — most agent-first thing any hyperscaler ships)

- Converts **APIs (OpenAPI), Smithy models, and Lambda functions into MCP tools** with "a few lines of code"; fully managed MCP endpoint.
- **Composition:** combine many APIs/functions into a single MCP endpoint.
- **Semantic tool search:** agents search across thousands of indexed tools at runtime to find the right one — explicitly designed to minimize prompt size/latency. This is a genuinely novel, agent-native feature.
- **Both ingress AND egress auth** fully managed: inbound OAuth for agents, **secure credential exchange/injection per tool** (egress), and (2026) **OAuth 2.0 on-behalf-of token exchange** for delegated auth.
- 2026 extensions: MCP **prompts and resources as first-class primitives**, dynamic runtime discovery of MCP servers, streaming + session management (stateful MCP), elicitation (mid-execution user input), sampling, progress notifications; **server-side tool execution** wired into the Bedrock Responses API (model calls tools without client-side orchestration).
- Observability: CloudWatch metrics (usage/invocation/performance/errors) + CloudTrail data events for full audit.
- **Pricing:** Search API $25/M calls, InvokeTool $5/M calls (consumption-based; example: 50M agent interactions → 50M searches + 200M tool invocations).
- Complement: AWS also connects API Gateway → AgentCore Gateway (blog pattern), and an MCP server exists to *control* AgentCore from coding assistants ("vibe coding" docs page) — i.e., AWS is exposing its agent infra to agents.

---

## 2. Azure API Management (APIM) "AI gateway"

Microsoft's docs (updated 2026-05-29) describe the AI gateway as a capability set across **all APIM tiers** (varies by tier), covering models, agents, and tools.

### 2.1 Supported API surfaces / providers

- **OpenAI Chat Completions AND Responses API**; **Anthropic Messages API** (v2 tiers); **Google Vertex AI API** schema. Models in Microsoft Foundry (née Azure AI Foundry/Azure OpenAI) or **non-Microsoft providers including Amazon Bedrock**; self-hosted models; OpenAI-compatible third-party inference providers.
- **Unified model API (preview):** one OpenAI-compatible client endpoint exposing multiple LLM backends with **automatic format translation** and policies applied once across all models — Microsoft building the LiteLLM-shaped router into APIM.
- **Remote MCP servers** and **A2A agent APIs** as first-class managed API types.
- Real-time (WebSocket) API support added via AI Gateway enhancements.

### 2.2 The policy catalog (the heart of it)

- **`llm-token-limit`** — token rate limit (TPM) and/or **token quota per hour/day/week/month/year**, keyed on ANY counter key (subscription, IP, arbitrary policy expression). Optional **prompt-token precalculation on the gateway** (`estimate-prompt-tokens`) to reject over-limit prompts without hitting the backend. Returns remaining-token headers/variables. (Older `azure-openai-token-limit` is the AOAI-specific twin.)
  - *Accuracy caveats:* completion tokens are **estimated** when responses stream; images overcounted at a flat 1,200 tokens when estimating; remaining-quota numbers are approximate and only converge near the limit.
- **`llm-emit-token-metric`** — emits prompt/completion/total token metrics to Application Insights/Azure Monitor with **custom dimensions** (client IP, API ID, user ID from headers, etc.) for per-consumer chargeback.
- **`llm-semantic-cache-store` / `llm-semantic-cache-lookup`** — semantic caching using an Embeddings API + **Azure Managed Redis (or any RediSearch-compatible external cache)**; similarity-threshold based completion reuse. GA, and applicable to third-party/OpenAI-compatible and self-hosted models, not just AOAI.
- **`llm-content-safety`** — pre-flight prompt moderation through **Azure AI Content Safety** before the prompt reaches any model.
- **Backends + load balancer** — round-robin, **weighted, priority-based, and session-aware** load balancing across model deployments (e.g., drain PTU instances first, spill to pay-as-you-go); **circuit breaker with dynamic trip duration honoring the backend's `Retry-After` header**.

### 2.3 MCP gateway capabilities

- **Expose any REST API operation as an MCP tool** (no code, no separate server) — "MCP server export."
- **Passthrough governance of existing MCP servers** (front a remote MCP server with APIM policies: auth, rate limits, monitoring).
- **Credential manager** injects OAuth2 tokens for backend calls made by MCP tools (egress auth).
- MCP registry/discovery via **Azure API Center** (organizational catalog of APIs, MCP servers, "skills"), synchronized with APIM; Copilot Studio connector so Microsoft agents consume the catalog.
- Tier note: MCP features in classic Developer/Basic/Standard/Premium (not Consumption); still rolling through preview→GA.

### 2.4 Foundry integration + governance posture (preview)

- AI gateway embeds into **Microsoft Foundry**: configure token quotas/rate limits per model deployment from the Foundry UI; **register agents running anywhere (Azure, other clouds, on-prem) into a central control plane**; register MCP tools for automatic governance/discovery. This is Microsoft's "single pane for models + agents + tools."
- Managed-identity auth to Azure AI services (no API keys); OAuth for apps/agents via credential manager.
- Observability: prompt/completion logging to Azure Monitor; per-consumer token metrics in App Insights; **built-in token-consumption analytics workbook/dashboard**.
- "AI Gateway Early release channel" — opt-in early-access update channel for new gateway features.
- Rich sample ecosystem: `Azure-Samples/ai-gateway` labs, AI hub gateway landing-zone accelerator, APIM policy toolkit for custom policies.

### 2.5 Pricing & performance

- Basic v2 ~$150/mo, Standard v2 ~$700/mo (50M requests included), Premium v2 ~$2,800/mo/unit; Standard unit ≈ **~2,500 RPS estimated throughput** (scales linearly with units; Basic/Standard v2 → 10 units, Premium v2 → 30).
- No per-token gateway charge — pricing is per gateway unit + request overage.

### 2.6 Weaknesses / complaints

- Policy authoring is **XML with embedded C# expressions** — widely disliked; policy toolkit exists precisely because raw XML is painful.
- Gateway adds latency to every call; troubleshooting guides acknowledge policy-execution overhead and capacity-exhaustion latency cliffs; semantic cache requires running (and paying for) Azure Managed Redis.
- Token accounting is approximate under streaming; tier/feature matrix is confusing (classic vs v2 vs Consumption support differs per feature, e.g., Anthropic schema = v2 only, MCP export ≠ Consumption).
- Deeply Azure-centric: identity (Entra), logging (Azure Monitor), caching (Managed Redis), safety (Azure Content Safety) — multi-cloud teams inherit the whole Azure stack.
- Classic-tier APIM is historically slow to provision/update (tens of minutes for config in some operations), which v2 tiers only partially fix.

---

## 3. Google Cloud — Apigee + Vertex AI

### 3.1 Apigee LLM policies (first-party, in-product)

- **`LLMTokenQuota` policy** — enforce token-consumption limits per minute/hour/day/month, scoped by **API product / app / developer**; two placement modes: `CountOnly` (response flow, track actual tokens consumed) and `EnforceOnly` (request flow, block at quota); returns **HTTP 429** when exceeded. Granular quota management tied to Apigee's existing API-product monetization model.
- **`SemanticCacheLookup` / `SemanticCachePopulate` policies** — semantic caching built on **Vertex AI Text Embeddings API + Vertex AI Vector Search**; caches responses for semantically similar prompts on **any model**.
- **Model Armor integration** — Google's model-safety service inspecting every prompt/response: prompt-injection and jailbreak detection, responsible-AI filters, malicious-URL filtering, sensitive-data protection, topic guardrails.
- **Routing & failover** — `llm-routing` reference (route by use case, e.g. Gemini Pro for quality vs Flash for speed, behind one API contract) and `llm-circuit-breaking` reference (re-route when a model hits token rate limits). Note: these are **GitHub reference solutions/templates** (`GoogleCloudPlatform/apigee-samples`), not single built-in policies — more assembly required than Azure's equivalents.
- Classic API security on top: API keys, OAuth2, JWT, OWASP Top-10 protections, rate limiting, abuse/DDoS protection, per-model access control.
- **Logging/compliance:** log prompts, responses, and RAG data to Cloud Logging with **de-identification** options; audit trails.
- **Analytics:** token-based usage dashboards per app/developer, cost reporting, latency/quality analysis (reference solution in `llm-token-limits`).

### 3.2 Apigee MCP support (Dec 2025 →)

- **Fully managed remote MCP servers:** create an "MCP proxy" (basepath `/mcp`, target `mcp.apigee.internal`) + attach OpenAPI specs → existing APIs become MCP tools, **no code, no server to run**, governed by the same Apigee policies with full visibility into agentic interactions.
- **API hub treats MCP as a first-class API style** (alongside REST/gRPC): import/register/manage MCP APIs and their tools; **managed Agent Registry integration auto-syncs MCP servers + tool metadata** so agents discover governed APIs without manual config.
- Apigee positioned in two patterns: **agent proxy layer** (between apps and agents, incl. A2A-style orchestration) and **model gateway** (between apps and LLM providers).
- **Apigee AI Gateway for ADK:** an `ApigeeLlm` wrapper so Google ADK agents call models *through* Apigee (Vertex/Gemini generateContent), inheriting gateway policies — agent-native consumption path.

### 3.3 Vertex AI platform substrate (what the gateway routes to)

- **Model Garden:** 200+ first-party, third-party (Anthropic Claude, etc.), and open models behind one platform.
- **Dynamic Shared Quota (DSQ):** pay-as-you-go has no fixed per-customer quota — a shared pool dynamically allocated; throughput varies with aggregate demand (predictability complaint: your effective capacity depends on other customers).
- **Provisioned Throughput (PT):** fixed-cost subscription reserving throughput per model/location, standardized across Model Garden models.
  - **Known footgun (GitHub issue googleapis/python-genai#2113):** requests to *regional* endpoints silently bypass PT and fall back to on-demand billing; only the **global endpoint** consumes PT quota — enterprises pay twice without noticing.
- Global endpoints, prompt caching, batch, and Model Armor available platform-wide.

### 3.4 Pricing & complaints

- Apigee: subscription tiers (Standard/Enterprise/Enterprise Plus) or pay-as-you-go; **PeerSpot-reported real-world cost ≈ $100K/year SaaS subscription**, hybrid/on-prem higher, plus $40–60K initial professional services; widely reviewed as "high-priced, suitable for large enterprises"; licensing complexity and paid add-ons (monitoring) are recurring complaints.
- Complexity: proxy bundles, flows, XML policies, environment groups — steep learning curve; LLM features partly delivered as sample repos rather than product.
- Latency: Apigee adds a managed-proxy hop; semantic caching requires standing up Vector Search infrastructure (its own cost/ops).

---

## 4. Cross-cutting analysis

### 4.1 What enterprises GET from hyperscaler AI gateways

1. **Token quotas per consumer** — Azure `llm-token-limit` (any counter key, TPM + calendar quotas, prompt precalc), Apigee `LLMTokenQuota` (API-product-scoped, count/enforce split), AWS only via LiteLLM guidance or request-level usage plans.
2. **Model failover/load balancing** — Azure is best-in-class here (priority/weighted/session-aware LB + Retry-After-aware circuit breaker as *config*, purpose-built for PTU-first-then-PAYG spillover); Apigee via reference templates; AWS via LiteLLM router or Bedrock cross-region profiles.
3. **Semantic caching as a gateway policy** — Azure (Redis/RediSearch) and Apigee (Vertex Vector Search) both productized; AWS has nothing first-party at gateway level (Bedrock prompt caching is exact-prefix, provider-side).
4. **Content safety in the request path** — Azure Content Safety policy, GCP Model Armor, AWS Bedrock Guardrails (notably appliable to non-Bedrock providers via the LiteLLM guidance).
5. **Per-consumer token metering + chargeback** — Azure emit-token-metric dims + workbook; Apigee analytics by app/developer; AWS application inference profiles with cost-allocation tags.
6. **MCP tool governance** (2025–26 convergence) — all three now: expose REST as MCP tools, govern existing MCP servers, central registries (API Center / API hub + Agent Registry / AgentCore), egress credential injection.
7. **Compliance gravity:** identity, networking (private endpoints/VPC), audit (CloudTrail/Azure Monitor/Cloud Logging), and procurement already exist — biggest moat.

### 4.2 What enterprises MISS (the gaps an OSS gateway can exploit)

1. **No true cross-cloud neutrality.** Each gateway routes "any provider" but instruments/observes/secures only in its own cloud's stack. Multi-cloud enterprises run two gateways or settle.
2. **AWS has no first-party unified LLM gateway at all** — the official guidance is "deploy LiteLLM yourself." Huge validation for OSS, and a gap: no managed SLA, you own ops.
3. **Token accounting is approximate** — Azure estimates under streaming (flat 1,200/image), Bedrock throttles on `max_tokens` not actuals, Vertex DSQ throughput is nondeterministic. Nobody does exact, provider-reconciled token/cost accounting at the gateway.
4. **Config ergonomics are poor:** XML policies (Azure, Apigee) and Terraform+containers (AWS). No "one binary + one declarative config file." Hours-to-days to first request vs minutes.
5. **LLM features gated behind enterprise pricing/tiers:** Apigee ~$100K/yr; Azure feature matrix split across classic/v2/Consumption tiers; AgentCore separate per-call billing.
6. **Dashboards are generic API-management consoles** retrofitted with token workbooks — no prompt/completion explorer comparable to Portkey/Helicone/Langfuse-class LLM observability.
7. **Semantic caching needs extra paid infra** (Managed Redis; Vertex Vector Search) instead of being built in.
8. **Routing intelligence is shallow:** priority/weight/health, not cost-or-latency-aware per-request model selection (Bedrock intelligent prompt routing exists but only within Bedrock model families).
9. **No agent-first control plane** for the gateway itself (see AX notes) — config still assumes a human in a portal.

### 4.3 Agent-experience (AX) observations

- **AWS AgentCore Gateway is the AX benchmark:** semantic tool search over thousands of tools, MCP prompts/resources/elicitation/sampling, OAuth on-behalf-of exchange, server-side tool execution from the model API, plus an **AgentCore MCP server so coding agents can configure AgentCore itself**. Steal: tool-search-as-a-tool; per-tool credential injection; consumption pricing per tool call.
- **Azure:** everything is ARM/Bicep/REST-manageable (good machine surface), MCP export is wizard-driven; Foundry registers agents/tools from anywhere into one inventory. But policies are XML blobs — hostile for agents to generate/diff safely; no first-party "MCP server to manage APIM."
- **GCP:** Apigee MCP proxy + Agent Registry auto-sync means agents *discover* governed tools automatically; ADK's ApigeeLlm makes the gateway the default model path for Google-stack agents. Gateway management itself is still console/gcloud/Terraform, not agent-native.
- **Common gap:** none offers an agent-first control plane for the *gateway itself* (e.g., "MCP tools to create a route, set a quota, read spend"). All three assume humans configure in portals/IaC; agents are only *consumers*. A new gateway whose admin surface IS an MCP server/CLI is differentiated against all three.

### 4.4 Published performance / pricing numbers (sparse — none publish gateway latency overhead)

- Azure APIM: ~2,500 RPS estimated per Standard unit; Basic v2 $150/mo, Standard v2 $700/mo (50M reqs), Premium v2 ~$2,800/mo/unit.
- AWS: Bedrock cross-region inference up to 2× in-region quota, global profile ~10% cheaper; AgentCore Gateway $25/M Search + $5/M InvokeTool calls; LiteLLM guidance = pay for ECS/EKS/ALB/NAT/Redis/RDS infra.
- GCP: Apigee real-world ≈ $100K/yr SaaS (PeerSpot); Vertex PT fixed-cost subscription; no Apigee throughput/latency numbers published for LLM policies.
- **No hyperscaler publishes added-latency benchmarks for its AI gateway policies** (semantic cache lookup cost, token-estimation overhead) — an opening for an OSS project that publishes honest overhead numbers.

### 4.5 Implications for a new OSS gateway (what to steal / what to beat)

**Steal:**
- Azure: calendar-period token *quotas* (not just TPM) on arbitrary counter keys; prompt-token precalc rejection; Retry-After-aware circuit breaker; priority LB for "drain provisioned capacity first"; session-aware LB; unified model API with format translation; built-in token analytics dashboard.
- AWS: semantic tool search across MCP tools; per-tool egress credential injection + OBO token exchange; application-inference-profile-style cost tags on virtual keys; guardrails applied uniformly across all providers.
- GCP: CountOnly vs EnforceOnly quota placement; MCP-as-first-class API style in the catalog; Agent Registry auto-sync; Model Armor-style pluggable prompt/response sanitization; logging with de-identification.

**Beat them on:** single binary + declarative config (vs XML/Terraform/consoles); exact streamed token accounting reconciled with provider billing; built-in semantic cache (no external Redis/Vector Search bill); cross-provider cost/latency-aware routing; LLM-native observability UI; agent-first admin surface (MCP control plane for the gateway itself); price (free vs $8K–100K/yr).

---

## Appendix: Key sources

- Azure: learn.microsoft.com `genai-gateway-capabilities` (2026-05-29), `llm-token-limit-policy`, `mcp-server-overview`, APIM pricing page, Azure-Samples/ai-gateway labs.
- AWS: `guidance-for-multi-provider-generative-ai-gateway-on-aws` (GitHub, MIT-0), AWS Architecture Blog AI-gateway-to-Bedrock post, Bedrock cross-region/inference-profiles docs, AgentCore Gateway docs + pricing page + "Extending MCP support" blogs (2026), re:Post 429/throttling threads.
- GCP: cloud.google.com Apigee-for-AI blog, LLMTokenQuota/SemanticCacheLookup/SemanticCachePopulate policy docs, "MCP support for Apigee" blog (Dec 2025), API hub Agent Registry docs, Vertex DSQ/Provisioned Throughput docs, googleapis/python-genai#2113, PeerSpot Apigee pricing reviews.
