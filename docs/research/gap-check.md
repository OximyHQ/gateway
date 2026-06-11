# Gap Check — AI/MCP Gateway Research Coverage (June 2026)

Web-search sweep for anything missed by the existing 50+ research agents. Coverage of the
mainstream LLM-gateway and MCP-gateway space is very good. Eight genuine gaps found —
mostly **adjacent categories** the gateway should absorb, plus a few new entrants and two
landscape-reshaping events.

## Landscape events affecting already-covered vendors (no new agent needed, but update notes)

- **Palo Alto Networks announced intent to acquire Portkey (Apr 30, 2026).** The most visible
  enterprise AI gateway is being folded into a security platform (Prisma AIRS). This validates
  the "gateway = security control point" thesis and removes an independent competitor.
- **OpenRouter raised $113M Series B (May 26, 2026)** led by CapitalG at ~$1.3B valuation, with
  NVIDIA/ServiceNow/MongoDB/Snowflake/Databricks ventures. Weekly token volume 5T → 25T in 6 months.
- **Katanemo/Plano → DigitalOcean** already captured in the research list.

## Missed item 1: Kubernetes-native inference routing for self-hosted fleets

Entire category absent from the list. For anyone running vLLM/SGLang/TensorRT-LLM fleets, the
"gateway" layer is now: **llm-d** (Red Hat/Google/IBM/CoreWeave/NVIDIA; CNCF Sandbox Mar 2026,
v0.7 May 2026; prefix-cache- and load-aware scheduling, disaggregated prefill/decode),
**NVIDIA Dynamo** (datacenter-scale disaggregated serving), **AIBrix** (ByteDance/vLLM control
plane: LoRA management, autoscaling), **Gateway API Inference Extension** (the k8s-standard
InferencePool/InferenceModel CRDs that Envoy AI Gateway and kgateway both implement), KServe,
vLLM production-stack, SGLang router. Also the intelligent-routing OSS twins **vLLM Semantic
Router** (Rust, ModernBERT intent classification, v0.2 "Athena" Mar 2026; +10.2pp accuracy,
−47% latency) and **RouteLLM** (Berkeley). A "most comprehensive gateway ever" must speak
KV-cache-aware/least-loaded routing to self-hosted endpoints, not just SaaS providers.

## Missed item 2: AI firewall / AI runtime security vendors

Guardrails were covered as a feature dimension, but not the standalone vendor category that
ships inline AI firewalls: **Palo Alto Prisma AIRS** (AI Runtime Firewall + API intercept +
red teaming; Protect AI integrated; now acquiring Portkey), **Cisco AI Defense** (Mar 2026
platform: Duo Agentic Identity, DefenseClaw governance, AI BoM), **F5 AI Gateway**,
**Akamai Firewall for AI**, **Lakera** (acquired by Check Point), **Prompt Security** (acquired
by SentinelOne), **Cloudflare Firewall for AI**. These define the enterprise security buyer's
expectations for what a gateway must enforce inline (prompt-injection, data leakage, tool
misuse, shadow-agent discovery).

## Missed item 3: Commercial MCP gateway/security startups (2025–26 wave)

The list has IBM ContextForge, Docker, Lasso, MetaMCP, mcpjungle, Obot, ToolHive — but misses
the funded commercial wave: **Runlayer** (out of stealth Nov 2025, $11M Khosla/Felicis;
MCP-specific threat detection: tool poisoning, shadowing, fake MCPs), **MintMCP** (STDIO→remote
hosting with auth/logging/compliance; aggressive comparison-content marketing), **Natoma**
(shadow-AI discovery + desktop MCP management), **Operant AI MCP Gateway** (runtime
discovery/defense across k8s + Bedrock), **Airia**, **MCP Manager**. These are the
competitive set enterprise buyers actually evaluate.

## Missed item 4: Agent sandboxing & egress control runtimes

Gateway-adjacent category: where agent/tool code executes and what network egress it gets.
**E2B** (Firecracker microVMs; notably NO egress policies), **Daytona** (27–90ms cold starts,
container + optional Kata/Sysbox), **Modal Sandboxes** (gVisor, granular egress policies, GPU),
**Vercel Sandbox**, **Blaxel**, plus DIY Firecracker/gVisor patterns. Egress control for
agents is the network-side complement of an MCP gateway — same policy plane should govern both.

## Missed item 5: AI billing/metering/monetization layer

Gateways meter tokens; these products turn meters into invoices: **OpenMeter** (OSS, has a
LiteLLM integration), **Lago** (OSS; Mistral runs on it; per-model input/output token rates),
**Metronome — acquired by Stripe for ~$1B (Jan 2026)** (was the metering layer behind
OpenAI/Anthropic/Databricks/NVIDIA; Stripe now auto-syncs LLM token prices + markup %),
**Amberflo**, **Revenium**, **Paid.ai**, Zenskar/Solvimon. A gateway with first-class
metering-export (or built-in rating/credits) absorbs this category for AI products.

## Missed item 6: Agent identity & cross-app authorization

Identity is becoming a gateway concern, distinct from the covered governance dimension:
**Okta Cross App Access (XAA)** — OAuth extension being incorporated as an **MCP authorization
extension**, plus Okta's own **Agent Gateway** (virtual MCP server + MCP registry + audit);
**WorkOS**, **Scalekit**, **Stytch**, **Descope** all ship MCP/agent-auth products; SPIFFE-style
workload identity for agents; Cisco Duo Agentic Identity. Standards-track: if XAA lands in the
MCP spec, every gateway must implement it.

## Missed item 7: Pydantic AI Gateway

New OSS entrant (open beta Nov 2025): AGPL-3.0 core, file-based config, runs on Cloudflare
edge; one key for OpenAI/Anthropic/Google/Groq/Bedrock; BYOK; real-time cost limits;
OTEL-native via Logfire; `logfire gateway launch` runs coding agents with zero keys on the
laptop. Notable because it comes from the Pydantic/Logfire ecosystem with a large Python
developer install base, and the keyless-local-dev pattern is an AX idea worth stealing.

## Missed item 8: New commercial gateways/routers (2025-26 long tail)

**Tetrate Agent Router Service** (managed "fleet of Envoy AI Gateways" from the Envoy
maintainers; OpenAI-compatible; traffic splitting/A-B; cost+5% pricing; Continue/Cline/goose
integrations), **LLM Gateway (llmgateway.io)** (OSS, self-hostable, 300+ models, zero markup on
BYOK), **nexos.ai** (enterprise LLM management, founded by Nord Security founders), Eden AI,
Inworld. Individually small; together they map the current go-to-market patterns (managed
OSS, zero-markup BYOK, security-bundled).

## Checked and judged NOT worth dedicated agents

- Hyperscaler agent runtimes (AWS AgentCore, Azure, GCP) — already covered.
- Lunar.dev/Javelin/Pomerium — already covered.
- Eval/prompt-experimentation platforms — covered by prompt-management + observability dimensions.
- GPTCache et al. — covered by caching dimension.
- Cloudflare MCP Server Portals — incremental to already-covered Cloudflare AI Gateway; fold into notes.
- Martian/Not Diamond/Unify — already covered (model-routing specialists).
