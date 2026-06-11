# Pydantic AI Gateway (PAIG) — Competitive Intelligence Report

**Researched:** 2026-06-10
**Category:** LLM gateway (OSS entrant, now consolidated into commercial Logfire platform)
**Vendor:** Pydantic Services Inc. (the Pydantic / Pydantic AI / Logfire company)

---

## 1. Executive summary / status (CRITICAL)

Pydantic AI Gateway launched in **open beta Nov 13, 2025** as an AGPL-3.0 open-source gateway running on Cloudflare Workers. **As of March 30, 2026 the open-source repo is ARCHIVED** and the gateway has been **consolidated into Pydantic Logfire** (the commercial observability SaaS). The standalone platform at `gateway.pydantic.dev` shut down **April 13, 2026**; remaining balances were refunded; **nothing migrated automatically** (keys, settings, usage history all required fresh setup in Logfire).

Strategic read: the "open-source gateway" play lasted ~4.5 months. The AGPL core (TypeScript on Cloudflare Workers, 189 stars / 14 forks at archive time) was effectively a beta-period artifact; the durable product is a **closed, managed gateway feature inside Logfire**, with self-hosting only via Logfire Enterprise (Helm chart, contact-sales). For a team building a true open-source single-binary gateway, PAIG is now primarily (a) a cautionary tale about OSS-as-marketing, (b) a source of genuinely good AX/design ideas (zero-translation routing, keyless local dev, hierarchical limits), and (c) evidence that even a beloved OSS brand (Pydantic) got near-zero community traction for a gateway repo (HN launch posts: 1–2 points, 0 comments).

---

## 2. Timeline

| Date | Event |
|---|---|
| Sep 2025 | Early preview / first HN submission (2 pts) |
| **Nov 13, 2025** | Open beta announcement; AGPL-3.0 repo public; gateway.pydantic.dev live |
| Jan 9, 2026 | Last OSS release: v0.0.3 (~200 commits total on main) |
| ~Feb–Mar 2026 | "Gateway is moving to Logfire" announcement; `logfire gateway launch` keyless-dev feature shipped |
| Mar 15, 2026 | Self-service refunds open on legacy platform |
| **Mar 30, 2026** | GitHub repo archived ("moved into Pydantic Logfire") |
| **Apr 13, 2026** | Legacy gateway fully shut down |

---

## 3. Architecture & implementation

- **Language:** TypeScript (95.8%), some Python (3.1%). NOT a single binary — a Cloudflare Workers application.
- **Runtime:** Cloudflare Workers on Cloudflare's global edge network; **Cloudflare KV** for caching state and cost tracking (no Durable Objects mentioned).
- **Deployment (OSS era):** self-host on your own Cloudflare account via `wrangler` CLI; config in a **TypeScript file** (`deploy/src/config.ts`) defining projects, users, providers, API keys; secrets via `.env.local`. Docker Compose present in repo for dev; `proxy-vcr` component for record/replay testing.
- **Core design principle — "one key, zero translation":** requests pass through in **each provider's native wire format** (no normalization to a common schema). Model string format: `gateway/<api_format>:<model_name>` (e.g. `gateway/anthropic:claude-...`). Claimed benefit: day-zero access to new provider features, no lossy translation layer. (Direct contrast with LiteLLM/OpenRouter-style OpenAI-schema harmonization.)
- **Current (Logfire) endpoints:** regional — `https://gateway-us.pydantic.dev/` and `https://gateway-eu.pydantic.dev/`.
- **Cost limits are "soft":** docs admit limits can't be enforced exactly because cost is only known after an LLM call completes; checks happen pre-request against accumulated spend.

## 4. Providers & protocols

