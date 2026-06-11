# Competitive Intel: Lasso MCP Gateway + MCP Security Tooling (mcp-scan / Invariant Labs)

Category: MCP gateway / MCP security ecosystem
Researched: June 2026
Subjects: (1) Lasso Security `mcp-gateway` (open source) + Lasso commercial platform; (2) Invariant Labs `mcp-scan` (now Snyk `agent-scan`), `invariant-gateway`, and Invariant Guardrails; (3) surrounding MCP-security landscape.

---

## 1. Lasso MCP Gateway (`lasso-security/mcp-gateway`)

### Positioning
"First open-source security gateway for MCP" (launched April 2025). A plugin-based intermediary that sits between an LLM/agent client and the user's existing MCP servers. Security-first framing: sanitize requests/responses, scan servers before loading, observe everything. Part of Lasso's broader commercial GenAI security platform (LLM firewall / "Deputies"), with the gateway as the OSS wedge.

### Architecture & deployment
- **Implementation**: Python (~99%). MIT license. ~376 stars / 29 forks (modest traction).
- **Install**: `pip install mcp-gateway` or Docker image (`INSTALL_EXTRAS` build arg to bake in optional plugin deps like `presidio,xetrack`).
- **Config model**: reads the client's existing `mcp.json` / `claude_desktop_config.json`; the gateway itself is registered as the single MCP server in the client, with the real servers nested under a `servers` sub-key inside the gateway's entry. Env vars for plugin keys (e.g. `LASSO_API_KEY`).
- **CLI**: `mcp-gateway --mcp-json-path <path> -p <plugin> [-p <plugin> ...] [--scan]`. That is essentially the entire CLI surface — no subcommands, no API server, no daemon mode.
- **Lifecycle**: manages startup/shutdown of all child MCP servers it proxies (stdio child-process model; remote-transport support not documented).

### How agents see it (key design choice)
- Since v1.0.0 ("Dynamic Capability Registration", Apr 2025), tools from proxied MCP servers are **re-exported as native gateway capabilities** — the agent sees the union of all downstream tools directly, not a generic proxy method.
- Older/fallback surface: two meta-tools — `get_metadata` (discover proxied MCPs and their capabilities) and `run_tool` (execute a downstream capability with sanitization applied).

### Security features
1. **Request/response sanitization** — interception of both directions; masking applied before data reaches the agent or the downstream server.
2. **Security Scanner** (`--scan`, v1.1.0 Jul 2025):
   - Reputation analysis of each configured MCP server using marketplace + registry data (NPM, Smithery, GitHub).
   - Tool-description scanning for hidden instructions / malicious patterns (tool poisoning).
   - **Auto-blocking**: servers below reputation threshold (30) are blocked; scan results written back into the mcp.json as status flags: `"passed"`, `"blocked"`, `"skipped"`, `null`.
3. **Plugin guardrails** (feature matrix):
   | Plugin | PII masking | Token masking | Custom policy | Prompt injection | Harmful content |
   |---|---|---|---|---|---|
   | `basic` | – | yes | – | – | – |
   | `presidio` | yes | – | – | – | – |
   | `lasso` (commercial API) | yes | yes | yes | yes | yes |
   - `basic` token-masking regex set: Azure secrets, GitHub tokens/OAuth, GCP API keys, AWS access tokens, JWTs, GitLab cookies, Hugging Face tokens, MS Teams webhooks, Slack tokens.
   - `presidio` (Microsoft Presidio, optional extra): credit cards, IPs, emails, phones, SSNs.
   - `lasso` plugin = thin client to Lasso's hosted Deputies/guardrails API (requires `LASSO_API_KEY`, signup); this is the monetization hook — full prompt-injection/harmful-content/custom-policy detection is NOT in the OSS gateway.
4. **Tracing plugin (`xetrack`)** — structured logging of every tool call to loguru logs or DuckDB/SQLite; queryable fields include `server_name`, `capability_name`, `path`, `content_text`. This is the gateway's entire observability story; no built-in dashboard in the OSS repo (the "comprehensive dashboard" marketing claim refers to the commercial Lasso platform).

