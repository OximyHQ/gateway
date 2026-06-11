# Docker MCP Gateway + MCP Catalog/Toolkit — Competitive Intelligence Report

Date: 2026-06-10
Subject category: MCP gateway / MCP server ecosystem (catalog + desktop toolkit + OSS gateway + enterprise governance)
Researcher context: input for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

---

## 1. What it is

Docker's MCP offering is a four-part stack, not one product:

1. **Docker MCP Gateway** (`docker/mcp-gateway` on GitHub) — an open-source Go CLI plugin (`docker mcp ...`) whose core feature is a gateway process that aggregates many MCP servers behind one MCP endpoint. Servers run as isolated Docker containers; the gateway handles lifecycle, routing, credential injection, and policy. MIT license. ~1.4k stars, actively developed (last push June 2026; releases roughly monthly, latest v0.42.2, 2026-05-28).
2. **Docker MCP Catalog** — a curated registry of containerized MCP servers on Docker Hub (`hub.docker.com/mcp`), 300+ servers (marketed as "the largest library of containerized MCP servers"; >1M pulls claimed mid-2025). Docker-built images are signed with SBOM/provenance attestations.
3. **Docker MCP Toolkit** — the GUI inside Docker Desktop (4.62+ for the current profile-based UI) for browsing the catalog, enabling servers, configuring them, managing OAuth/secrets, and connecting MCP clients. Closed-source, ships with Docker Desktop.
4. **Docker AI Governance** (2026, enterprise) — a paid admin-console layer that pushes org-wide policies (network, filesystem, credentials, allowed MCP servers/tools) down to every developer's Docker runtime at authentication time.

Positioning: "secure infrastructure for agentic AI" — the gateway is the runtime enforcement point, the catalog is the trusted supply chain, the toolkit is the developer UX, governance is the enterprise control plane.

Note: this is an **MCP-only** gateway. There is no LLM/provider gateway component (no model routing, no token cost management, no provider keys). Docker's adjacent "Model Runner" is a separate product.

---

## 2. Architecture & deployment model

```
AI Client (Claude Desktop / Claude Code / Cursor / VS Code / Zed / Gemini / continue ...)
        → MCP Gateway (one MCP endpoint; stdio | SSE | streamable HTTP)
            → MCP Servers, each in its own Docker container
```

- **Lifecycle on demand**: when a tool call arrives, the gateway identifies the owning server, starts its container if not running, injects credentials, applies restrictions, forwards the call, returns the result. `--keep` retains stopped containers.
- **Transports**: `stdio` (default, single client), `sse`, and `streaming` (streamable HTTP; multi-client with `--port`).
- **Runs anywhere there's a Docker engine**: as a Desktop background service, as a bare CLI (`docker mcp gateway run`), or as a container (`docker/mcp-gateway` image) under Docker Compose — the standard Compose pattern mounts `/var/run/docker.sock` so the gateway can spawn server containers (a real security trade-off, see §9).
- **Headless/CE mode**: `DOCKER_MCP_IN_CONTAINER=1` bypasses Docker Desktop feature checks for WSL2/Docker CE/containers; `docker mcp feature enable profiles` enables profiles outside Desktop. OAuth has a separate "CE mode" doc (oauth-ce-mode.md) for running without Desktop.
- **Config storage**: local database managed via CLI; feature flags in `~/.docker/config.json`; legacy file-based config (`~/.docker/mcp/catalogs/docker-mcp.yaml`, `config.yaml`, `registry.yaml`) still addressable via flags.
- **Catalog source**: online catalog at `desktop.docker.com/mcp/catalog/v2/catalog.yaml` (v3 when the `mcp-oauth-dcr` feature is enabled).
- Implementation: **Go (97.8%)**, MIT. Prereqs: Docker Desktop 4.59+ or any Docker engine; Go 1.24+ to build.

---

## 3. Full feature surface

