# Competitive Intel: IBM ContextForge MCP Gateway

**Date:** 2026-06-10
**Repo:** https://github.com/IBM/mcp-context-forge (~3.9k stars, ~700 forks, 20 releases)
**Docs:** https://ibm.github.io/mcp-context-forge/
**Category:** MCP gateway / registry / AI gateway (the most feature-complete OSS MCP gateway)
**License:** Apache 2.0
**Language:** Python ~81% (FastAPI + Pydantic v2 + async SQLAlchemy), JavaScript ~8% (React admin UI), Rust ~4% (experimental MCP runtime/session core, A2A 1.0), HTML ~4%
**Current version:** v1.0.3 (June 10, 2026); GA v1.0.0 shipped May 1, 2026 after a long alpha/beta/RC train

## Positioning

"An AI Gateway, registry, and proxy that sits in front of any MCP, A2A, or REST/gRPC APIs, exposing a unified endpoint with centralized discovery, guardrails and management. Optimizes Agent & Tool calling, and supports plugins."

Four self-declared pillars:
1. **Tools Gateway** — MCP federation, REST/gRPC-to-MCP translation, TOON compression
2. **Agent Gateway** — A2A protocol, OpenAI-compatible + Anthropic agent routing
3. **API Gateway** — rate limiting, auth, retries, reverse proxy for plain REST
4. **Plugin extensibility + Observability** — 40+ plugins; OpenTelemetry to Phoenix/Jaeger/Zipkin/Tempo/DataDog/New Relic/Langfuse

## Full Feature Surface

### Federation & protocol translation
- Federates any number of upstream MCP servers behind one endpoint, with **namespaced tool federation** (tool names prefixed per gateway) to avoid collisions.
- **Multi-cluster federation** with Redis-backed caching/state; distributed tracing propagates across federated gateways.
- **UAID cross-gateway routing** (v1.0.1+): domain allowlists + bearer-token forwarding between gateways.
- **REST-to-MCP adapter**: wraps legacy REST APIs as MCP tools with automatic JSON Schema extraction; "REST passthrough" config.
- **gRPC-to-MCP translation**: zero-config via gRPC server reflection; protobuf↔JSON translation; TLS/mTLS; unary + server streaming.
- **A2A (Agent-to-Agent)**: register external AI agents (OpenAI, Anthropic, custom) as callable entities; Rust-backed A2A 1.0 support in GA.
- **TOON compression** for token-efficient tool payloads to LLMs.
- Auto health-checking of federated gateways (made concurrent in beta — O(n)→O(1) latency).

### Transports
- HTTP/JSON-RPC, streamable-HTTP, SSE (configurable keepalive), WebSocket, stdio.
- `mcpgateway.translate` — standalone protocol/transport bridge CLI (e.g., stdio↔SSE).
- `mcpgateway.wrapper` — stdio wrapper so any local MCP client (Claude Desktop etc.) can talk to the remote gateway.
- Reverse proxy mode for tunneling local MCP servers out through the gateway (disabled by default since RC2 for security).

### Registries (the "registry" half of the product)
- **Tools**: native MCP or adapted REST/CLI/gRPC; JSON Schema input validation (strict validation enforced at registration since RC1); concurrency controls; tool annotations.
- **Prompts**: Jinja2 templates, multimodal support, versioning + rollback.
- **Resources**: URI-based access, MIME detection, caching, SSE change notifications.
- **Virtual servers**: compose arbitrary subsets of tools/prompts/resources from multiple upstream servers into a new MCP server — the core curation primitive ("give this agent exactly these 12 tools").
- **MCP server catalog**: curated catalog with one-click registration.
- **Bulk import** + **configuration export/import** (with batch attribution tracking).
- **Tags system** for organizing tools/servers; metadata tracking (who created, when, from where, how; modification history; federation source tracking).

### Auth, governance, multi-tenancy
- JWT bearer (with JTI session claims; UUID subjects + JIT credential resolution in 1.0.3), email/password, Basic (off by default for APIs since RC1), custom header schemes, query-param auth, proxy auth.
- **OAuth 2.0** with Dynamic Client Registration (DCR); user-scoped OAuth tokens per tool; OAuth token validation via JWKS; OAuth secret at-rest protection; OIDC id_token cryptographic verification.
- **SSO**: GitHub, Google, Microsoft Entra ID (with role/group mapping), IBM Security Verify, Okta, Keycloak, generic OIDC.
- **RBAC + teams**: multi-tenant workspaces with isolated tool catalogs, team management, per-team visibility; plugin multi-tenancy (per-tool plugin config) since RC3.
- **End-user identity propagation** to downstream MCP servers (GA flagship feature) — the agent's human identity travels with the tool call.
- HTTP header passthrough (incl. unconditional `X-Upstream-Authorization`), multi-auth headers, AES-encrypted stored credentials.
- Guardrails: SSRF protection (strict by default since RC2 — blocks localhost/private nets), content-size limits (DoS), content security pattern detection (XSS/SQLi), password policies, nonce-based CSP, environment-aware secure defaults (prod requires strong secrets), well-known URI config.
- FedRAMP compliance mode + FIPS hardening, parameterized base images (v1.0.3); airgapped deployment support.

