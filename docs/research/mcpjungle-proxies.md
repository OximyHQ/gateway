# Competitive Intel: MCPJungle + the mcp-proxy family (sparfenyuk, TBXark, mcgravity, Unla/AmoyLab)

Category: Self-hosted MCP registries/proxies — server registration, tool discovery, ACLs, transport bridging (stdio <-> SSE <-> streamable HTTP).
Research date: 2026-06-10.

---

## TL;DR for the synthesizer

This family spans the full maturity ladder of the "MCP gateway" niche:

- **sparfenyuk/mcp-proxy** (Python, MIT, 2.6k stars) — the canonical single-purpose **transport bridge**. No registry, no ACLs, no UI. Wins on simplicity; the most-installed tool in the niche.
- **TBXark/mcp-proxy** (Go, MIT, ~700 stars) — the canonical **static aggregator**: one JSON config, N upstream servers, one HTTP endpoint. Adds auth tokens + tool allow/block filters. No dashboard, no dynamic registration.
- **mcpjungle/MCPJungle** (Go, MPL-2.0, 1.1k stars) — the **registry + gateway**: dynamic `register`/`deregister` via CLI, tool groups, enterprise mode with per-client tokens/ACLs, OTEL metrics, beta dashboard. Single binary + SQLite/Postgres.
- **AmoyLab/Unla** (Go+TS, MIT, 2.1k stars) — the **API-to-MCP converter + gateway**: turns REST/gRPC/WebSocket APIs into MCP servers via YAML config with zero code, plus MCP proxying, multi-tenancy, session persistence, full web UI with chat playground. Heaviest architecture (apiserver + gateway + Redis/DB).
- **tigranbs/mcgravity** — **DEAD as an MCP product.** The repo was repurposed (by Feb 2026) into a Rust TUI for orchestrating Claude Code/Codex/Gemini. The original Bun/TypeScript "nginx for MCP" load balancer never got past basic CLI + YAML config; its planned health checks and web UI never shipped. Signal: pure load-balancing of MCP servers was not a strong enough standalone product.

Strategic gaps across the whole family that a new gateway can exploit: **end-to-end OAuth 2.1 (downstream client auth + upstream credential proxying), audit trails of tool calls, process/container isolation of stdio servers, semantic/agent-driven tool search, and an agent-first control plane (the gateway itself manageable over MCP).** Nobody in this family has all of these; most have none.

---

## 1. MCPJungle (mcpjungle/MCPJungle)

**What it is:** "One place to manage & connect to all your MCP servers." Self-hosted MCP gateway/registry for developers and teams. Register MCP servers once; every AI client (Claude, Cursor, Codex, custom agents) connects to a single `/mcp` endpoint.

**OSS status:** Open source, **MPL-2.0** (permits self-hosted commercial use without copyleft on your own code). **Go** (84%) + TypeScript dashboard (9%). 1,088 stars / 141 forks / 83 open issues; last push 2026-05-20; active Discord. Docs at docs.mcpjungle.com (which itself exposes an `llms.txt` index and an MCP server endpoint for AI agents to consume the docs — notable AX touch).

### Feature surface

**Deployment**
- Single binary (direct host), Docker Compose for local (development mode), Docker Compose + PostgreSQL for team (enterprise mode).
- Two Docker image variants: standard (minimal) and `stdio` (bundles npx/uvx so it can spawn stdio servers).
- DB: SQLite by default (`mcpjungle.db`), PostgreSQL for production (`DATABASE_URL` / `POSTGRES_*` env vars).

**Server modes**
- *Development* (default): no access control enforcement, OTEL metrics off by default.
- *Enterprise*: `mcpjungle init-server` enables authentication, per-client ACLs, observability.

**Transports**
- Streamable HTTP (primary, both downstream and upstream), STDIO upstream (spawned as child processes), SSE supported but flagged "not mature."

