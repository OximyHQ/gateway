# Dimension Deep-Dive: Dashboard & Admin UX for AI Gateways

Date: 2026-06-10. Subjects: LiteLLM UI, Bifrost UI (Maxim), Portkey, Helicone, OpenRouter, TensorZero UI, ContextForge (IBM MCP Gateway) admin. Research via web search + official docs/GitHub.

Context as of mid-2026 (matters a lot for this dimension):
- **Helicone acquired by Mintlify (Mar 3, 2026)** — product in maintenance mode, no new feature dev. Its dashboard is still the UX benchmark people cite.
- **Portkey acquired by Palo Alto Networks (Apr 2026)**; in Mar 2026 it fully open-sourced the gateway *including* previously SaaS-only governance/observability/auth/cost-control. Hosted console remains proprietary.
- **LiteLLM hit by CVE-2026-42271** (MCP test-connection endpoints → command injection), chained with Starlette host-header bypass (CVE-2026-48710) to **unauthenticated RCE**, actively exploited, in CISA KEV (June 2026). Plus a PyPI typosquat incident (Mar 24, 2026). Admin-UI-adjacent endpoints were the attack surface.
- **ContextForge shipped 1.0.0 (RC2 era)** with heavy admin-UI polish but still documents the Admin UI as a dev-only convenience to disable in production.

---

## 1. LiteLLM Admin UI

**What it is:** React/Next.js dashboard served by the Python proxy at `/ui`, driving the proxy's management API (everything in the UI is also a REST endpoint).

**Screens (the widest surface of any OSS gateway):**
- **Virtual Keys** — create/edit/rotate keys; per-key budgets, rate limits (TPM/RPM), model allowlists, metadata, expiry; key regeneration.
- **Test Key / Playground** — built-in chat playground to exercise a key against configured models.
- **Models + Model Hub / AI Hub** — add models *without restarting the proxy*; public model/agent catalog page for internal developers; price data synced from GitHub.
- **Usage** — spend by key/team/user/model/provider over time; token burn; latency dashboards.
- **Logs** — per-request spend, tokens, key, team; request/response viewing (opt-in).
- **Teams / Organizations / Internal Users** — hierarchy, member roles, self-serve key creation, invitations, per-team budgets and model access.
- **MCP Servers** — register MCP servers (URL + transport), MCP tool-testing playground, **Toolsets** tab, per-entity (key/team/org) MCP permissions.
- **Guardrails** — configure guardrails and invocation stage (pre/post/during call, during logging).
- **Settings** — routing, logging/alerting integrations (Slack/Langfuse/etc.), SSO config, router settings.
- **Search tools** across admin UI entities.

**Praised:** completeness — it's the de-facto checklist everyone copies; key/team/budget model is the industry's mental model; add-model-without-restart; MCP permission management in UI is genuinely ahead.