### Plugin framework (extensibility)
- 40+ plugins; framework extracted to an external package ("CPEX") in v1.0.1 — fully decoupled from gateway internals.
- Hook points across the whole request cycle: `prompt_pre/post_fetch`, `tool_pre/post_invoke`, `resource_pre/post_fetch`, `http_pre/post_request`, `agent_pre/post_invoke`.
- Native examples: PIIFilter (detect/mask PII), SearchReplace (regex transforms), DenyList, ResourceFilter (size/protocol/domain validation).
- **External plugins are themselves MCP servers**, callable over streamable HTTP, **gRPC (~4,700 calls/sec vs ~600 for HTTP)**, or Unix domain sockets; mTLS for external plugins; plugins scale independently of the gateway.
- Three authoring patterns: convention-based method names, `@hook` decorator, fully custom hook types; shared `PluginContext` state.
- Runtime plugin management with global on/off toggle (GA).

### Admin UI / dashboard
- Originally HTMX 2 + Alpine.js + Tailwind; **rewritten in React** (completed v1.0.2) — virtual server + user management components.
- Real-time log viewer (filter/search/export), config dashboard, full CRUD for tools/servers/gateways/prompts/resources, user/team admin, metrics views.
- Customization: section visibility, theme/branding; works airgapped; also packaged in an Electron shell.
- **LLM Chat**: a built-in MCP client/chat playground in the UI — pick a virtual server, pick an LLM (OpenAI, Azure OpenAI, Anthropic, AWS Bedrock, Ollama, watsonx.ai, OpenAI-compatible/vLLM/LocalAI), chat with streaming + transparent tool invocation; Redis-backed sessions for multi-worker. This makes the gateway a test bench for agent+tool combos.

### Observability
- OpenTelemetry (OTLP) vendor-agnostic tracing: Phoenix (LLM-focused), Jaeger, Zipkin, Tempo, DataDog, New Relic, Langfuse.
- Automatic instrumentation of tools/prompts/resources/gateway ops; distributed tracing across federated gateways; "zero overhead when disabled."
- LLM-specific metrics: token usage, costs, model performance.
- Prometheus metrics endpoint (`tool_calls_total`, `gateway_up`, …), structured JSON logs, health/readiness with dependency verification, DB performance observability, audit trails on every entity.

### API & CLI surface (agent-relevant)
- Full REST API for everything the UI does: `/tools`, `/servers` (virtual server CRUD), `/gateways` (federation registration), `/prompts`, `/resources`, `/health`, `/version`; Swagger UI at `/docs`, ReDoc at `/redoc`; JWT-protected by default.
- CLI: `mcpgateway` (server), `mcpgateway.translate`, `mcpgateway.wrapper`, JWT token mint utility (`create_jwt_token`), config validation, plugin management commands.
- **300+ documented environment variables**, `.env` support, Pydantic-validated settings; config export/import for promotion between environments.

### Deployment & scaling
- PyPI (`pip/pipx/uvx install mcp-contextforge-gateway`), Docker/Podman OCI images (GHCR; standard, lite, scratch variants), Docker Compose full stack, Helm charts for Kubernetes/OpenShift, Argo CD GitOps, Minikube; AWS ECS/EKS, Azure AKS, GCP Cloud Run, IBM Code Engine, Fly.io tutorials.
- SQLite default; PostgreSQL (psycopg3) for prod; MySQL supported; Redis for caching/federation/sessions; PgBouncer guidance.
- Gunicorn default; **Granian (Rust HTTP server) option: +20–50% perf, 3x throughput claim in beta notes**; multi-arch incl. s390x and ppc64le (IBM mainframe/Power).

### MCP server ecosystem (bundled sample servers)
- Go: calculator, fast-time, slow-time, pandoc. Python (15+): chunker, code-splitter, csv-pandas-chat, data-analysis, docx, evaluation, graphviz, latex, libreoffice, mermaid, plotly, powerpoint, sandbox, url-to-markdown, xlsx. External integrations: GitHub Copilot, Box, monday.com, IBM Instana, Terraform.

## Published performance numbers (docs/scaling guide + release notes)
- ~**800 RPS per pod** (8 workers); 10 pods ≈ 8,000 RPS; 1000+ concurrent requests per worker (async).
- Request validation <1ms (Pydantic v2 Rust core); orjson 5–6x faster serialization; auth overhead 5–12ms → <1ms with caching; auth cache cuts DB queries 3–4 → 0–1/request; registry cache 95%+ hit rate; overall DB load −80–95%; hiredis parser 83x faster on large responses; PgBouncer 8x connection reduction.
- External plugin transports: gRPC ~4,700 calls/sec vs ~600 HTTP.
- BETA-2 release: "100+ perf improvements," N+1 elimination (−90% queries in multi-gateway), Granian 3x throughput.
- 7,000+ tests claimed; OWASP/DAST, fuzzing, mutation testing in CI; 48+ ADRs.