### Release cadence / maintenance signal
- v0.1.2 (Apr 20 2025) → v1.0.0 (Apr 22 2025) → v1.1.0 (Jul 15 2025) → v1.2.0 (Jan 21 2026, Lasso Guardrails v3 API support, better message extraction, debug logging). Roughly one release per 6 months after launch — slow cadence; the OSS gateway looks like a maintained-but-not-prioritized funnel into the paid platform.

### Pricing / commercial
- OSS gateway: MIT, free, self-hosted.
- Lasso platform: contact-sales enterprise pricing (no public numbers). Hosted/managed options via sales. Partnership with Portkey to embed Lasso guardrails into Portkey's AI gateway (signal: Lasso wants to be the security layer inside other people's gateways, not necessarily win the gateway itself).

### Published performance numbers
None. No latency, throughput, or overhead benchmarks anywhere in repo, blog, or launch coverage.

### Known weaknesses / criticisms
- Python single-process; no published perf data; sanitization (Presidio NLP) on the hot path implies real latency overhead.
- No auth/identity layer: no OAuth handling for downstream servers, no RBAC, no SCIM, no per-user credentials, no audit log (called out by competitor MintMCP; vendor-biased source but directionally accurate against the repo).
- No remote-transport story documented (stdio orchestration only; SSE/streamable-HTTP upstreams and serving the gateway itself over HTTP are not first-class).
- Best detection capabilities (prompt injection, custom policies, harmful content) are gated behind the commercial Lasso API key.
- Reputation scoring based on GitHub/marketplace stats is gameable and crude (fixed threshold 30).
- No dashboard/UI in OSS; tracing = SQLite/logs you query yourself.
- Tool re-export model re-namespaces everything through one server — collisions and client tool-count limits unaddressed.

---

## 2. Invariant Labs: mcp-scan / invariant-gateway / Guardrails (now Snyk)