### 3.1 Gateway runtime (`docker mcp gateway run`) — complete flag set
- `--servers s1,s2` / `--server <oci-ref>` (standalone dockerized server, no catalog needed) / `--profile <id>`
- `--tools server:tool` granular tool filtering (e.g. `--tools server1:* --tools server2:tool2`)
- `--transport stdio|sse|streaming`, `--port`
- `--catalog`, `--config`, `--registry` (file paths)
- `--secrets docker-desktop:./.env` — ordered secret-provider fallback chain (Desktop secrets API, then .env files)
- `--cpus` (default 1/server) and `--memory` (default 2GB/server) per-server resource caps
- `--block-secrets` (default **true**) — scans inbound AND outbound payloads for secret-shaped content and blocks the call
- `--block-network` — block forbidden outbound network access from tool containers
- `--verify-signatures` — verify provenance/signature of server images before running
- `--log-calls` (default true) — tool-call logging/tracing
- `--interceptor when:type:spec` — pluggable middleware (see 3.2)
- `--watch` (default true) — hot-reload on config change; `--dry-run` config validation; `--verbose`; `--keep`

### 3.2 Interceptors (middleware/plugin system)
Three types × two hook points (`before`/`after` tool call):
- **`exec`**: `--interceptor 'before:exec:<shell script>'` — request/response JSON piped to stdin via `/bin/sh -c`; write JSON to stdout to override/short-circuit the response; stderr goes to gateway logs.
- **`docker`**: `--interceptor 'before:docker:image arg…'` — same contract, run in a container.
- **`http`**: `--interceptor 'before:http:http://host/path'` — JSON POSTed; non-empty JSON reply overrides.
`before` interceptors can allow, mutate, or fully bypass a call with a custom response; `after` interceptors can rewrite responses (documented example: `jq` one-liner truncating tool output to 100 chars to save tokens). This is the extensibility story — no compiled plugin API; everything is process/container/webhook-shaped.

### 3.3 Profiles (working sets)
- Group servers into named collections; connect a profile to a client (`--connect cursor`), per-project/per-environment separation.
- Per-profile **server config** (`profile config --set github.timeout=30`) and **tool allowlists** (`profile tools --enable github.create_issue`, `--enable-all/--disable-all`).
- **Profiles are OCI artifacts**: `profile push/pull <oci-ref>` plus `export/import` YAML — shareable, versionable team working sets.
- Server reference schemes: `catalog://mcp/docker-mcp-catalog/github`, `docker://image:tag`, `https://registry.modelcontextprotocol.io/v0/servers/<id>` (official MCP community registry), `file://./server.yaml`.

### 3.4 Catalog management
- Multiple catalogs; default = `mcp/docker-mcp-catalog` OCI image.
- `catalog create` from explicit server refs, **from an existing profile**, or **imported from the OSS MCP community registry** (`--from-community-registry registry.modelcontextprotocol.io`).
- Catalogs are OCI artifacts too: `push/pull/tag`, per-catalog `server ls/add/remove/inspect`. Enterprises curate private catalogs of approved servers.
- Public catalog: 300+ servers; two build tiers — **Docker-built** (Docker builds from source, signs, attaches SBOM + provenance, "enhanced security") and **community/partner-built**; plus **remote (cloud-hosted) servers** for SaaS (GitHub, Notion, Linear) using OAuth. Submission via PR to `docker/mcp-registry`; approved servers appear within ~24h on Desktop + Hub.