## Agent-experience (AX) notes
- The product is explicitly agent-oriented: "Optimizes Agent & Tool calling." Virtual servers exist so agents get curated, minimal tool sets; TOON compression reduces token cost of tool schemas/results.
- A2A turns agents themselves into registry entities the gateway can route to; `agent_pre/post_invoke` plugin hooks give policy over agent calls.
- Everything in the Admin UI is also a JWT-protected REST API (Swagger/ReDoc) → fully scriptable/agent-drivable control plane; 300+ env vars = API-first config; config export/import enables GitOps.
- `mcpgateway.wrapper` (stdio) + `translate` bridge mean any MCP client — including coding agents — can consume the gateway from anywhere.
- Client docs cover MCP Inspector, Claude Desktop, GitHub Copilot, Cline, Continue, OpenWebUI, MCP CLI; agent-framework docs for LangChain, LangGraph, CrewAI, AutoGen, Semantic Kernel, OpenAI SDK, LlamaIndex, Bee.
- End-user identity propagation = the agent's human principal flows to downstream tools (key enterprise governance pattern competitors lack).
- No dedicated "agent-first CLI" though — control plane is REST + Admin UI; no MCP-based *management* interface for the gateway itself (agents manage it via REST, not via MCP tools).

## Weaknesses / complaints
- **Heavy operational footprint**: Python + PostgreSQL + Redis + PgBouncer + Nginx for production; third-party comparisons say it's "better suited for organizations with strong internal DevOps expertise"; 300+ env vars is a config surface users must learn.
- **Python performance ceiling**: per-pod ~800 RPS requires 8 workers; scaling guide is largely about compensating (caching layers, PgBouncer, Granian, orjson, hiredis); confirmed memory-pressure bug under 1000+ RPS from httpx.AsyncClient churn (issue #1731) + plugin client leaks; a docs page exists for "CPU spin loop mitigation" (telling).
- **Long pre-GA period**: labeled alpha/beta "no commercial support" through 2025; HN commenters mocked IBM shipping a 0.6.0 alpha with solo-dev-style caveats; GA only arrived May 2026.
- **Churny architecture**: Admin UI rewritten (HTMX→React), plugin framework extracted mid-flight (CPEX), Rust runtime "experimental," modular-runtime migration ongoing — moving target for operators and plugin authors.
- **Security posture earned late**: SSRF defaults were permissive until RC2; basic auth/public registration on by default until RC1; 40+ security controls "tightened" in RC2 implies prior gaps.
- Not a single binary — Python app + sidecar services; lite/scratch containers mitigate but don't equal a Go/Rust single-binary story.
- No LLM-provider gateway: it routes *agents and tools*, not model inference (LLM Chat consumes external providers but there's no unified LLM API/keys/cost-routing layer) — pairing with LiteLLM is the documented pattern (OpenWebUI+LiteLLM+ContextForge tutorial).
- Quality-of-life bugs in tracker: 500-instead-of-400 on bad gateway names, dependency-update chores across the polyglot sample-server zoo; very large backlog (issues #2502+ are meta "backlog guide" issues).
- Crowded category: HN thread lists MetaMCP, hyper-mcp, Wassette, Hypr MCP, Unla, Docker MCP Gateway etc.; ContextForge differentiates on completeness, not simplicity.

## Pricing
None — pure OSS (Apache 2.0), no commercial/managed edition announced. IBM monetizes adjacently (watsonx, consulting); docs integrate watsonx.ai and IBM Security Verify.

## What to steal vs. what to beat
**Steal:** virtual servers as the curation primitive; external plugins-as-MCP-servers over gRPC/UDS with pre/post hooks at every lifecycle point; end-user identity propagation; REST/gRPC-to-MCP auto-adaptation (schema extraction / server reflection); LLM Chat playground bound to virtual servers; metadata/audit attribution on every entity; config export/import; TOON-style token compression.
**Beat on:** single-binary Go/Rust simplicity (their #1 weakness is operational weight and Python perf); first-class unified LLM gateway in the same binary (they have none); an MCP-native control plane (manage the gateway *via* MCP tools, not just REST); sane secure defaults from day one; sub-minute zero-dependency quickstart (no Postgres/Redis required for real use).

## Sources
- https://github.com/IBM/mcp-context-forge (README, releases, issues #1731/#1203/#251/#2502)
- https://ibm.github.io/mcp-context-forge/latest/ (features, plugins, LLM chat, scaling, FAQ)
- https://developer.ibm.com/blogs/context-forge-mcp-gateway-now-available/
- https://news.ycombinator.com/item?id=45010524 (MCP gateway thread)
- https://www.truefoundry.com/blog/best-mcp-gateways ; https://dev.to/kuldeep_paul/best-mcp-gateways-to-connect-tools-and-mcp-servers-to-your-ai-agent-536m ; https://www.mintmcp.com/blog/portkey-with-mcp
