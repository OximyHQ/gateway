# Dimension Deep-Dive: Governance & Multi-Tenancy in AI Gateways

Research date: 2026-06-10. Scope: virtual keys, org/team/project hierarchies, budgets, rate limits, quotas, model allowlists, key rotation/expiry, JIT provisioning, SSO/SCIM, RBAC, audit logs — across the AI-gateway competitive landscape (LiteLLM, Portkey, Bifrost/Maxim, Kong AI Gateway, TrueFoundry, Helicone, OpenRouter, Cloudflare AI Gateway, Envoy AI Gateway, Higress, and MCP-gateway players).

---

## 1. Landscape summary

Governance is the single most enterprise-gated dimension in AI gateways. The pattern across every vendor:

- **OSS / free tier**: virtual keys, basic budgets, RPM/TPM limits, model allowlists.
- **Enterprise tier (paywalled)**: orgs (vs. just teams), RBAC beyond admin/member, SSO, SCIM, audit logs, scheduled key rotation, model-specific budgets, tag-based budgets.

LiteLLM defines the de-facto vocabulary (virtual keys, teams, `max_budget`, `tpm_limit`/`rpm_limit`) that everyone else either copies (Bifrost) or maps onto their own hierarchy (Portkey workspaces, Kong consumers). The biggest live weakness in the market is **enforcement reliability**: LiteLLM has a long trail of open/closed GitHub issues where budgets simply weren't enforced. A new gateway that makes enforcement *provably correct* (atomic, tested, fail-closed) and ships SSO/SCIM/audit in OSS has a clear wedge.

---

## 2. Per-product governance surface

### 2.1 LiteLLM (Python, MIT core + paid Enterprise features in same repo)

The reference implementation; richest single feature surface.

**Hierarchy**: Organization (Enterprise) → Team (OSS) → User → Virtual Key. Three key types: user-only (deleted with the user), team service-account (`team_id` only, survives personnel changes), user+team (individual accountability inside a team budget). Org-level tenant isolation: orgs cannot see each other's keys/data; budgets cascade downward and cannot exceed parent limits. Docs recommend org-per-customer for SaaS multi-tenancy and team-per-environment (prod/staging/dev).

