# Obot (Acorn Labs / Obot AI) — Competitive Intelligence Report

**Category:** MCP gateway / enterprise MCP platform (hosting + registry + gateway + chat client + LLM gateway)
**Research date:** 2026-06-10
**Sources:** github.com/obot-platform/obot, docs.obot.ai, obot.ai (marketing + release blogs), PR Newswire/SiliconANGLE funding coverage, TrueFoundry/MintMCP competitor analyses, HN.

---

## 1. Company & Positioning

- **Obot AI** (formerly **Acorn Labs**; rebranded ~Aug 2025 when the MCP Gateway launched). CEO **Sheng Liang** — founding team previously built **Rancher Labs** (→SUSE) and **Cloud.com** (→Citrix). Strong open-source-to-enterprise playbook DNA.
- **$35M seed** (Sept 2025), co-led by **Mayfield Fund** and **Nexus Venture Partners** — one of the largest seeds in the MCP infra space.
- Tagline evolution: "Complete MCP Platform — Hosting, Registry, Gateway, and Chat Client." Positioning is **IT/enterprise control plane for MCP adoption**, not a developer LLM gateway. Pitch: AI clients (Claude, Cursor, ChatGPT, internal agents) → Obot → any MCP server (local, remote, hosted), with OAuth enforcement, access policies, audit of every call, and a curated catalog.
- Sister OSS project: **Nanobot** — "MCP Agent Framework" (Apache 2.0, Go, ~1.3k stars): an MCP host that combines MCP servers + LLM + context into an agent; YAML/markdown agent definitions; voice/SMS/email/Slack surfaces; alpha quality. Nanobot now powers Obot's chat runtime (legacy chat was removed in v0.22).
- Also ships **Discobot** — tooling to run multiple coding agents simultaneously (blog-level, newer).

## 2. Open Source / Licensing / Language

- **MIT licensed**, repo `obot-platform/obot` (~821 stars, 174 forks, 95 releases as of research date — modest stars relative to funding).
- **Implementation:** Go (~56%), Svelte UI (~36%), TypeScript (~8%). Nanobot: Go (84%) + Svelte.
- **Enterprise Edition** (commercial): advanced IdP (SAML/OIDC — Okta, Microsoft Entra), SLAs, priority support. No public pricing; contact sales. **Obot Cloud**: managed hosted gateway with 14-day free trial.
- Docs versioned per release (v0.13 → v0.22 current, v0.23 in RC) — rapid ~monthly release cadence.

## 3. Architecture

Four core components:

1. **MCP Hosting** — runs MCP server workloads on **Docker or Kubernetes** backends (`OBOT_SERVER_MCPRUNTIME_BACKEND`). Supports Node (npx), Python (uvx), and containerized servers; single-user STDIO and multi-user HTTP servers.
2. **MCP Registry** — curated index of MCP servers + run metadata; **conformant with the MCP registry specification**; multiple registries with per-registry visibility.
3. **MCP Gateway** — reverse-proxy passthrough: authenticates user against IdP, ensures target server is running, forwards traffic without modifying MCP protocol.
4. **Obot Agent (Chat)** — native MCP chat client (Nanobot runtime).

Key architectural construct — the **MCP Server Shim**: a protocol-aware sidecar running next to every MCP server that handles **authorization, audit logging, webhook filters, and OAuth token exchange (RFC 8693)**. Security property: token-exchange credentials live in the shim, never exposed to the MCP server. On K8s: server + shim + converters in one pod over localhost; on Docker: via host.docker.internal. (v0.15 was a deliberate "gateway re-architecture" slimming the gateway to auth + deployment validation + proxying, pushing protocol logic into the shim.)

**LLM Gateway** (newer, documented in v0.23 docs): a proxy between chat clients and LLMs for monitoring/control; admin configures which LLM providers Obot Chat can use; Message Policies enforce at this layer.

**Persistence/infra:** PostgreSQL (SQLite for local dev) for config/metadata; audit logs to disk or S3 (mode: off/disk/S3, default 90-day retention); artifact/workflow storage to S3/GCS/Azure Blob/local; credential encryption via AWS KMS/GCP/Azure or custom provider; built-in rate limits (100 rps unauthenticated / 200 rps authenticated defaults) and per-user daily token limits. No explicit HA/clustering story in config reference.

## 4. Full Feature Surface

### Gateway / access
- Single endpoint per server: `https://<instance>/mcp-connect/{server-id}`; **all servers exposed as streamable-http regardless of underlying runtime** (STDIO→HTTP conversion).
- User/group access rules per server; **fine-grained per-tool access control** (expose only chosen tools per group, v0.13).
- **Composite MCP servers** (v0.13): virtual servers aggregating tools from multiple servers, with tool rename/description override and separate ACLs per composite — "fine-grained tool RBAC without exposing entire servers."
- **API keys** (v0.16): user-tied machine-to-machine keys, scoped to specific servers or all, optional expiry.
- Request/response **filters**: two kinds — (a) **MCP filter servers** (any runtime, designated filter tool) and (b) **HTTP webhook filters** (auto-wrapped as MCP servers beside the shim; HMAC-SHA256 signed via `X-Obot-Signature-256`). Filters can **accept / reject / mutate** messages; selector targeting by method/tool/URI. Use cases: DLP, guardrails, content policy.
- **OAuth**: centralized OAuth 2.1; token brokering; RFC 8693 token exchange in the shim; **OAuth inspector** for debugging remote-server auth (v0.22).