- **Supported API formats/providers (5):** OpenAI, Anthropic, Google (Vertex / google-cloud format), Groq, AWS Bedrock.
- **No MCP gateway.** PAIG is purely an LLM-call proxy. (Pydantic's MCP story is separate: `logfire-mcp`, an observability MCP server — see §8.)
- **Key modes:** BYOK (your provider credentials stored in gateway; zero markup) or **built-in/managed provider accounts** (pay through Pydantic; 5% markup on Personal/Team, 3% on Growth+).
- **Pricing-data gate:** by default the gateway **rejects requests for models it has no pricing data for** (400 for custom providers, 404 for built-in) so cost tracking stays complete; a per-provider toggle lets unmapped models through untracked. Notable failure mode users hit with new models.

## 5. Feature surface (current, inside Logfire)

**Routing**
- Routing Groups: named collections of members serving identical models, with **priority levels (failover)** and **weights (load balancing)**; members can be marked active/inactive. Configured in Logfire UI (Gateway → Routing Groups).
- Provider swap without client reconfiguration (clients address the routing group).

**Cost governance (its strongest area)**
- Hierarchical spend limits: **organization → project → user → API key → session**, each with daily/weekly/monthly/total windows.
- Limits checked before requests reach the provider; enforced centrally so "you can't blow through them by switching models, regions, or machines."
- Per-session caps on **spend, request count, or token usage** (for `gateway launch` sessions) — hard ceiling for runaway agent loops.
- Real-time cost monitoring / spend insights in Logfire dashboard.

**Observability**
- Built-in OpenTelemetry instrumentation; every gateway request traceable in Logfire alongside application telemetry (cost, latency, errors in one place). Gateway traffic + app traces correlate in one trace tree.
- Centralized audit trail via OTEL traces.

**Dashboard (Logfire)**
- Gateway settings UI: provider credentials, routing groups, keys, limits.
- **LLM Playground**: test prompts against any model available through your gateway key, with automatic tracing.
- Inherits Logfire enterprise controls: SSO (OIDC), custom roles, fine-grained permissions, security-group mapping.

**Integrations**
- Pydantic AI: one-line `gateway_provider()` / `gateway/` model-string prefix; env var `PYDANTIC_AI_GATEWAY_API_KEY`.
- OpenAI SDK / Anthropic SDK / Vercel AI SDK via base-URL override.
- Claude Code (env vars), Codex (config file), OpenCode — first-class launch targets.

**Missing / never shipped (was "planned" at launch):** response caching, security guardrails, code execution, web search, RAG. No prompt management, no semantic caching, no eval tooling in the gateway itself (Logfire covers evals/observability separately).

## 6. Licensing & pricing

- **OSS era:** core AGPL-3.0 (deliberately copyleft to block cloud resellers); hosted UI + SSO closed-source. Now archived/unmaintained — practical OSS status today: **dead code, AGPL, TypeScript**.
- **Current pricing (via Logfire plans):** Personal $0 (credit card required to use gateway), Team $49/mo, Growth $249/mo, Enterprise (contact sales; self-hosted Logfire incl. gateway, SSO, custom retention). BYOK routing itself is free/no-markup on all plans; managed provider credits carry 5%/3% markup.
- Self-hosting now = **Logfire Enterprise self-hosted** (Kubernetes Helm chart, full Logfire stack) — heavyweight vs the old wrangler deploy.

## 7. AX (agent experience) — the part worth stealing

This is PAIG/Logfire's most original contribution: it treats **coding agents as the primary client** of the gateway.

1. **`logfire gateway launch <agent>` — keyless local dev.** `uvx --with 'logfire[gateway]' logfire gateway launch claude` runs Claude Code (or Codex, OpenCode) with **zero API keys stored on the laptop**:
   - CLI opens browser for a **PKCE OAuth flow** (no client secret on disk); user approves a session scoped to one project + gateway-proxy access.
   - CLI starts a **local proxy bound to 127.0.0.1** and hands the agent a **one-off bearer token valid only for that proxy**.
   - The real OAuth token lives only in proxy process memory, auto-refreshes, and is discarded on exit.
   - Each invocation is a **session** with its own spend/request/token caps — a runaway agent has a hard ceiling far below the monthly bill.
   - Explicit framing: "AI coding agents have turned developer laptops into exposed credential stores" — keys in dotfiles are reachable by any installed package.
2. **`logfire gateway serve`** — generic local proxy (base-URL) for any OpenAI- or Anthropic-protocol tool beyond the three first-class agents.
3. **Coding Agent Skills**: Pydantic ships a skills marketplace (Logfire Instrumentation skill, Logfire Query skill) installable into Claude Code (Anthropic marketplace plugin), Codex (plugin), and 30+ agents via the agentskills.io standard — agents learn how to instrument apps and query telemetry.
4. **`logfire-mcp`** — hosted remote MCP server (plus local option) exposing OTEL data: deliberately small, **4 tools** (find exceptions, exception details, arbitrary SQL over telemetry, schema). Lets agents self-debug ("what caused the 500 on checkout?").
5. **Codex exporter**: export Codex agent activity as OTEL traces into Logfire.

Caveat: gateway **configuration** is dashboard-first (routing groups, credentials, limits set in Logfire UI); no evidence of a public config API / Terraform provider / CLI for gateway admin. The AX brilliance is on the *consumption* side, not the *administration* side.

## 8. Performance

- No published latency/throughput benchmarks found anywhere. Only claim: runs on "Cloudflare's globally distributed edge compute network, meaning absolutely minimal latency" + zero-translation pass-through implying minimal per-request overhead. No numbers, no comparisons vs LiteLLM/Portkey/OpenRouter.

## 9. Traction & community sentiment

- **GitHub:** 189 stars, 14 forks, 2 watchers, ~200 commits, last release v0.0.3 (Jan 9, 2026), archived Mar 30, 2026. Very weak for the Pydantic brand (pydantic-ai has ~10k+ stars).
- **Hacker News:** three submissions (Sep 2025 preview, Nov 2025 open beta, Nov 2025 repost) earned **1–2 points and zero comments each**. Essentially no organic interest.
- Representative community criticism (HN, on the broader pivot): *"I really wish Pydantic invested in... Pydantic, instead of some AI API wrapper garbage."* — skepticism that the gateway is venture-driven product sprawl rather than community demand.
- No meaningful Reddit complaint threads found — the product seems too low-usage to generate complaint volume.
- The Logfire consolidation itself confirms weak standalone traction: separate accounts/billing/dashboards created "friction," and no usage numbers were ever published.

## 10. Weaknesses & gaps (for positioning)

1. **OSS strategy collapsed**: AGPL repo archived after 4.5 months; self-host path went from `wrangler deploy` to enterprise-sales Helm chart. Anyone burned by this is a prospect for a genuinely committed OSS gateway.
2. **Cloudflare-only architecture** (OSS era): can't run on a VM/k8s/laptop as a binary; requires a Cloudflare account even to self-host. No single-binary story ever.
3. **Migration burned users**: zero data portability (keys, usage history, settings), hard shutdown deadline, credit-card required even on free plan to use gateway.
4. **Narrow provider matrix**: 5 providers; no Azure OpenAI, Mistral, Cohere, Ollama/local models, OpenRouter passthrough, etc.
5. **No MCP gateway** — LLM proxy only; MCP exists only as a separate observability server.
6. **Promised features never shipped in gateway**: caching, guardrails, web search, code execution, RAG (announced as planned Nov 2025; absent from current docs).
7. **Soft cost limits** (post-hoc accounting) and the pricing-data gate that 400/404s unknown/new models by default — operational papercuts.
8. **Admin plane is UI-first**: no API-first/IaC/file-config administration in the Logfire incarnation (the OSS file-config model died with the repo).
9. **Vendor coupling**: gateway value is now inseparable from Logfire subscription and Logfire as the observability backend (OTEL-native, but the dashboard/limits/playground all live in Logfire).
10. **Zero community traction** despite a huge Python install base — brand reach in `pip install pydantic` (hundreds of millions of downloads/month) did not convert to gateway adoption.

## 11. What to steal

1. **Keyless local dev** (`gateway launch`): PKCE browser auth → localhost-only proxy → one-off bearer token in process memory → per-session spend/token/request caps. Best-in-class AX pattern; trivially portable to a single-binary gateway (`mygateway launch claude`).
2. **Zero-translation native pass-through** as a first-class routing mode alongside (not instead of) schema translation — day-zero new-feature support is a real differentiator vs LiteLLM-style normalization.
3. **Hierarchical limits with a session tier** (org → project → user → key → *session*) — the session tier is the novel bit; it maps exactly to agent runs.
4. **First-class launch targets** for Claude Code / Codex / OpenCode rather than generic "set your base URL" docs.
5. **Gateway + observability in one trace tree** — gateway spans correlated with app spans; cost/latency/errors unified.
6. **Small, sharp MCP surface** (4 tools) over the gateway's telemetry instead of a 50-tool kitchen sink.
7. **Agent skills distribution** (agentskills.io standard, Claude Code marketplace plugin) as a docs/onboarding channel for agents.

## 12. Lessons for our build

- AGPL alone doesn't create community; a gateway that only deploys to Cloudflare Workers excludes most self-hosters. A **single binary that runs anywhere** directly answers PAIG's structural weakness.
- An OSS gateway from a famous OSS brand still failed to gather contributors — distribution must come from the product being the default tool in a workflow (PAIG's bet: Pydantic AI's `gateway/` prefix), not from stars.
- The market window PAIG vacated: **open-source, self-hostable, file/API-configurable gateway with agent-session governance**. Its best ideas (keyless launch, session caps) are now locked inside a paid SaaS — re-implementing them in OSS is both differentiating and defensible.

---

## Sources

- https://pydantic.dev/docs/ai/overview/gateway/ (current gateway docs)
- https://github.com/pydantic/pydantic-ai-gateway (archived repo; OLD_README.md)
- https://pydantic.dev/ai-gateway (marketing page)
- https://pydantic.dev/articles/logfire-gateway-launch (keyless local dev)
- https://pydantic.dev/articles/gateway-merging-into-logfire (consolidation rationale)
- https://pydantic.dev/docs/logfire/gateway-migration/ + https://logfire.pydantic.dev/docs/gateway-migration/ (migration/shutdown timeline)
- https://pydantic.dev/pricing (Logfire plans + gateway markup)
- https://pydantic.dev/docs/logfire/guides/skills/ (coding agent skills)
- https://pypi.org/project/logfire-mcp/ + https://pydantic.dev/docs/logfire/guides/mcp-server/ (MCP server)
- HN: items 45209479, 45919374, 45933604 (1–2 pts, 0 comments each); comment 45056540 (community criticism)