**Registry / tool discovery**
- `mcpjungle register` (HTTP or STDIO upstream config: name, transport, description, URL/command+args+env, bearer token, custom headers, `${VAR}` env substitution), `deregister`, `list tools/prompts/groups`, `get`, `delete`, `invoke` (invoke a tool straight from the CLI), `usage`.
- Canonical tool naming: `<server>__<tool>` (double-underscore namespacing).
- Per-tool and per-server enable/disable (`disable/enable tool`, also for prompts).

**Tool Groups** (key differentiator)
- Curated subsets of tools exposed at dedicated endpoints `/v0/groups/{name}/mcp`; include specific tools, whole servers, or exclude selected tools. Prompts not yet supported in groups.

**Sessions**
- Stateless (default, new upstream connection per invocation) or stateful per-server (persistent connections w/ idle timeout `SESSION_IDLE_TIMEOUT_SEC`) — connection pooling to cut cold-start latency.

**Auth & governance**
- Upstream: static bearer tokens, custom headers, env-var substitution; OAuth for upstream servers now in **beta** (per roadmap).
- Downstream (enterprise mode): per-client API tokens with per-client server allowlists (`create mcp-client`), user accounts (`create user`), admin vs standard roles. Downstream OAuth/SSO/OIDC not shipped.

**Observability**
- OpenTelemetry metrics, Prometheus-compatible `/metrics`, `/health`, `OTEL_RESOURCE_ATTRIBUTES` support. No tool-call audit log.

**Dashboard**
- Web dashboard in **beta** — server registration, tool browsing/invocation, common management tasks; positioned as a companion to the CLI, not the primary surface.

**API surface**
- `/mcp` (streamable HTTP gateway), `/v0/groups/{name}/mcp`, `/health`, `/metrics`, plus an HTTP management API mirroring the CLI (documented in the HTTP API reference).

**Roadmap (official docs, mid-2026)**
- OAuth: upstream beta shipped; downstream client OAuth/SSO/OIDC + token refresh in progress.
- Config-driven workflows & GitOps: declarative config, reconciliation, live reload (in development).
- Dashboard expansion (beta available).
- Enterprise access control: per-client credentials, finer-grained ACLs (planned).

### Agent experience (AX)
- CLI-first design (`mcpjungle invoke` lets an agent/script execute any registered tool from the shell).
- Docs published as `llms.txt` + an MCP endpoint for the documentation itself.
- Client integration recipes: Claude Desktop via `mcp-remote`, Cursor native streamable HTTP, Copilot `mcp.json`.
- No MCP-based *control plane* (you cannot manage the gateway itself over MCP) — management is CLI/HTTP only.