### 3.5 Clients
- `docker mcp client connect <name> --profile <id> [--global]` rewrites client configs automatically. Supported: Claude Desktop, Claude Code (respects `CLAUDE_CONFIG_DIR`), Cursor, VS Code/Copilot agent mode, Zed, continue, Gordon (Docker's own AI), Amazon Q, Codex (docs in progress), etc. One gateway config shared consistently across all clients.

### 3.6 Tools CLI (agent/scripting surface)
- `docker mcp tools count | ls [--format=json] | inspect <tool> | call <tool> args…` — i.e., tool invocation from shell/CI without an LLM client. JSON output exists but is not pervasive across all commands.

### 3.7 Secrets & OAuth
- Secrets stored via Docker Desktop secrets API (`docker mcp secret set 'brave.api_key=XXX'`), injected into server containers at runtime — never in client config files or env vars; servers get **zero host env vars** by default.
- `docker mcp oauth authorize/revoke <service>` — built-in OAuth flows (GitHub, Notion, Linear…); v3 catalog adds **OAuth DCR (dynamic client registration)** behind the `mcp-oauth-dcr` feature flag.
- `docker mcp secret export server1 server2` for shipping secrets to cloud runs (interim until Docker Cloud reaches secret stores).

### 3.8 Dynamic MCP (agent-first control plane) — flagship differentiator
When connected to the gateway, the agent itself gets **management tools**:
- `mcp-find` — search the catalog by name/description
- `mcp-add` / `mcp-remove` — install/remove servers into the live session, no restart
- `mcp-config-set` — configure servers (gateway handles credential injection)
- `mcp-exec` — execute tools of available servers without each tool occupying a context-window slot
- `code-mode` — experimental: agent writes JavaScript that composes multiple MCP tools inside an isolated sandbox that can only talk through MCP tools; reduces token usage by putting 1–2 synthetic tools in context instead of dozens of schemas
Limitations: dynamically added servers are **session-only** (no persistence across sessions); code-mode "not yet reliable"; the whole feature is experimental. Docker frames this as moving from "what do I configure?" to "what can agents do for themselves?" — exactly the agent-experience direction.

### 3.9 Toolkit (Docker Desktop GUI)
- Browse/search catalog, one-click enable servers into profiles, per-server config tabs, OAuth authorize/revoke UI, secrets entry, client connect toggles, filesystem-path grants per server. The gateway runs automatically in the background when the Toolkit feature is on. This is the closest thing to a "dashboard" — there is **no web dashboard**, no usage analytics UI, no audit-log viewer; logging is CLI/file-based.

### 3.10 Security model (defense in depth)
- **Supply chain (passive)**: Docker-built images signed; SBOM + provenance attestation; code/dependency scanning; tool descriptions frozen at build time and enforced at runtime (anti rug-pull/tool-poisoning); servers with dynamic tool lists flagged.
- **Runtime (active)**: per-server CPU/memory caps; no host filesystem access by default (opt-in per-directory via GUI; read-only enforced at container level when the tool is annotated readonly); outbound network restrictions per server annotation; env-var stripping; secret-pattern scanning on both directions of every call (on by default); signature verification; call logging.
- Documented threat scenarios: tool prompt injection, embedded cryptominer, secret exfiltration, filesystem snooping, runtime tool-list manipulation. Docker markets these via its "MCP Horror Stories" blog series.

### 3.11 Docker AI Governance (enterprise, 2026)
- Central admin console; policies pushed at Docker Desktop authentication time; four control surfaces: **network** (domain/IP/CIDR allow/deny), **filesystem** (mount rules, RO/RW scope), **credentials** (session-scoped, exfiltration blocked to unapproved destinations), **MCP tools** (org-managed allowlists, unapproved servers blocked by default).
- Pitch: the same policy follows the agent from laptop → CI → production because Docker is the runtime in all three.

---

## 4. Pricing / licensing
- Gateway CLI: open source, MIT, Go.
- Catalog: free to use/pull.
- Toolkit: bundled in Docker Desktop — free for individuals/small business (<250 employees AND <$10M revenue); otherwise requires a paid Docker subscription (Pro/Team/Business). The full polished experience is effectively a Docker Desktop adoption funnel.
- Docker AI Governance: enterprise add-on (pricing not public; sold with Business tier).

## 5. Published performance numbers
None. No latency/throughput/overhead benchmarks anywhere in docs, blog, or repo. Adoption stats only: 300+ catalog servers, >1M catalog pulls (mid-2025), "millions of developers" reach claim. Resource defaults (1 CPU / 2GB per server) are security caps, not perf claims. Container cold-start cost per first tool call is real but unquantified by Docker.

## 6. Observability
- `--log-calls` (default on) + `--verbose`; telemetry docs folder in repo (OTel-ish internal telemetry); interceptors usable as a DIY audit/monitoring tap (exec/http hooks).
- No metrics endpoint, no dashboard, no built-in trace export documented for users, no per-tool usage analytics. Governance product adds policy/audit centrally but that's enterprise-only.

## 7. How agents are expected to use it (AX notes)
- Dynamic MCP is the headline: gateway exposes its own control plane AS MCP TOOLS (`mcp-find/add/remove/config-set/exec/code-mode`) so the agent self-provisions tools mid-conversation, with the catalog as the trust boundary and the gateway injecting credentials so the agent never sees secrets.
- `mcp-exec` + code-mode are explicit answers to context-window/token bloat — keep one meta-tool in context, not 50 schemas.
- CLI is scriptable (`tools call`, `--format=json` on some commands) but JSON output is not uniform; config is CLI/db-backed rather than a declarative API; no REST admin API at all.
- Session-only persistence of agent-added servers is the big AX gap: nothing the agent builds survives a restart.

## 8. Recent trajectory (changelog signal)
- v0.40.x (Feb–Apr 2026) → v0.41 (Mar) → v0.42.x (Apr–May 2026): profiles feature GA'd in Desktop, OAuth DCR / catalog v3, Dynamic MCP + code-mode (late 2025/2026), Docker AI Governance launch (2026). MCP spec 2025-11-25 referenced. Cadence ~monthly.

## 9. Weaknesses & user complaints (GitHub issues, community posts)
- **Secrets management is the #1 pain**: top issues are "Unable to set secrets," "bypass Docker MCP secret store; pass secrets from env vars instead," secrets failing in server/CE mode ("Error: open /.s0: file does not exist"), fail-slow when Desktop is absent. The Desktop secrets API dependency hurts headless/CI use.
- **Docker Desktop coupling**: profile feature invisible on some Desktop versions; WSL2 connectivity problems; Windows + Claude Desktop connection failures; CE/headless requires env-var workarounds and a degraded OAuth path.
- **Catalog/config DX**: deprecated catalog.yaml fields accepted silently (hours of debugging); custom catalogs reportedly only loading a single server; custom catalog servers not displayed in Toolkit UI; stdio→SSE transport confusion when moving from laptop to cloud.
- **Token bloat**: raw aggregated tool responses measured at ~3x tokens vs hand-built integration (motivation for code-mode, which is itself unstable; Dynamic MCP tool-schema bugs filed).
- **docker.sock mount** in the Compose deployment pattern = root-equivalent host access for the gateway container.
- **No environment separation story**: one gateway/catalog mixing dev+prod servers caused real prod incidents for users; profiles help but env-awareness isn't first-class.
- **No web dashboard / metrics / audit UI** in OSS; observability is logs + DIY interceptors; governance features paywalled into Desktop/Business.
- Container-per-server model: heavier footprint and cold-start vs in-process aggregation; resource caps are fixed flags, not policy.

## 10. What to steal / where to beat them
**Steal:**
1. Dynamic MCP management-tools pattern (`mcp-find/add/exec` + code-mode) — control plane exposed to the agent as MCP tools.
2. Interceptor model (before/after × exec/container/http with stdin-JSON contract) — dead-simple, language-agnostic middleware.
3. Profiles and catalogs as **OCI artifacts** (push/pull/tag) for team distribution.
4. Default-on `--block-secrets` bidirectional payload scanning, env-var stripping, build-time-frozen tool descriptions enforced at runtime.
5. Signed images + SBOM as catalog trust tiers; import path from the official community MCP registry.
6. `docker mcp client connect <client>` automatic client-config rewriting.

**Beat them on:**
- Single binary with **no Docker/Desktop dependency** (their biggest structural weakness: best features gated on Desktop, secrets broken headless).
- A real web dashboard + metrics/audit/usage analytics in OSS (they have none).
- Persistent agent-added servers + first-class environments (dev/staging/prod).
- Unified LLM gateway + MCP gateway (they have no model/provider layer at all).
- Uniform machine-readable API (REST/JSON everywhere) — their admin surface is CLI-only, JSON only in spots.
- Published performance numbers (they have none to compare against).

---

### Sources
- https://github.com/docker/mcp-gateway (README, docs/mcp-gateway.md, docs/security.md, examples/interceptors, releases, issues)
- https://docs.docker.com/ai/mcp-catalog-and-toolkit/ (gateway, toolkit, catalog, dynamic-mcp pages)
- https://www.docker.com/blog/docker-mcp-gateway-secure-infrastructure-for-agentic-ai/
- https://www.docker.com/blog/dynamic-mcps-stop-hardcoding-your-agents-world/
- https://www.docker.com/blog/docker-ai-governance-unlock-agent-autonomy-safely/ ; https://www.docker.com/products/ai-governance/
- https://hub.docker.com/mcp ; https://www.docker.com/pricing/
- https://dasroot.net/posts/2026/01/docker-mcp-gateway-interceptors-security/ ; community posts (collabnix, ajeetraina)