### Corporate arc (important context)
- Invariant Labs: ETH Zurich spin-off, founded 2024, ~10 people. Coined/dominated the MCP-threat research vocabulary: **tool poisoning**, **MCP rug pulls**, **tool shadowing**, **toxic flows** (their WhatsApp/GitHub MCP exploit writeups drove much of 2025's MCP-security discourse).
- **Acquired by Snyk (June 2025)**, folded into Snyk's "AI Trust Platform" / Snyk Labs.
- `mcp-scan` repo now redirects to **`snyk/agent-scan`**; PyPI `mcp-scan` (v0.4.3, Mar 2026) is a redirect shim that installs `snyk-agent-scan`. Invariant's docs site (`explorer.invariantlabs.ai/docs`) now 301s to GitHub — docs are decaying post-acquisition.
- Takeaway: the best-known independent OSS MCP security toolchain got absorbed into an enterprise platform; the standalone OSS tools are in maintenance/transition. This is an opening.

### 2a. mcp-scan (original) — feature surface
- **Language/license**: Python, Apache-2.0. CLI via `uvx mcp-scan` / pip.
- **Commands**: `scan` (default), `proxy`, `inspect` (dump tool/prompt/resource descriptions unverified), `whitelist` (approve entities, add/reset), `server`, `help`.
- **Scan mode (static)**:
  - Auto-discovers MCP configs for: Claude Desktop, Claude Code, Cursor, Windsurf, Gemini CLI (agent-scan extends to VS Code, Codex, Amazon Q, Kiro, OpenCode, Amp, Antigravity, OpenClaw, etc.).
  - Starts each configured server, pulls tool/prompt/resource descriptions, runs local checks + Invariant Guardrailing API checks.
  - Detects: prompt injection in tool descriptions, **tool poisoning**, **cross-origin escalation / tool shadowing** (one server's tool description manipulating another server's tools), **toxic flows** (combinations of tools that together enable exfiltration; `--full-toxic-flows` to show all participating tools, top-3 default).
  - **Tool pinning / hashing**: hashes tool descriptions and flags changes → detects **MCP rug pulls** (server silently swapping a benign tool for a malicious one post-approval).
  - Flags: `--checks-per-server`, `--server-timeout`, `--suppress-mcpserver-io`, `--json`, `--storage-file`, `--base-url` (point at own verification server).
- **Proxy mode (dynamic, the clever bit)**:
  - `mcp-scan proxy` **temporarily rewrites all discovered MCP client configs** to route every server through a locally-injected Invariant Gateway; restores configs on exit. Zero per-server setup; system-wide MCP traffic interception in one command.
  - Real-time logging of all tool calls (`--pretty oneline|compact|full`) + guardrail enforcement. Guardrail evaluation runs **fully locally** (no external API on the data path).
- **Guardrails config**: `~/.mcp-scan/guardrails_config.yml`, hierarchy `client → server → guardrails`:
  - Built-in shorthand guardrails: `pii`, `secrets`, `links`, `moderated` — each set to `block` or `log`.
  - `custom_guardrails`: named rules with full Invariant policy-language code, per client/server, action `block`/`log`.
- **Privacy model**: tool names+descriptions are sent to invariantlabs.ai for verification by default; anonymous per-scan ID; `--opt-out` flag; call contents never stored. (Agent-scan now requires Snyk API token auth — a regression vs the old anonymous flow.)
- **Snyk agent-scan additions**: scans **agent skills** too (prompt injection, malware payloads, hardcoded secrets, credential handling in skill files); background mode via MDM/CrowdStrike reporting to Snyk Evo for fleet monitoring; interactive consent before executing servers (`--dangerously-run-mcp-servers` to bypass); **closed to external contributions**.
- **Known limitations**: scanning executes the configured server commands (risk on untrusted configs); static description analysis can't catch behavioral attacks; Snyk token requirement; docs/ecosystem churn after rename.

### 2b. invariant-gateway — feature surface
- LLM + MCP transparent proxy; Python, Apache-2.0, ~72 stars; Docker (`ghcr.io/...gateway:latest`, port 8005).
- **LLM providers**: OpenAI chat completions, Anthropic Messages, Gemini generateContent/stream — via base-URL swap, no code change.
- **MCP**: proxies stdio, SSE, and streamable HTTP tool calling.
- **Guardrails delivery — two modes** (notable design):
  1. **Header-based**: rules shipped per-request in an `Invariant-Guardrails` header → guardrails live in the agent's code, versioned with it.
  2. **Explorer-managed**: rules attached to a project in the Explorer web UI → ops-managed, decoupled from agent code.
- All traces auto-pushed to **Invariant Explorer** (their OSS trace-viewer/annotation dashboard, also self-hostable) into datasets.
- Framework recipes: OpenAI Swarm, LiteLLM, AutoGen, OpenHands, SWE-agent.
- Performance claim (qualitative only): rule engine uses **stateful + incremental evaluation** and overlaps evaluation with natural LLM latency to "significantly reduce guardrailing latency". No published numbers.

### 2c. Invariant Guardrails policy language (the crown jewel)
- Python-ish declarative DSL over **agent traces** (not single messages): `raise "msg" if: <pattern>` with typed trace selectors `(msg: Message)`, `(call: ToolCall)`, `(out: ToolOutput)`.
- **Flow operator `->`**: source-to-sink dataflow matching across the whole trace — e.g. "if `get_inbox` output contained prompt injection, block any later `send_email` call". This contextual, cross-step matching is the differentiator vs per-message regex/classifier guardrails.
- Importable built-in detectors usable inside rules: `secrets()`, `pii()`, `prompt_injection()`, `moderated()`, code analysis (unsafe patterns like `eval`), copyright detection, links.
- Expressible policies: tool allow/deny, parameter-value constraints, ordering constraints, loop/retry detection, conditional flows ("if agent read untrusted X, forbid write-action Y").
- Open source (`invariantlabs-ai/invariant`, Apache-2.0); also offered hosted via the (now Snyk) Guardrails API.

---

## 3. Surrounding MCP security landscape (brief)

- **Cisco `mcp-scanner`**: YARA-rule scanning of tool descriptions/schemas (prompt injection, credential harvesting, code exec patterns). Apache-2.0, v4.3.x.
- **eSentire MCP-Scanner**: keyword + semantic + LLM-judge layered analysis (poisoning, rug pulls, impersonation); academic paper at ACM/IEEE 2026.
- **Proximity (Nova)**: OSS scanner enumerating prompts/tools/resources and risk-rating them.
- **CyberArk `agent-guard`**: secrets-retrieval for agents (AWS/CyberArk secret managers) + MCP proxy for traceability — identity/credential angle.
- **Gateways with security posture**: MCP Manager, MintMCP (SaaS, SCIM/RBAC/audit/virtual tool bundles, SOC2), Golf.dev (deploy-your-own servers + authn), Pipelock (inline agent firewall: HTTP/WebSocket/MCP), Pomerium, Runlayer, Docker MCP Toolkit. NSA published MCP security guidance (2026), pushing "agent firewall" as a category — regulatory/enterprise tailwind.
- Common landscape take: static scanners are commoditizing fast (YARA-level); the defensible layers are (a) runtime trace-aware policy (Invariant's flow rules) and (b) identity/governance (RBAC, per-user creds, audit) — which almost no OSS tool ships today.

---

## 4. Agent-experience (AX) observations

- **Lasso**: agent-facing surface is the re-exported tool union (good — transparent to agents) or `get_metadata`/`run_tool` meta-tools (bad — burns agent reasoning on indirection). No CLI for agents to manage the gateway, no management API, no MCP control-plane tools; config is hand-edited JSON. Scanner writes results *into* the user's mcp.json (mutating user config — clever but invasive).
- **mcp-scan**: genuinely agent/CLI-native — single `uvx mcp-scan` command, `--json` output on every command, auto-discovery of client configs, and the temporary-config-injection proxy trick means zero-setup interception. The config-injection/restore pattern is worth stealing for any "wrap all of a user's MCP servers" onboarding.
- **Invariant gateway**: header-carried policy (`Invariant-Guardrails` header per request) lets the *agent code itself* declare its guardrails — a notably API-first/agent-first control surface vs dashboard-only policy management.
- Nobody in this cluster exposes the gateway's own controls *as MCP tools* (e.g., "list blocked calls", "approve this tool", "explain why blocked") — an obvious gap for an agent-first gateway.

## 5. What to steal / where they're beatable

**Steal:**
- Invariant's trace-level flow-rule DSL (`->` operator, source→sink policies) — the only guardrail model that handles toxic flows; everything else is per-message.
- Tool hashing/pinning + diff-on-change (rug-pull defense) as a first-class gateway feature, not a scanner afterthought.
- mcp-scan's auto-discover + temporarily-inject-proxy onboarding (one command secures every client on the machine).
- Lasso's plugin matrix clarity (basic regex masking free, NLP PII optional extra, hosted detection premium) as a packaging pattern.
- Pre-load reputation/description scanning with auto-block + persisted pass/block status per server.

**Beatable because:**
- Both flagship OSS tools are Python with zero published perf numbers — a single static binary with measured overhead wins the credibility fight immediately.
- Invariant's standalone tooling is decaying post-Snyk (docs 301, repo renamed, token-gated, closed to contributions); Lasso's OSS gateway ships ~2 releases/year and gates its best detections behind a paid API.
- Neither has identity/governance (OAuth to downstream servers, RBAC, audit log) nor a real OSS dashboard.
- Neither unifies LLM-gateway + MCP-gateway in one binary (Invariant's gateway proxies both but is a thin Docker side-car with no management plane).

---

### Key sources
- https://github.com/lasso-security/mcp-gateway (+ /releases)
- https://www.lasso.security/resources/lasso-releases-first-open-source-security-gateway-for-mcp
- https://github.com/invariantlabs-ai/mcp-scan (→ snyk/agent-scan), https://pypi.org/project/mcp-scan/
- https://github.com/iflow-mcp/invariantlabs-ai-mcp-scan (mirror of original README)
- https://github.com/invariantlabs-ai/invariant-gateway, https://github.com/invariantlabs-ai/invariant
- https://invariantlabs.ai/blog/guardrails, https://invariantlabs.ai/blog/introducing-mcp-scan
- https://snyk.io/news/snyk-acquires-invariant-labs-to-accelerate-agentic-ai-security-innovation/
- https://www.mintmcp.com/blog/mintmcp-vs-lasso-security (vendor-biased)
- https://mcpmanager.ai/blog/mcp-security-tools/, https://pipelab.org/blog/nsa-mcp-security-guidance/