### Weaknesses (from comparisons + reviews)
- OAuth gap: downstream auth is bearer-token only; can't proxy a user's GitHub OAuth through the gateway; teams must front it with oauth2-proxy/Pomerium.
- No process isolation: stdio servers run as child processes of the gateway — a hostile/heavy server degrades neighbors or host (vs Docker MCP Gateway's per-container sandbox).
- No audit logging of tool calls; no compliance story; external SIEM required.
- RBAC basic and enterprise-mode-only; no credential isolation.
- SSE support immature; prompts unsupported in tool groups.
- Reviewers position it as "best for prototyping/early-stage… not a platform you won't have to replace in 12 months."

### Performance
- No published benchmark numbers. Claims: stateful connection pooling reduces cold-start latency; single-binary lightweight footprint.

---

## 2. sparfenyuk/mcp-proxy (Python)

**What it is:** "A bridge between Streamable HTTP and stdio MCP transports." The de-facto standard transport shim. 2,584 stars / 242 forks, MIT, Python, very active (push 2026-06-08, v0.12.0, 18 releases).

### Feature surface

**Two modes**
1. *stdio → SSE/StreamableHTTP client mode*: lets stdio-only clients (e.g., Claude Desktop historically) talk to remote SSE/streamable-HTTP servers. Args: remote URL, `--transport sse|streamablehttp`, `--headers K V` (repeatable), OAuth2 client-credentials (`--client-id/--client-secret/--token-url`), `--no-verify-ssl`, `API_ACCESS_TOKEN` env var.
2. *SSE/StreamableHTTP server mode*: exposes local stdio servers over HTTP. Args: `--port/--host`, `--env`, `--cwd`, `--pass-environment`, `--allow-origin` (CORS, wildcard ok), `--expose-header` (default `Mcp-Session-Id`), `--stateless` (stateless streamable HTTP), `--named-server NAME 'CMD'` (repeatable), `--named-server-config file.json`.

**Multi-server hosting (since 0.8.0)**
- One instance proxies multiple stdio servers; each at `/servers/<NAME>/sse`; optional default server at root `/sse`; global `/status` endpoint. JSON config uses the familiar `mcpServers` map (command/args/env/enabled; `timeout`/`transportType` fields parsed but ignored — stdio only).

**Session handling**
- Handles the full streamable-HTTP session lifecycle (create/persist/renegotiate `mcp-session-id`); often used specifically as a workaround for clients that botch session negotiation (servers returning 400 "Missing session ID").

**Packaging**
- `uv tool install mcp-proxy`, pipx, git install; containers on GHCR + Docker Hub, linux/amd64 + arm64; Dockerfile/Compose recipes; debug/log-level flags.

### Weaknesses
- Not a gateway: no registry, no dynamic registration, no ACLs beyond shared headers, no tool filtering, no UI, no metrics/observability, no audit.
- Auth pass-through quirks (e.g., issue #64: GitHub PAT works over stdio but fails over SSE).
- Per-named-server config fields (`timeout`, `transportType`) silently ignored.
- Containerized stdio servers need explicit `--pass-environment`; random port if unset surprises users.
- Python runtime dependency (uv/pipx) vs single static binary.

### AX notes
- Pure CLI; zero state; composable (proxy-behind-proxy patterns documented). Agents use it as plumbing, not a control plane. Inspector-tool integration for testing.

---

## 3. TBXark/mcp-proxy (Go)

**What it is:** "An MCP proxy server that aggregates and serves multiple MCP resource servers through a single HTTP server." Static config-file aggregator. 697 stars / 96 forks, MIT, Go (93.5%), v0.43.2 (Jan 2026), push Feb 2026.

### Feature surface
- Aggregates **tools, prompts, and resources** from N upstream servers; serves each upstream at its own path under one HTTP server via **SSE or streamable HTTP**.
- Upstream client types: `stdio` (command/args/env), `sse` (url/headers), `streamable-http` (url/headers).
- `mcpProxy` global config: `baseURL`, `addr`, `name`, `version`, `type` (sse/streamable-http), plus `options`:
  - `authTokens` — static bearer-token list gating access (per-proxy and overridable per-server),
  - `toolFilter` — allow/block lists per server (tool-level exposure control),
  - `panicIfInvalid` — fail-fast on a bad upstream,
  - `logEnabled`.
- Config loading from local file **or remote URL** (`--config https://…`) — proto-GitOps.
- Docker image bundles npx + uvx for spawning stdio servers; Docker Compose recipes; `go install` one-liner.
- Online "Claude config converter" (tbxark.github.io/mcp-proxy) converts a standard `claude_desktop_config.json` into proxy config — nice onboarding touch.

### Weaknesses
- Static config only: any change (add/remove server) = edit JSON + restart; no API/CLI for dynamic registration; no hot reload.
- No UI, no observability/metrics, no audit, no per-client identity (shared tokens only), no rate limiting.
- Per-path exposure (each upstream keeps its own endpoint) rather than one merged namespace — clients still need N entries unless they use one upstream path each.
- Slower release cadence; single-maintainer project.

### AX notes
- One JSON file fully describes the deployment — trivially generatable by an agent; remote-URL config enables config-as-artifact workflows. No runtime API for agents to mutate state.

---

## 4. mcgravity (tigranbs/mcgravity) — ABANDONED / REPURPOSED

**What it was (2025):** "Nginx for MCP" — a Bun/TypeScript proxy composing multiple MCP servers into one unified endpoint with **load balancing** across replicas of the same MCP server. YAML config, CLI, single compiled Bun executable, Docker image (`tigranbs/mcgravity`). Roadmap promised health checks and a web interface — **never shipped**.

**What it is now (2026):** The repo (98 stars) was repurposed into a **Rust TUI** that orchestrates AI coding CLIs (Claude Code, Codex, Gemini) in a plan→execute→review loop (v0.1.5–v0.1.8, Jan–Feb 2026). The MCP load balancer is gone from the README; directory sites (eliteai.tools, mcpindex, archestra) still describe the old product.

### Lessons for us
- Load balancing across MCP replicas was the one genuinely unique feature in this family — and the market didn't sustain it as a standalone product. Either bundle LB into a broader gateway or treat MCP servers as stateless and LB at the infra layer.
- Stale third-party directories mean competitive scans overstate this product; treat directory listings as unreliable.

---

## 5. Unla (AmoyLab/Unla)

**What it is:** "MCP Gateway — a lightweight gateway service that instantly transforms existing MCP Servers and APIs into MCP servers with zero code changes." Go backend + React/TS management UI. 2,134 stars / 173 forks / 91 open issues, MIT, very active (push 2026-06-08). Origin: Chinese dev community (docs bilingual, zh-CN README first-class). Docs: docs.unla.amoylab.com.

### Feature surface

**Protocol conversion (the headline feature)**
- **REST API → MCP server** via YAML config (routers/servers/tools), zero code; Go-template-based request/response mapping.
- **gRPC → MCP** and **WebSocket → MCP** (in development).
- **MCP proxying**: Client → MCP Gateway → upstream MCP servers (SSE + streamable HTTP).
- Multimodal MCP responses: text, images, audio.

**Architecture**
- Two-component system: **apiserver** (management plane, web UI backend) + **mcp-gateway** (data plane); deployable multi-replica.
- Config persistence: disk, SQLite, PostgreSQL, MySQL; config **version control** built in.
- Hot reload / config sync via OS signals, HTTP endpoint, or **Redis PubSub** (multi-replica coordination).
- Session persistence and recovery (sessions survive restarts; Redis-backed option).
- Multi-tenancy: tenant-scoped servers/configs and role-based admin.
- MCP server grouping/aggregation (in development).

**Auth & governance**
- OAuth-based pre-authentication for MCP servers; JWT-based API auth for the management plane; admin role credentials.

**UI**
- Full web management UI: add/edit MCP servers by pasting config, config versioning, and a **chat playground with SystemPrompt + Authorization fields** for testing tools end-to-end.

**Observability & ops**
- OpenTelemetry tracing (Jaeger recipe), downstream request/error capture.
- Background periodic fetch + caching of upstream capabilities with configurable refresh interval and TTL.
- Docker (multi-port 8080 + 5234-5236), Kubernetes manifests + Helm chart, bare-metal/VM/ECS.

### Weaknesses
- Self-acknowledged: "under rapid development," backward compatibility **not guaranteed**, docs lag implementation.
- Heaviest operational footprint of the family (apiserver + gateway + DB + Redis for full features) despite "lightweight" branding.
- gRPC/WebSocket conversion and grouping/aggregation still incomplete.
- Mostly Chinese-first community; English docs thinner; little Western community discussion (no HN/Reddit footprint found).
- No published performance numbers; no audit-trail/compliance story; no process isolation for spawned servers.

### AX notes
- Everything is declarative YAML (routers/servers/tools) — highly generatable by agents; hot-reload via HTTP/Redis means an agent can push config without restarts. Management is REST+JWT (machine-usable) but there is no MCP-native control plane either.

---

## Cross-cutting analysis

### Table stakes in this niche (everyone has or users assume)
- Aggregate N MCP servers behind one HTTP endpoint; tools/prompts/resources passthrough.
- Transport bridging stdio <-> SSE <-> streamable HTTP (streamable HTTP now the default; SSE legacy).
- `mcpServers`-style JSON/YAML config compatible with the Claude Desktop convention.
- Docker image (with npx/uvx bundled for stdio servers) + compose recipe; single static binary expected for Go entrants.
- Tool namespacing to avoid collisions (`server__tool`).
- Static bearer-token auth; custom headers to upstreams; env-var substitution in config.
- Tool-level allow/block filtering of what's exposed.
- Health/status endpoint.

### Differentiators worth stealing
- **MCPJungle**: tool groups with dedicated per-group MCP endpoints; CLI `invoke` of any tool; stateless-vs-stateful per-server session modes with pooling; `llms.txt` + MCP-served docs; Prometheus/OTEL out of the box; dev-vs-enterprise mode split in one binary.
- **sparfenyuk**: bidirectional bridging incl. OAuth2 client-credentials on the client side; correct streamable-HTTP session lifecycle handling (a real pain point); `--stateless` mode; named-server multiplexing on one port.
- **TBXark**: remote-URL config loading (config-as-artifact); per-server authTokens override; online config converter from Claude Desktop format; `panicIfInvalid` fail-fast semantics.
- **Unla**: REST/gRPC/WS → MCP conversion with Go templating (zero-code MCP-ification of legacy APIs); config versioning + hot reload via Redis PubSub; recoverable sessions across restarts; multi-tenancy; chat playground in UI; OTEL tracing.
- **mcgravity (RIP)**: load balancing across MCP server replicas — still unclaimed in OSS.

### Common weaknesses = whitespace
1. **No end-to-end OAuth 2.1** (downstream client auth + upstream user-credential proxying) anywhere in the family — the #1 cited gap.
2. **No audit trail of tool invocations** (who called what tool with what args) — kills regulated-industry adoption.
3. **No sandboxing**: stdio servers run as raw child processes everywhere (Docker MCP Gateway is the only one doing per-container isolation, outside this family).
4. **No semantic tool search / context-budget management** — all expose full (or statically filtered) tool lists; nobody reduces agent token bloat dynamically.
5. **No agent-first control plane** — none can be administered *over MCP* by an agent; management is human CLI/UI/REST.
6. **No rate limiting / quotas / cost attribution** per client or per tool.
7. Performance: zero published benchmarks anywhere in the family — "lightweight" claims are unquantified; a credible perf story (latency overhead, conns/instance) would stand out.

### Positioning ladder (for synthesis)
bridge (sparfenyuk) → static aggregator (TBXark) → dynamic registry+ACL gateway (MCPJungle) → converter+platform (Unla). Each step up adds ops weight; the family's churn (mcgravity dead, Unla unstable APIs, MCPJungle "replace in 12 months" critique) shows users want the MCPJungle-level capability set with bridge-level simplicity — i.e., a single binary that also solves OAuth, audit, isolation, and agent-driven control.

---

## Sources
- https://github.com/mcpjungle/MCPJungle ; https://docs.mcpjungle.com/ ; https://docs.mcpjungle.com/roadmap
- https://github.com/sparfenyuk/mcp-proxy (README, issues #59/#64, releases)
- https://github.com/TBXark/mcp-proxy ; https://tbxark.github.io/mcp-proxy/
- https://github.com/tigranbs/mcgravity (current Rust TUI) ; https://eliteai.tools/mcp/tigranbs-mcgravity (archived description of original LB)
- https://github.com/AmoyLab/Unla ; https://docs.unla.amoylab.com/en/
- https://mcp.directory/blog/mcpjungle-vs-obot-vs-docker-mcp-gateway-vs-composio-2026
- https://www.lunar.dev/post/the-best-open-source-mcp-gateways-in-2026
- GitHub API repo stats fetched 2026-06-10.