### Hosting / runtimes
- Server types: **single-user** (per-user instance + personal creds), **multi-user** (shared creds, org-level monitoring, user-defined headers), **remote** (external streamable-HTTP endpoints), **composite**.
- Runtimes: npx, uvx, containerized, remote. Per-server `startupTimeoutSeconds` (v0.22). K8s resource requests/limits, affinity/tolerations, pod annotations, scheduling policies, pod security standards; **capacity dashboard** of requested CPU/RAM across MCP servers (v0.16). **Image pull secrets** (Docker/GHCR/ECR) and **external secrets** (bind env vars to K8s Secrets) in v0.22.

### Registry / catalog
- Curated internal catalog ("84+ verified MCP servers" pre-loaded: Slack, GitHub, Notion, Microsoft, Postgres, Atlassian, Outlook, MongoDB, Salesforce…); one-click connect; shared credentials; visibility scoping per user/group; multiple registries; Power User+ can publish to registries.
- **GitOps**: point Obot at Git repos (GitHub/GitLab/self-hosted, branch + PAT support) of YAML server definitions — metadata, tool previews, env var schema w/ sensitivity flags, K8s resources, secret bindings, runtime config; deterministic connect-URLs across syncs. Limitation: GitOps currently **only supports singleUser servers**; secret bindings only on the K8s backend.

### Governance / identity
- Roles: **Basic User → Power User (publish personal servers) → Power User+ (share via registries) → Admin**; plus an **Auditor** role (read-only incl. API keys, group assignments, capacity). Default role for new users configurable.
- Auth providers: Google, GitHub OOTB; **Okta, Entra, SAML/OIDC in Enterprise**; Auth0, JumpCloud mentioned on marketing page.
- **Model access policies** (v0.16): who can use which LLM models (users/IdP groups/all; supports default-model aliases).
- **Message policies**: natural-language content rules enforced at the LLM proxy on user messages and/or tool calls; subject matching → two-stage LLM review (small fast model screen, full model adjudication w/ user-facing explanation); **fails closed**.
- **Skill access policies** (v0.22): same policy model applied to agent skills.

### Audit / observability
- Audit log of every tool call through gateway: user, agent/client, server, tool, arguments, outcome; real-time query in UI; **export one-time or scheduled to S3/GCS/Azure Blob/S3-compatible** with pre-filtering (v0.13); retention default 90 days.
- Usage analytics dashboard: most-used tools/servers, response times, per-user activity.
- No OpenTelemetry export, no tracing, no token/cost analytics documented (competitors call this out).

### Chat client (Obot Agent)
- MCP-native chat: conversations, projects (shareable configs, v0.11 "project sharing"), built-in RAG/knowledge, project-wide memory, **scheduled tasks/workflows** (recurring or on-demand), workflow sharing/publishing (artifact storage).
- Model providers: OpenAI, Anthropic, Azure OpenAI (API key or Entra service principal w/ deployment auto-discovery), Amazon Bedrock, plus a **generic OpenAI-Responses-compatible provider** (works with Ollama, LiteLLM, any OpenAI-compatible gateway — replaced the dedicated Ollama provider in v0.22). Default-model aliases per task type (chat / chat-fast / embeddings).

### Agent-device fleet features (v0.22 — notable strategic turn)
- **`obot` CLI** for end users on their laptops:
  - `obot setup` — OAuth login to an Obot instance; detects coding agents using the `~/.agents` skills convention (Cursor, VS Code, Codex, etc.) plus Claude Code; installs an `obot` skill (teaches the agent how to use the CLI) and `obot-scan` / `obot-skills-install` / `obot-skills-search` slash commands.
  - `obot scan` — walks the home dir and inventories **every installed AI client** (Claude Code, Claude Desktop, Codex, Cursor, Goose, Hermes, Openclaw, Opencode, VS Code, Windsurf, Zed), the MCP servers each is configured with, and skills/plugins on disk → fleet-wide **device visibility dashboard** for admins.
  - `obot skills` — install admin-curated, policy-gated skills into local coding agents.
- This is "MDM for AI clients" — central skill catalog + device inventory + policy, distributed via CLI into local agents.

### Admin UX / misc
- Modern Svelte admin UI; role-aware unified UX (v0.15); **custom branding** (colors/logos/icons, v0.15); update notifications; guided first-run setup for auth + model providers; Kubernetes Helm chart; Docker single-container quickstart (bootstrap token + docker.sock); user dashboard for basic users (v0.23-rc).

## 5. Deployment & Config Model