**Hated / weaknesses:**
- Recurring **performance regressions**: GitHub issues "UI very slow in 1.82.x", "significant perf regression 1.81.x (UI + API)"; UI dies at scale (**service failure at ~380k keys**, issue #19477) — list views not built for large tenants.
- **Security blast radius:** the MCP "test connection" UI feature accepted full stdio server configs (command/args/env) server-side → RCE (CVE-2026-42271). A cautionary tale: admin-UI convenience endpoints = privileged code execution paths.
- **Enterprise gating inside the UI:** SSO beyond 5 users, audit logs, several guardrail callbacks, "enterprise management UI" features require a paid license (~$250/mo Basic, ~$30k/yr Premium). OSS users see locked/greyed "premium feature" affordances — widely disliked.
- General jank: distributed deployments cause UI issues (sticky sessions/CDN caveats documented); visual polish is low; lots of tabs of uneven depth.

**AX (agent experience):** strong in principle — every UI action maps to a management REST endpoint (`/key/generate`, `/team/new`, etc.), so agents can administer it API-first; LiteLLM also *is* an MCP gateway. But no first-party admin MCP server; auth model for management API is the same master-key pattern.

**OSS:** MIT core; UI usable but key admin features enterprise-licensed. Python (proxy) + Next.js (UI).
**Perf:** community benchmarks ~8–40ms added latency P95; the UI itself is a known perf liability.

---

## 2. Bifrost UI (Maxim AI)

**What it is:** React 19 + Vite + TanStack Router + Redux Toolkit/RTK Query dashboard built into the Go single-binary gateway. Closest architecture to "new OSS gateway, single binary, dashboard" — the direct competitor template.

**Screens:**
- **Live Logs dashboard** — WebSocket real-time streaming of requests, advanced filtering, request/response inspection, connection status + auto-reconnect.
- **Providers** — visual config of 15+ providers (1000+ models), per-provider API keys, model assignment per key, network config (proxies, timeouts), weighted key distribution.
- **Governance** — virtual keys, budgets, rate limits, team/customer hierarchy.
- **MCP Clients** — manage MCP tool connections from the gateway.
- **Plugins** — manage plugin ecosystem (semantic cache, Maxim logger, OTel, mocker).
- **Analytics** — request metrics, success rates, latency, token usage, provider performance comparison, error categorization, trends.
- **Docs hub** — built-in documentation browser inside the app.
- Dark/light theme.

**Praised:** zero-setup ("built-in dashboard for quick glances without complex setup"); config entirely via UI *or* file/API; performance story (11µs overhead claim; ~9.5x faster, ~54x lower P99, 68% less RAM than LiteLLM on a t3.medium — marketed as "50x").
**Weaknesses:** young — analytics depth is thin vs Portkey/Helicone (no cost-center reporting, no user-level analytics, no log search language); benchmark marketing ("50x") draws skepticism since real multiplier is workload-dependent; vendor-led content dominates search results (few independent user reports); observability beyond the built-in dash pushes you to Maxim's paid platform.
**AX:** config file + REST API parity with UI; OTel export; no admin MCP. WebSocket log stream is machine-consumable.
**OSS:** Apache 2.0, Go. UI fully open (no feature gating observed) — a differentiator vs LiteLLM.

---

## 3. Portkey (hosted console; gateway OSS)

**What it is:** the most mature commercial console. Gateway (MIT, TypeScript/edge-deployable) + proprietary hosted control plane; Mar 2026 OSS drop moved governance/observability/auth/cost-control into the OSS gateway.

**Screens / capabilities:**
- **Analytics dashboard** — 40+ metrics, 15+ filters, cost/latency/error views, feedback analytics, metadata-scoped views (per-user, per-env, per-crew).
- **Logs** — every request with full detail; **Replay button opens any logged request in the prompt playground** to re-run/edit until it works (best-in-class loop); manual feedback annotation on logs.
- **Prompt management + Playground** — versioned prompts, deployments, A/B compare across 1600+ models, parallel tests.
- **Configs** — routing strategies (fallbacks, load balancing, retries, canary) as versioned JSON configs editable in UI and referenced by ID in requests.
- **Virtual keys / API keys** — provider key vault, budget/rate limits per key.
- **Guardrails** — 50+ guardrails configured in UI, attached to configs.
- **Audit/activity logs** — every admin action across resources tracked (governance-grade).
- **Admin API + community admin MCP server** — manage prompts, configs, analytics, API keys from any MCP client.

**Praised:** observability depth + the log→replay→playground loop; configs-as-versioned-objects; enterprise governance.
**Weaknesses:** pricing scales with logged requests + retention — expensive at volume; governance was Enterprise-gated (~$2–5k/mo) pre-OSS-drop; full observability historically required their cloud (no on-prem for compliance until recently); Palo Alto acquisition creates roadmap/identity uncertainty (consolidation under Prisma AIRS); console itself remains closed source.
**AX:** best of cohort — documented **Admin API** covering the whole console surface + MCP server for it; `llms.txt` published for docs.
**Perf claims:** <1ms gateway overhead (managed edge 20–40ms incl. network); 99.99% SLA; "2 trillion tokens/day".

---

## 4. Helicone

**What it is:** OSS (Apache-2.0) LLM observability platform + AI gateway (Rust gateway "one-liner" proxy). YC W23. Acquired by Mintlify Mar 2026 → **maintenance mode** (security updates, new models, bug fixes only). Stack: ClickHouse + Kafka.

**Screens:**
- **Dashboard** — cost breakdowns, latency percentiles, request volume, top models/users out of the box.
- **Requests** — full prompt/completion, tokens, latency, computed cost; filter by time/model/user/custom property; **HQL** (query language) over logs.
- **Sessions** — grouped multi-request agent traces; session-type filtering and cross-session performance comparison.
- **Users** — per-end-user metrics (requests, cost, activity) via `Helicone-User-Id` header.
- **Properties** — arbitrary custom dimensions via headers; become filters/groupings everywhere.
- **Playground** — open any request/session and iterate on prompts in UI.
- **Experiments** — run prompt variants against historical production data with quantified comparison (regression prevention).
- **Datasets / Evals**, **Alerts** (Slack, threshold-based), **Webhooks** (sampled, property-filtered), **Cache** + **rate-limit policies** configured via headers (`10;w=1000;u=cents;s=user`).

**Praised:** the UX gold standard of the cohort — "change a base URL, see every request the same afternoon"; instant cost visibility; header-based features mean zero SDK lock-in.
**Weaknesses:** maintenance mode = dead-end risk (community actively migrating to Langfuse etc.); shallower tracing than Langfuse for complex agent graphs; header-config (cache/rate limits) is invisible/undiscoverable from the UI — config state lives in client code; self-hosting was historically painful (ClickHouse+Kafka; they wrote a "we simplified self-hosting in 30 days" mea culpa).
**AX:** everything is HTTP headers — trivially automatable by agents; REST API for data; no admin MCP.

---

## 5. OpenRouter

**What it is:** closed-source hosted multi-provider router (500+ models). Its dashboard is consumer-grade simple but has several best-in-class public surfaces.

**Screens:**
- **Activity** — historic usage filtered by model/provider/API key; CSV/PDF export grouped by model, key, or org member.
- **Keys** — per-key spend limits with daily/weekly/monthly auto-reset, labels; per-environment keys with own caps/alerts/activity.
- **Provisioning keys** — a key class that can *only* mint/manage other keys via `/api/v1/keys` (programmatic key issuance for platforms/agents). Steal this.
- **Credits** — balance, purchases, refunds; live credits API.
- **Settings** — default model, provider preferences, privacy/ZDR controls (block providers that train/log; 1% discount for opting into logging), BYOK per provider.
- **Chatroom/Playground** — multi-model chat (local-only storage, no sync).
- **Public model catalog** — per-model pages with live latency, throughput, uptime per underlying provider, pricing/M tokens.
- **Rankings/leaderboard** — public token-share leaderboard by model/app; became the industry's demand barometer (huge top-of-funnel; unique asset).
- **Organizations** — members, roles, shared billing, per-member attribution in activity export.

**Praised:** model catalog with live provider performance/uptime data; rankings page; frictionless credits; ZDR/privacy routing controls exposed as user-facing toggles.
**Weaknesses:** closed-source, cloud-only, ~5.5% credit fee; no logs of request *content* (it's a router, not observability — you bring Helicone/Langfuse); chat history doesn't sync; opaque support/account issues recur in community reports; no self-host story at all.
**AX:** credits API, provisioning-keys API, generous machine-readable model metadata (`/api/v1/models` is the ecosystem's de-facto model DB). No MCP control plane.

---

## 6. TensorZero UI

**What it is:** OSS (Apache 2.0) LLMOps platform: Rust gateway + separate UI container (`tensorzero/ui`, port 4000) over ClickHouse (+ Postgres for auth). Config is **TOML-file/GitOps-first**; the UI is a read/analyze/optimize surface, not the primary config editor.

**Screens:**
- **Observability** — Inferences (individual calls), **Episodes** (multi-inference workflows), Functions (aggregate views per declared function).
- **Playground** — interactive prompt iteration; replay historical inferences with new prompts/models/variants.
- **Datasets** — "Build Dataset" from logged inferences for evals/optimization.
- **Evaluations** — heuristics + LLM-judge over datasets, inference- or episode-level.
- **Optimization** — launch supervised fine-tuning (and other recipes) from the UI using logged data; experimentation/A-B over variants tracked with metrics/feedback.
- **API keys** page (`/api-keys`) when gateway auth enabled (requires Postgres).

**Praised:** only cohort member where the dashboard closes the loop **inference → feedback → dataset → eval → fine-tune → A/B** (the "data flywheel" framing); structured schema in ClickHouse (not blob logs); Prometheus + OTLP export.
**Weaknesses:** UI not standalone (needs ClickHouse URL + gateway URL + creds; issue #5062 "make UI fully standalone"); historically no auth on the UI (gateway auth recent, Postgres-gated); no key/team/budget governance screens at all — not an admin console; no caching/rate-limiting in the product; engineer-centric (TOML config required before anything shows up); steeper conceptual model (functions/variants/episodes).
**AX:** GitOps TOML = perfect for agents editing config via PRs; everything queryable straight from ClickHouse; no admin API for governance (none exists), no MCP.

---

## 7. ContextForge (IBM MCP Context Forge) Admin UI

**What it is:** OSS (Apache 2.0, Python/FastAPI + HTMX/Alpine-style admin) MCP/A2A/REST federation gateway + registry. Admin UI at `:4444/admin`. The only cohort member whose admin UX is MCP-native — the template for the MCP half of a combined gateway.

**Screens / concepts:**
- **Virtual Servers** — compose selected tools/resources/prompts into one context-specific MCP endpoint (the killer concept: curated tool bundles per team/agent).
- **Global Tools** — register MCP or REST functions with JSON-Schema validation, auth config, per-tool enable/disable; tool testing in UI.
- **Global Resources / Global Prompts** — URI-addressed read-only data; templated prompts with arguments.
- **Gateways** — federate other MCP servers; their tools/resources/prompts surface locally.
- **MCP Server Catalog** — YAML-defined catalog of pre-configured servers for one-click registration.
- **Roots**, **Metrics** (executions, latency, failure rates, top performers), **Version & Diagnostics** (protocol version, CPU/mem, readiness), **Teams + RBAC**, plugin management, OAuth config forms; import/export config; A2A agent registration.

**Praised:** federation + virtual-server composition; catalog UX; 1.0 RC2 shipped 30+ UI fixes (pagination, search/filter, team selectors) showing real investment.
**Weaknesses:** **Admin UI is officially a dev convenience — docs say disable it in production** (`MCPGATEWAY_UI_ENABLED=false` default); UI quality historically rough (the RC2 fix list is the complaint list: broken virtual-server editing, pagination, filters); Python perf ceiling; no LLM-gateway side (no model routing/cost screens); enterprise-IBM aesthetic and heavy config surface (300+ env vars).
**AX:** full Admin API mirrors UI (also flagged disable-in-prod); YAML catalog; being an MCP gateway, agents are first-class *consumers*, but admin-by-agent isn't a designed path.

---

## Cross-cutting synthesis

### Table stakes (every credible gateway dashboard has these; users assume them)
1. Virtual/API key CRUD with per-key budgets, rate limits, model allowlists, expiry.
2. Teams/orgs/users hierarchy with roles and per-team budget+model access.
3. Usage/spend analytics: cost+tokens+latency by key/team/model/provider over time.
4. Request log viewer with full request/response payloads and computed cost per call.
5. Provider/model management in UI without process restart; provider key vault.
6. Built-in chat playground to test a key/model/prompt.
7. Filterable everything (time, model, key, user, custom metadata/properties).
8. Routing config surfaced in UI (fallbacks, retries, load balancing).
9. SSO + RBAC + audit logs (even if enterprise-tier elsewhere, expected to exist).
10. Self-host parity: dashboard works offline/airgapped, dark mode, REST API parity for every UI action.
11. MCP server registration + per-entity tool permissions (post-2025 this moved from differentiator to expected — LiteLLM, Bifrost, ContextForge all have it).
12. Alerting hooks (Slack/webhooks) on spend/error thresholds.

### Differentiators worth stealing
1. **Portkey log→Replay→playground loop**: one click from any production log into an editable re-runnable playground.
2. **OpenRouter provisioning keys**: a restricted key class that can only mint/manage other keys — programmatic, agent-safe key issuance.
3. **OpenRouter public model catalog with live per-provider latency/throughput/uptime** + the rankings leaderboard (top-of-funnel asset).
4. **ContextForge virtual servers**: compose tools/resources/prompts into curated per-team MCP endpoints in UI.
5. **TensorZero's closed loop**: datasets-from-logs → evaluations → fine-tuning launched from the dashboard (logs as flywheel, not exhaust).
6. **Helicone HQL + custom properties**: a query language over logs and arbitrary user-defined dimensions that become filters everywhere; per-end-user cost analytics.
7. **Bifrost live WebSocket log streaming** in a single Go binary with zero-setup dashboard.
8. **Portkey versioned configs**: routing strategy as a versioned, ID-referenced object edited in UI, attached per-request.
9. **Helicone Sessions**: agent-trace grouping with session-type comparison.
10. **LiteLLM MCP toolsets + per-key/team MCP tool permissioning** UI.
11. **Portkey Admin API + admin MCP server**: the entire console is drivable from an MCP client.
12. **OpenRouter privacy/ZDR routing toggles** as a first-class user setting.

### Weaknesses & dark patterns to avoid
1. LiteLLM-style **enterprise nagware in OSS UI** (locked SSO/audit-log tabs, "premium feature" gates) — most-resented pattern in the space.
2. **Admin UI as RCE surface**: LiteLLM's MCP "test connection" executed user-supplied commands server-side; ContextForge ships its UI disabled-by-default for the same fear. Test/preview endpoints need sandboxing + strict authZ.
3. **List views that melt at scale** (LiteLLM 380k-key outage; perf regressions every few releases). Paginate/virtualize/server-filter from day one.
4. **Pricing on logged requests + retention** (Portkey) — punishes the observability you're selling.
5. **Config state invisible to the UI** (Helicone header-based caching/rate limits): runtime behavior that the dashboard can't show is a debugging trap.
6. **UI requiring a constellation of services** (TensorZero: ClickHouse + gateway + Postgres for auth; Helicone: ClickHouse + Kafka) vs Bifrost's single binary — setup friction decides adoption.
7. **No auth on the dashboard as default** (early TensorZero; ContextForge basic-auth admin) — gateways hold provider keys; the dashboard is a vault door.
8. **Cloud-only analytics** (pre-2026 Portkey, OpenRouter) — compliance buyers walk.
9. **Maintenance-mode/acquisition risk messaging** — Helicone's fate is now the #1 sales objection vs any VC-backed observability dashboard; OSS-forever posture is a feature.
10. Marketing benchmarks that don't reproduce ("50x faster") — earns distrust in technical communities.

### What the BEST gateway dashboard would include
- **One binary, one port**: gateway + dashboard + embedded store (SQLite/DuckDB) up in one command, ClickHouse optional for scale (Bifrost model, TensorZero schema discipline).
- **Three-pane core**: Keys/Teams/Budgets (LiteLLM's model, performant), live Logs with replay-to-playground (Portkey loop, Bifrost streaming), Analytics with custom properties + query language (Helicone).
- **Unified LLM+MCP governance**: one permission model where a key/team grants models AND MCP toolsets; virtual MCP servers composed in UI (ContextForge) sitting next to model routing config (Portkey configs-as-versioned-objects).
- **Model catalog with live health**: per-provider latency/uptime/price, machine-readable (OpenRouter).
- **Closed loop optional tier**: logs → datasets → evals → fine-tune (TensorZero) as a later module, not v1.
- **Agent-first control plane**: every screen = documented REST endpoint = MCP tool; provisioning-key class for agent-issued keys; config also expressible as a file for GitOps; `llms.txt` + OpenAPI shipped. The winning posture: *the dashboard is just one client of the admin API/MCP server.*
- **No enterprise gates in the UI**; auth-by-default; admin actions audited; test/preview endpoints sandboxed.

---

## Sources (primary)
- https://docs.litellm.ai/docs/proxy/ui ; /docs/proxy/ui_logs ; /docs/proxy/virtual_keys ; /docs/mcp ; /docs/proxy/enterprise ; /docs/troubleshoot/ui_issues
- https://github.com/BerriAI/litellm/issues/19477 , /issues/23005 , /issues/19921
- https://thehackernews.com/2026/06/litellm-flaw-cve-2026-42271-exploited.html ; https://horizon3.ai/attack-research/vulnerabilities/cve-2026-42271-chained-with-cve-2026-48710/
- https://github.com/maximhq/bifrost ; https://github.com/maximhq/bifrost/blob/dev/ui/README.md ; https://www.getmaxim.ai/bifrost
- https://portkey.ai/docs/product/observability/logs ; https://portkey.ai/features/prompt-management ; https://github.com/Portkey-AI/gateway (+ discussion #1576 open-sourcing) ; https://thenewstack.io/portkey-gateway-open-source/ ; https://www.truefoundry.com/blog/portkey-pricing-guide
- https://github.com/Helicone/helicone ; https://docs.helicone.ai/features/sessions ; /features/advanced-usage/user-metrics ; https://chatforest.com/reviews/helicone-llm-observability-gateway/ ; https://dev.to/stockyarddev/the-llm-proxy-landscape-in-2026-helicone-acquired-litellm-compromised-and-whats-next-3oon
- https://openrouter.ai/docs/faq ; /docs/features/provisioning-api-keys ; /docs/cookbook/administration/organization-management
- https://github.com/tensorzero/tensorzero ; https://www.tensorzero.com/docs/deployment/tensorzero-ui ; /docs/operations/set-up-auth-for-tensorzero ; issue #5062
- https://ibm.github.io/mcp-context-forge/overview/ui-concepts/ ; /manage/catalog/ ; https://github.com/IBM/mcp-context-forge (SECURITY.md, discussion #3548 RC2)