**Virtual keys** (`/key/generate`): `models` allowlist, `aliases` (model name remapping — silently upgrade/downgrade a key's model), `duration` (relative expiry, "30min"/"30d"), `expires` (absolute timestamp), `max_budget`, `budget_duration`, `max_parallel_requests`, `tpm_limit`, `rpm_limit`, `metadata`, `tags` (cost attribution), `auto_rotate` + `rotation_interval` (Enterprise), guardrails per key. Admin can set **upperbound key-generate params** (caps on what any generated key may request) and **default params**. Custom `custom_generate_key_fn` hook. Keys can be blocked/unblocked (`/key/block`). Custom auth header name supported.

**Budgets** — most granular in market:
- Per key, per user, per team, per team-member-within-team (`max_budget_in_team`), per org, global proxy budget, per end-customer (`/budget/new` + `/customer/new` — budget the `user` field of requests without issuing keys), model-specific budgets per key (`model_max_budget`, Enterprise), tag-based budgets (Enterprise), agent/session budgets (`max_budget_per_session`, `max_iterations`).
- `budget_duration` resets: seconds → days; multiple concurrent windows per key ($10/day AND $100/month). Reset checker runs every ~10 min. Temporary budget increases with expiry.
- Exceed → HTTP 400 `ExceededBudget`. Precedence rule: team key ⇒ team budget applies, not personal.

**Rate limits**: TPM (count total/input/output via `token_rate_limit_type`), RPM, max parallel requests — at key/user/team/customer level; per-model RPM/TPM on keys and teams (`model_rpm_limit`); session-level agent limits. 429 on exceed; `x-litellm-key-remaining-*` headers. Multi-instance sync via Redis (in-memory cache synced every 10ms; documented drift "at most ~10 requests at high traffic").

**RBAC**: Proxy Admin, Proxy Admin Viewer, Internal User, Internal User Viewer (deprecated); Org Admin and Team Admin (Premium). Granular per-endpoint team-member permissions (which key endpoints a member may call). JWT/OIDC auth with JWT→virtual-key field mapping; service accounts; IP allowlisting; CLI auth.

**SSO/SCIM**: Admin-UI SSO (Enterprise). Full SCIM v2 server (Enterprise, since v1.67.0): IdP (Okta/Entra/OneLogin) pushes users + groups→teams; deprovisioning a user auto-deletes all their keys. Documented constraint: **JIT and SCIM are mutually exclusive** — you pick one.

**Audit logs**: Enterprise feature; logs admin actions on keys/teams/users.

**Weaknesses (documented, high-signal)**:
- Repeated budget-bypass bugs: #24770 (budgets bypassed when model name not `provider/model` format), #11083 (end-user budget not blocked), #26672 (key/user max_budget not enforced in v1.82.3), #27381 (global `_PROXY_MaxBudgetLimiter` instantiated but never registered → global budget silently never enforced), #10750 (budgets not enforced on pass-through routes), #12905 (user budget not enforced for team keys), #14097 (confusing budget/limit precedence), #19781 (can't reset a budget back to unlimited).
- Governance config sprawl: yaml + DB + UI three-way state; many features marked ✨ Enterprise inside OSS docs creates a "paywall maze."
- Python proxy performance is the standard critique (everyone benchmarks against it; Bifrost claims "50x faster").

### 2.2 Portkey (Gateway OSS/MIT in TS; governance plane is the SaaS/Enterprise product)

**Hierarchy**: Account → Organization(s) → Workspace(s) (workspaces = Enterprise). Workspace roles: Manager / Member; org-level Owners/Admins.

**Keys**: Two API-key classes — org-level **Admin API keys** and **Workspace API keys**, each typed as *Service* or *User* keys. Provider creds vaulted as "virtual keys"/integrations (rotate/revoke/monitor without touching apps); model catalog per workspace.

**Budgets & rate limits**: per API key and per workspace; cost-based (min $1) or token-based; **alert thresholds that notify before blocking**; reset = none / weekly (Sun 00:00 UTC) / monthly (1st); rate limits request- or token-based per minute/hour/day. Enforced at ingress before the provider call. Notifications go to org admins/owners + key creator + arbitrary emails (finance, dept heads).

**Distinctive**: org-mandated **metadata schemas** (JSON Schema draft-07) — owners define required metadata fields that every request/key/workspace must carry, with precedence workspace > API key > request. This is governance-by-attribution: nothing untagged gets through.

**Enterprise**: SSO, SCIM (org roles, workspace memberships, workspace roles via SCIM groups), audit logs across every resource action, AWS KMS, JWT auth, multi-workspace, data residency, SOC2/GDPR/HIPAA. "Usage policies" enforce request/token/cost-level rules at runtime.

**Weaknesses**: nearly all governance lives in the hosted/enterprise control plane, not the OSS gateway repo; the OSS gateway alone has no teams/budgets/RBAC. Two-tier key model (admin vs workspace) is less granular than LiteLLM's per-user keys-with-team-budgets. Pricing opacity for enterprise tier is a recurring complaint in comparison blogs.

### 2.3 Bifrost / Maxim (Go, Apache-2.0 core + Enterprise)

The fastest-moving challenger; explicitly markets governance.

**Hierarchy & budgets**: Customer → Team → Virtual Key → Provider budget cascade; **all levels must pass; cost deducts from every level simultaneously** on each transaction. Costs auto-derived from a built-in model catalog.

**Virtual keys (OSS)**: model filtering, provider control, budgets, token+request rate limits (reset windows 1 min–1 month), binding a VK to specific provider keys (dev/staging/prod separation), instant enable/disable. Multi-format key headers (accepts `sk-bf-*`, OpenAI `Authorization: Bearer`, Anthropic `sk-ant-*` style, Gemini `x-goog-api-key`) so existing SDKs work unmodified.

**MCP governance (OSS — notable)**: per-virtual-key **MCP tool allowlists with wildcards** and automatic header generation — LLM gateway and MCP gateway governance unified under one key object. Plus "required headers" enforcement (reject 400 if tenant/audit headers missing) for tenant isolation.

**Enterprise**: RBAC (Admin/Developer/Viewer + custom roles), SSO via OIDC (Okta, Entra, Zitadel, Keycloak) + SAML, **automatic role assignment from IdP groups (highest-privilege wins)**, **team auto-creation from IdP groups** (Business Unit → team → user hierarchy sync), user-level governance/budgets, audit logs, vault integration (HashiCorp/AWS/Azure), SOC2/HIPAA/GDPR/ISO27001.

**Config surface**: Web UI, REST API (`/api/governance/*`), declarative `config.json` (GitOps), CLI. Performance claim: 11–100µs overhead at 5k RPS, "50x faster than LiteLLM."

**Weaknesses**: no documented key rotation or SCIM; audit logs and any real RBAC are enterprise-only; young product — much of its visibility is its own SEO content farm (getmaxim.ai articles ranking for every comparison query); smaller community track record on enforcement correctness.

### 2.4 Kong AI Gateway (Lua/Go plugins on Kong; OSS core + Enterprise plugins)

Governance expressed through Kong's existing API-management primitives — **consumers and consumer groups**, not AI-native teams/users.

- **AI Rate Limiting Advanced plugin**: limit dimensions = consumer, consumer group, IP, header, path, **model**, **provider**, combinable with AND logic. Token strategies: `total_tokens` / `prompt_tokens` / `completion_tokens` / **`cost`** (computed `(prompt×in_cost + completion×out_cost)/1M` — i.e., true dollar-denominated rate limiting). Custom token-counting function supported. Storage strategies: local / cluster / redis. Returns `X-AI-RateLimit-{Limit,Remaining,Reset,Retry-After,Query-Cost}` headers; 429 on exceed.
- Kong 3.14 (2026): "precision token budgets at scale," per-**agent** token attribution for cost showback; Kong Agent Gateway extends governance to MCP/agent traffic.
- Identity (OIDC, mTLS, key-auth), RBAC over admin API, audit logging, SSO — all mature Kong Enterprise features inherited for free.

**Weaknesses**: no native concept of LLM "teams/users/virtual keys with budgets" — you assemble it from consumers + plugins; per-key spend ledgers/budget periods are rate-limit-windows, not accounting budgets; Enterprise pricing is famously high; heavy operational footprint (DB-backed control plane) for teams that just want an LLM proxy.

### 2.5 TrueFoundry (Go gateway; commercial, enterprise-focused)

- Token types: **Personal Access Tokens** (individuals) vs **Virtual Accounts** (apps/shared tooling, admin-scoped and revocable).
- **Rule-based rate-limit/quota engine**: policies target `user:john`, `team:engineering`, `virtualaccount:va-james`, models, and arbitrary request metadata; request-, token-, or **cost-based quotas**; sliding-window token bucket, per-minute minimum window.
- Hard budget enforcement per team/service/endpoint — requests stop pre-provider when exhausted.
- Enterprise: SSO, RBAC, OAuth2 tool policies (agent gateway), audit logging, centralized API keys.

**Weakness**: closed-source control plane; governance config is platform-coupled (it's an MLOps platform first); less community validation.

### 2.6 Helicone (Rust gateway, MIT; observability-first)

- Rate limits via **custom properties** (e.g., `Helicone-Property-Organization` header → per-org request/hour policies; `Helicone-User-Id` → per-user limits) and cost/request caps configured in dashboard.
- Gateway is new (2025): RBAC hard enforcement, SCIM, workspace isolation, per-team budget *blocks*, audit trails are absent or enterprise-contact-only — even sympathetic comparisons say its governance "is still maturing."
- Notable: governance-by-header (caller self-declares tenant) is easy to adopt but trivially spoofable without signed keys — a cautionary pattern.

### 2.7 OpenRouter (closed SaaS marketplace)

- Per-API-key **spending caps, usage counters, reset behavior, BYOK accounting, activity attribution**; key status/credits introspectable via `GET /api/v1/key`; **Provisioning API keys** let programs mint/limit/revoke runtime keys (the agent-friendly bit).
- Org features: budget separation, model governance, **provider allowlists**, ZDR enforcement org-wide (enterprise).
- Weaknesses: no self-host, no team/user hierarchy depth, no SCIM/RBAC story, 5%+ credit fee; governance limited to key-level caps.

### 2.8 Cloudflare AI Gateway / Envoy AI Gateway / Higress

- **Cloudflare**: edge observability + caching + basic rate limiting; comparisons explicitly note **no budget enforcement** and no routing logic — governance-light.
- **Envoy AI Gateway** (OSS, CNCF orbit, Go/Envoy): usage-based rate limiting on token counts; per-client API key injection; per-tenant spend caps and showback/chargeback are the headline use case; config via K8s CRDs (Gateway API). No dashboard, no SCIM/RBAC of its own — assumes platform team owns it.
- **Higress** (Alibaba, Apache-2.0, Istio/Envoy + Wasm plugins): multi-tenant key management, quota plugins, enterprise governance posture; strongest in China-cloud ecosystems.

### 2.9 MCP-gateway-specific governance (adjacent market)

- Pattern emerging in 2026: a credential carries **its own tool allowlist + budget + rate limits** per customer integration (agentic-community/mcp-gateway-registry with Keycloak/Entra OAuth; MintMCP; Lunar.dev MCPX; IBM Context Forge).
- AWS Bedrock AgentCore uses **Cedar policies** down to tool-parameter constraints; Azure APIM governs MCP tools via gateway policy.
- Bifrost is the only LLM gateway shipping per-key MCP tool filtering in OSS today — validates the "one key governs models AND tools" design.

---

## 3. Feature-by-feature market matrix (condensed)

| Capability | LiteLLM | Portkey | Bifrost | Kong | TrueFoundry | Helicone | OpenRouter |
|---|---|---|---|---|---|---|---|
| Virtual keys | OSS, richest | vaulted provider keys + 2-tier API keys | OSS, multi-format | consumers+key-auth | PAT + virtual accounts | header-based props | keys + provisioning API |
| Hierarchy | Org→Team→User→Key (org=Ent) | Org→Workspace | Customer→Team→VK→Provider | consumer groups | user/team/va | org via headers | org (flat) |
| Budgets | per key/user/team/org/customer/model/tag/session | per key/workspace, cost or token | 4-level cascade, simultaneous deduction | cost-windowed rate limits | cost/token quotas via rules | cost caps | per-key caps |
| Budget reset | s/m/h/d windows, multi-window | none/weekly/monthly | 1min–1month | window-based | sliding window | — | reset behavior on key |
| Rate limits | RPM/TPM/parallel, per-model | req/token per min/hr/day | token+request | 6 dimensions, AND-combinable, $-cost strategy | rule engine incl. metadata | per-property | per-key |
| Model allowlist per key | yes + aliases | model catalog/workspace | yes + provider pinning | model-dimension policies | rule targets | no | provider allowlist (ent) |
| Rotation/expiry | duration, absolute expiry, regenerate w/ grace, scheduled auto-rotate (Ent) | rotate vaulted creds | enable/disable only | via Kong key-auth | revoke | — | revoke/provision |
| SSO | Ent | Ent | Ent (OIDC+SAML) | Kong Ent | yes | Ent | n/a |
| SCIM | Ent, full v2, dedprovision deletes keys | Ent (groups→roles) | not documented | via Kong Ent | — | no | no |
| JIT provisioning | yes (XOR with SCIM) | via SSO | IdP group→role/team auto | — | — | — | n/a |
| RBAC | 6 roles + per-endpoint member perms | org+workspace roles | 3 roles + custom (Ent) | Kong admin RBAC | RBAC | Ent only | none |
| Audit logs | Ent | Ent, every resource | Ent | Kong Ent | yes | immature | no |
| MCP tool governance | MCP gateway w/ per-key perms | MCP client | per-VK tool allowlist (OSS) | Agent Gateway | OAuth2 tool policies | no | no |

---

## 4. What "table stakes" means for this dimension (synthesis)

Every credible gateway has: virtual keys with model allowlists; key-level + team-level USD budgets with periodic reset; RPM/TPM/concurrency limits returning 429 + remaining-quota headers; spend tracking per key/user/team; key expiry and revocation; admin/member role split; Redis-backed limit sync for multi-instance; an admin REST API for all of it.

## 5. Differentiators worth stealing

1. **Bifrost's cascade-with-simultaneous-deduction** (Customer→Team→VK→Provider, all must pass, one ledger event debits every level) — cleanest budget mental model.
2. **Bifrost's per-virtual-key MCP tool allowlist (wildcards) in OSS** — one key object governs models AND tools; nobody else ships this open.
3. **Kong's dollar-cost rate-limit strategy** ($/window as a first-class limit unit, not just tokens) + multi-dimensional AND-combinable policies.
4. **LiteLLM's key aliases** (transparent model upgrade/downgrade per key) and **upperbound key-generate params** (meta-governance: caps on what limits sub-admins can grant).
5. **LiteLLM's end-customer budgets without keys** (`/customer/new` budgets the request's `user` field) — multi-tenant SaaS chargeback without key sprawl.
6. **Portkey's mandatory metadata schemas** (JSON-Schema-validated required attribution on every key/request) — governance-by-attribution.
7. **Portkey's alert-before-block thresholds** with finance-team notification routing.
8. **LiteLLM SCIM deprovision → automatic key deletion** (identity offboarding kills access atomically).
9. **Bifrost IdP-group→team auto-sync** and highest-privilege role mapping.
10. **TrueFoundry's single rule engine** where one policy grammar covers users/teams/virtual-accounts/models/metadata for both rate limits and quotas.
11. **Scheduled key auto-rotation with grace periods** (LiteLLM Enterprise) — rare; ship it OSS.
12. **OpenRouter's Provisioning API** — keys minting keys, designed for programmatic/agent consumption.

## 6. Common weaknesses / gaps to exploit

1. Enforcement unreliability: LiteLLM's documented budget-bypass bugs (never-registered budget hook #27381, format-dependent bypass #24770, pass-through bypass #10750, version regressions #26672). Fail-open is the industry default; **fail-closed + property-tested enforcement is open ground**.
2. SSO/SCIM/RBAC/audit are paywalled everywhere — an OSS gateway shipping them free flips the table.
3. Budget precedence is confusing in every product (LiteLLM #14097, #12905: team vs user vs customer vs key — undocumented interactions, JWT path ignores customer budgets).
4. No product unifies LLM + MCP governance under one tenancy model except Bifrost (partially, enterprise-gated for RBAC/audit).
5. Rotation is nearly absent (only LiteLLM Enterprise does scheduled rotation; Bifrost has none).
6. Config state split across YAML/DB/UI (LiteLLM) or SaaS-only control planes (Portkey/TrueFoundry) — no one does clean GitOps + UI + API parity except Bifrost's claim.
7. JIT-vs-SCIM forced choice (LiteLLM) is an artificial constraint.
8. Header-declared tenancy (Helicone) is spoofable; budgets-as-rate-limit-windows (Kong) aren't real accounting ledgers.

## 7. Agent-experience (AX) observations

- LiteLLM: full management REST API (`/key`, `/team`, `/user`, `/org`, `/budget`, `/customer`) — everything the UI does is API-doable; plus it *is* an MCP gateway with per-key MCP permissions. No first-class MCP control-plane ("agent manages the gateway") surface though.
- Bifrost: 4-way parity (UI / REST `/api/governance/*` / declarative config.json / CLI) is the best AX story; virtual keys accept native OpenAI/Anthropic/Gemini header formats so agent SDKs need zero changes.
- OpenRouter Provisioning API is explicitly built for programs minting scoped keys at runtime — the pattern agents need (ephemeral, capped, attributed credentials per agent run).
- Envoy AI Gateway: CRD/GitOps-only — machine-readable but K8s-coupled, no dashboard.
- Gap nobody fills: an MCP server exposing the gateway's own control plane (create key, set budget, read spend) so an orchestrating agent can self-provision scoped sub-keys with attenuated budgets. Closest analogues: OpenRouter provisioning keys + LiteLLM session budgets. This is the agent-first differentiator.

## 8. Recommendations for our gateway

1. One canonical entity graph: Org → Team/Project → Principal (human|agent|service) → Key, with budgets/limits attachable at every node and **deterministic, documented precedence** (publish the resolution algorithm).
2. Fail-closed enforcement core: atomic ledger (single source of spend truth), property-tested invariants ("no request proceeds if any ancestor budget exhausted"), enforcement on *all* routes including pass-through.
3. Ship SSO (OIDC), SCIM v2 (with key-revocation-on-deprovision), RBAC, and audit log in OSS — it's the market's biggest paywall resentment.
4. Per-key governance object covers models AND MCP tools (allowlist w/ wildcards) AND budget AND limits — one grammar.
5. Dollar-cost as a first-class limit unit (Kong-style) alongside RPM/TPM/concurrency; multi-window budgets (daily AND monthly).
6. Key lifecycle: TTL, absolute expiry, regenerate-with-grace, scheduled rotation — all OSS, all API-driven.
7. Agent-first control plane: provisioning-key pattern + MCP server for the gateway admin API, so agents mint attenuated child keys (budget ≤ parent remaining, models ⊆ parent allowlist, auto-expiring).
8. GitOps parity: declarative config file ⇄ DB ⇄ UI ⇄ CLI ⇄ MCP, no three-way state drift.

## Sources (primary)

- https://docs.litellm.ai/docs/proxy/virtual_keys · /proxy/users · /proxy/access_control · /proxy/multi_tenant_architecture · /tutorials/scim_litellm · /proxy/team_budgets
- LiteLLM GitHub issues: #24770, #11083, #22726, #10750, #26672, #27381, #14097, #12905, #19781, #28750
- https://portkey.ai/docs/product/administration (metadata enforcement, budget/rate-limit pages) · /product/enterprise-offering/org-management
- https://www.getmaxim.ai/bifrost/resources/governance · github.com/maximhq/bifrost
- https://developer.konghq.com/plugins/ai-rate-limiting-advanced/ · konghq.com Kong AI Gateway 3.14 / Agent Gateway posts
- https://docs.truefoundry.com/docs/ai-gateway/ratelimiting · truefoundry.com/ai-gateway
- https://docs.helicone.ai/features/advanced-usage/custom-rate-limits
- https://openrouter.ai/docs/api/reference/limits
- https://aigateway.envoyproxy.io/docs/ (usage-based rate limiting)
- MCP governance: github.com/agentic-community/mcp-gateway-registry, dxheroes.io MCP governance landscape 2026, learn.microsoft.com Foundry MCP governance