- **Docker**: `docker run` w/ env vars, port 8080, docker socket mount. **Kubernetes**: Helm chart (production-recommended). No single-binary story — needs Postgres + (for hosting) Docker/K8s; SQLite only for local play.
- Config = env vars (`OBOT_SERVER_*`) + admin UI + GitOps YAML for server catalog. Bootstrap token for first login; authentication itself is **optional/flag-gated** (`OBOT_SERVER_ENABLE_AUTHENTICATION`) — and MCP auth gets disabled when Obot auth is off (v0.22.1 fix).
- Encryption providers AWS/GCP/Azure KMS; artifact storage pluggable; rate-limit and token-quota env vars.

## 6. API surface / extensibility

- Control plane operable via **Web UI, CLI, or GitOps**; REST API exists (the UI consumes it) but there is **no polished public API reference doc** — GitOps YAML is the blessed "as-code" path, and it covers only a subset (singleUser servers).
- Extensibility points: webhook filters (HTTP), MCP filter servers, custom model providers (OpenAI-compatible), custom encryption provider, custom auth providers (enterprise), registry spec conformance.

## 7. Performance

- **No published performance numbers** (no latency/throughput benchmarks anywhere in docs/blog). Competitor TrueFoundry characterizes Obot as "moderate latency (tens of ms per call)" vs their sub-3ms claim — hostile source, unverified, but Obot publishes nothing to rebut it.
- Built-in rate limiting defaults (100/200 rps) hint at modest expected scale per instance. Known past issue: npx servers building from source timed out under default startup timeout (fixed by per-server `startupTimeoutSeconds`).

## 8. Weaknesses & complaints

- **Heavy footprint**: needs Postgres + Docker/K8s + (ideally) KMS + object storage. No single-binary, no edge/lightweight mode. Kubernetes-centric design; competitor analyses cite "limited on-prem/air-gapped flexibility."
- **Observability shallow**: request logs + usage counts; **no OTel export, no distributed tracing, no token/cost analytics** on the MCP path.
- **No LLM gateway for general traffic**: the "LLM Gateway" only proxies Obot Chat ↔ models; it is not a unified OpenAI-compatible gateway for arbitrary apps. Scope is MCP-only (TrueFoundry's core attack: "focused only on MCP servers, not unified LLM + MCP management").
- **RBAC relatively coarse** until recently; policy engine is bespoke (no OPA/cedar); message policies rely on LLM judging (latency + cost + nondeterminism; fails closed = availability risk).
- **GitOps incomplete**: singleUser-only, K8s-only secret bindings, remote servers header-bindings-only.
- **No public API reference**; automation beyond GitOps requires reverse-engineering the UI API.
- **SAML/OIDC paywalled** in Enterprise (OSS gets Google/GitHub) — a classic open-core friction point for enterprise OSS users.
- **Churn/breaking changes**: legacy chat removed wholesale in v0.22; Helm chart params removed; gateway re-architected in v0.15; Ollama provider dropped — fast-moving, unstable surfaces.
- Quality bugs visible in tracker (e.g., audit logs showing "unknown user" entries after nanobot chat, issue #5907).
- **Low community traction relative to capital**: ~821 stars / 1 comment on the HN launch thread; mindshare battle vs LiteLLM/Portkey/Kong/MintMCP etc.
- Auth optional-by-default in quickstart paths; bootstrap-token model invites insecure deployments.

## 9. Agent Experience (AX) notes — how agents use it

- Strongest AX idea in the space: the **`obot` CLI installs a skill INTO the coding agent that teaches the agent to use Obot itself** (search/install skills, scan devices) — the gateway recruits the agent as its own operator.
- External agents/clients connect via one stable URL pattern (`/mcp-connect/{server-id}`), always streamable-http, with central OAuth handled by the gateway — agents never hold downstream credentials (token exchange in the shim).
- API keys scoped per server enable headless/m2m agent access; skills + skill policies create a governed distribution channel for agent capabilities; `obot scan` gives admins inventory of agent fleets.
- Config-as-code via GitOps YAML, but no full admin REST/MCP control-plane API exposed for agents to administer the gateway itself.

## 10. What to steal / counter-position

**Steal:**
1. Shim/sidecar pattern isolating credentials + audit + filters from both client and server.
2. Composite servers + per-tool RBAC + tool rename/description override (virtual server composition).
3. RFC 8693 token exchange for downstream OAuth without gateway changes.
4. `obot scan`/`obot setup`/skills — CLI that enrolls local coding agents and inventories AI client fleets.
5. Accept/reject/**mutate** webhook filter contract with HMAC signing and selector targeting.
6. Scheduled audit export to object storage; registry-spec-conformant catalog; GitOps YAML for server defs.
7. Natural-language message policies w/ two-stage LLM evaluation (idea, if not implementation).

**Counter-position (their gaps a new single-binary unified LLM+MCP gateway can win on):**
- Single binary, zero-dependency (they need Postgres+K8s/Docker); air-gap friendly.
- Unified LLM gateway + MCP gateway in one (they only proxy their own chat's LLM traffic).
- Real observability: OTel traces, token/cost accounting per tool call and per model call.
- First-class public API + MCP control plane (manage the gateway via MCP), vs UI-first + partial GitOps.
- Published benchmarks (they have none).
- Full SSO in OSS to undercut the open-core IdP paywall.
