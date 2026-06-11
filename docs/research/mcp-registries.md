# Competitive Intel: Smithery + Glama + PulseMCP (MCP Registries / Marketplaces)

Researched 2026-06-10. Subject: the three dominant MCP registry/marketplace/hosting players, evaluated as competitors/adjacent infrastructure to a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

---

## 1. Smithery (smithery.ai)

### Positioning
Started as "the MCP app store" — the best-known hosted MCP registry (~7,300+ servers as of May 2026). Now repositioning: homepage tagline is **"Turn scattered context into skills for AI"** and the platform has added a **Skills Registry** alongside MCP servers — a notable strategic pivot toward agent skills, mirroring the Claude Skills wave. Tagline elsewhere: "The Infrastructure for AI Agents."

### Feature surface

**Registry & discovery**
- Public catalog of 7,300+ MCP servers, organized in categories (development tools, data connectors, productivity, AI utilities); plus a separate Skills Registry.
- Search API: `GET https://api.smithery.ai/servers` (Bearer API key) with **full-text + semantic search** (`q`), filters: `namespace`, `qualifiedName`, `remote`, `isDeployed`, `verified`, `ownerId`, `repoOwner`, `repoName`, `ids`; pagination (`page`, `pageSize` 1–100, `topK` 10–500 candidates), `fields` selection, and a `seed` param for **deterministic sort across paginated requests** (explicitly designed for agents that paginate).
- Server objects expose: id, qualifiedName, namespace, slug, displayName, description, iconUrl, homepage, owner, `verified`, `isDeployed`, `remote`, `bySmithery`, `useCount` (popularity), relevance `score`.
- Docs are agent-readable: every docs URL serves markdown with `.md` appended; `smithery.ai/docs/llms.txt` is a full machine-readable index; docs themselves are exposed as an MCP server at `https://smithery.ai/docs/mcp`.

**Connect API (the gateway-ish product)** — REST API that abstracts the MCP protocol entirely:
- Connections: create / create-or-update / get / list / delete connection; get/list tools; **call tool over REST** (no MCP client needed); list tools across a namespace.
- **Triggers**: list/get/subscribe/unsubscribe — event/webhook-style subscriptions on servers (a registry doing eventing is unusual).
- Health check endpoint.
- Two connection URL patterns: per-connection `mcpUrl`, and **namespace aggregation**: `https://mcp.smithery.run/{namespace}` bundles all connections in a namespace into ONE MCP endpoint, with tool names prefixed by connection ID (`notion-personal.search`, `user-123-github.search_repositories`) for uniqueness. This is effectively an MCP gateway/multiplexer.
- **Managed OAuth**: create connection → `auth_required` + hosted `setupUrl` → user authorizes upstream → credentials stored. No OAuth app registration, redirect URIs, or token exchange for the integrator. Automatic token refresh. Credentials are **write-only** (can never be read back, only used).
- Config-needed servers return `input_required` with a schema listing missing fields; client can redirect to hosted setup page or supply values programmatically (headers/query params).

**Auth & multi-tenancy**
- **API keys** (backend, full namespace access) vs **service tokens** (client-safe, short-lived, policy-scoped: namespaces, resources, operations, metadata match e.g. `metadata.userId == user-123`, TTL). Token scoping is a documented first-class concept.
- **Namespaces** = globally unique app/environment identifiers grouping connections (e.g. `my-app`, `my-app-staging`); connections carry arbitrary metadata for multi-user filtering.
- Organizations API: team API keys (create/list/revoke).

**Hosting / publishing (in flux)**
- Servers API: create/update/delete/publish/transfer servers, releases (get/list/resume, stream release logs), runtime logs, server bundles (downloadable), icon management, **"infer a tool output schema"** endpoint.
- Historically: free hosted deployments (Docker build from repo) with hosted OAuth modals, setup in <1 min.
- **Major change (announced ~late 2025): Smithery is killing the free hosting plan — free-plan servers live only until March 1, 2026** — and "rebuilding the hosting platform from the ground up" citing architectural constraints. Path forward: paid hosting plans or register an **external server** for free. Observability-tab insights remain free for both hosted and external servers.
- **Uplink**: expose a *local* MCP server (stdio or HTTP) as a regular cloud Smithery connection via a persistent WebSocket tunnel from the CLI — no deployment. Transparent to stateful sessions/progress/streaming. Reports connected/disconnected/error; fails fast when the laptop goes offline (explicitly not for production). Great dev-loop feature.

**CLI** (`npm i -g smithery`, TypeScript, Node 20+, **AGPL-3.0**)
- `smithery mcp search/add/list/remove/publish`, `smithery tool list/find/get/call` (find = search tools by intent), `smithery auth login/logout/whoami/token` (mint service tokens from CLI), `smithery namespace list/use`, plus skills commands (`skill search/add`, upvote/downvote/review). The CLI is itself an agent-first control plane: an agent can search the registry, add a connection, and call a tool entirely from the shell.

**Other**
- Deep Linking spec (one-click install links into clients), "Listing Your Client" program, Vercel AI SDK integration guide + TypeScript SDK (`@smithery/api`), OAuth-client cookbook, status page.

### Pricing
Registry/discovery free. Vendor hosting plans: Hobby → **Pro $99/mo (100 user accounts, 10K MCP calls/mo)** → **Team $499/mo (500 accounts, 100K calls/mo)** → Enterprise custom. (Free hosting being retired 2026-03-01.)

### OSS / language
Platform is closed-source SaaS. CLI open source (TypeScript, AGPL-3.0); SDKs published; registry itself not self-hostable.

### Performance claims
None published (no latency/throughput numbers). "Setup in under a minute" is the only quantified UX claim.

### Weaknesses / complaints
- **Critical security incident (June 2025)**: path-traversal in the Docker build pipeline exposed an overprivileged auth token — could have compromised **3,000+ hosted servers and thousands of user API keys** (GitGuardian: "from path traversal to supply chain compromise"). Fixed in 2 days, keys rotated, but it's the canonical MCP-hosting supply-chain cautionary tale.
- **Registry quality/moderation**: independent scans found **22 of the top 100 Smithery servers had security findings** (4 critical, 24 high), most commonly tool-description injection; at least one trojanized repo passed with no moderation review; UpGuard found ~1-in-15 MCP servers ecosystem-wide are lookalikes/squatters. Largely unmoderated submission.
- **Hosting whiplash**: building a business on free Smithery hosting just got rug-pulled (March 2026 shutdown); trust cost with server developers, and the "rebuild from the ground up" admission signals the original hosting architecture didn't scale.
- `useCount` popularity metrics are gameable / criticized as inflated; lots of low-quality duplicate servers.
- AGPL CLI deters some corporate embedding.

### Agent-experience (AX) notes
Best-in-class among the three: semantic search API with deterministic pagination seed; `.md`-suffix docs + llms.txt + docs-as-MCP-server; CLI `tool find` (intent-based tool search) and `tool call`; REST tool-calling so agents don't need an MCP client; scoped short-lived service tokens designed to be handed to agents; namespace endpoint = one MCP URL for a whole toolbox.

---

## 2. Glama (glama.ai)

### Positioning
"The MCP Server Registry, Inspector & Gateway." Largest-volume index (~34,000 open-source servers listed; "10,000+ scanned and scored" claim; 21k+ cited by third parties). Solo-founder-driven (Frank Fiegel / @punkpeye), moved the whole company onto MCP ("even our homepage redirects to the MCP directory"). Claims "trusted by 50,000+ businesses" (Databricks, Shopify, Cloudflare logos). Uniquely, Glama is **both an LLM gateway and an MCP registry/gateway** — the closest existing shape to the product being built.

### Feature surface

**Registry & discovery**
- Auto-indexes every public MCP server it can find (GitHub crawling) — a **superset of the official MCP Registry** with much deeper per-connector data: health checks, **quality scores** (security, compatibility, ease-of-use ranking), security audits, tool schemas with annotations, usage telemetry, license info, README rendering, visual previews, "claim your server" flow for maintainers (claim → manage Dockerfile/listing).
- **Tool-level search across the whole registry**: search every tool exposed by every server ("query Postgres", "send email") instead of picking a server first — a genuinely differentiated discovery model.
- Distinguishes remotely-hostable vs local-only servers.
- Public API: `https://glama.ai/api/mcp/v1/servers/{owner}/{server}` (per-server metadata); browseable categories; daily updates.

**MCP hosting + gateway (control plane)**
- Deploy from: registry (one-click), GitHub repo (GitHub App), or custom package (Dockerfile, npm, PyPI). Dedicated machines per deployment (no resource sharing). **stdio servers auto-wrapped into Streamable HTTP** — no transport rewrite.
- Gateway in front of every hosted server: **managed OAuth 2.1 credentials with auto-refresh, per-tool access control (enable/disable individual tools), full JSON-RPC request/response logging, searchable/filterable logs, live runtime log tailing, tool-call analytics by tool/user/time (1h–90d windows), MCP-aware health checks** (verifies the MCP handshake, not generic HTTP probe).
- Env vars encrypted at rest, decrypted only in memory at startup; access tokens scoped to connection profiles; deployments private by default, optionally listed publicly.
- The hosted Glama URL can be pasted into Claude, ChatGPT, or Cursor — Glama acts as the credentialed middleman.

**Inspector**
- Browser-based MCP inspector: paste any MCP server URL, debug from the browser, ephemeral sandbox, **no login required, state persisted in the URL** for shareable repro links. Supports every MCP feature: tools, resources, prompts, tasks, elicitation, sampling, progress notifications, OAuth 2.1 + Dynamic Client Registration.

**LLM gateway + chat**
- **OpenAI-compatible API gateway** over models from OpenAI, Anthropic, Google, DeepSeek, Mistral, xAI, etc. (use official OpenAI SDKs against it).
- ChatGPT-like chat UI that can use any MCP server you own (your hosted connectors become chat tools); custom agents, automations, projects with shared memory, file uploads, web search/fetch tools.

### Pricing
- **Free**: open-source MCP servers deploy/run free; directory and inspector free.
- **Starter $9/mo**: $9 AI credits, 3 fast servers (+$4 ea), 100k logs/mo (+$9/100k), 30-day retention, unlimited MCP connectors, no rate limits.
- **Pro $26/mo**: 10 servers (+$3 ea), persistent storage, 90-day logs, file uploads, projects/shared memory, web search+fetch.
- **Business $80/mo**: 30 servers (+$2 ea), 180-day logs, custom exports, priority support, request labeling.
- Billed **per server instance, not per request/tool call — no usage caps**; infra metered by machine-hours + egress.

### OSS / language
Platform closed-source SaaS (founder pledges directory "will always be free and open" as a directory, but code is proprietary). Frank Fiegel maintains popular OSS side artifacts (e.g., `punkpeye/awesome-mcp-servers`, FastMCP for TypeScript). Stack publicly described as TypeScript/Node.

### Performance claims
None published (no latency/throughput numbers). Scale claims only: ~34k servers indexed, 50k+ businesses.

### Weaknesses / complaints
- **Auto-indexing without consent**: maintainers discover their repos listed (with quality scores/badges) and must "claim" listings to fix metadata (e.g., wanaku-ai issue about missing Dockerfile); some view the scoring of unclaimed repos + claim-email outreach as growth-hacky.
- Quality scores/security "audits" are automated, shallow, and not independently validated; with 34k auto-indexed entries the long tail is abandonware.
- Solo-founder concentration risk for an infra product; no SLA language found.
- Closed source: can't self-host the gateway/control plane — a direct opening for an OSS competitor.
- Chat/LLM-gateway product blurs focus (consumer chat workspace + dev infra in one brand).
- Per-instance pricing means idle hosted servers still cost money; dedicated-machine model is less elastic than serverless rivals.

### Agent-experience (AX) notes
Tool-level registry search is the strongest "what agents query" primitive (capability-first, not server-first). Public per-server metadata API. Inspector with URL-persisted state is shareable into agent workflows. OpenAI-compatible LLM API + MCP gateway under one roof = one credential for models and tools. Weaker than Smithery on machine-readable docs/llms.txt and on a scoped-token story for handing credentials to agents.

---

## 3. PulseMCP (pulsemcp.com)

### Positioning
"Keep up-to-date with MCP." The **community/curation layer**: largest hand-reviewed directory (17,600+ servers, updated daily), 394+ MCP **client** directory, use-case showcase (20+ worked examples), and the influential Weekly Pulse newsletter ("The Agentic Loop"). Founders Tadas Antanavicius and Mike Coffin sit on the **MCP Steering Committee**; Tadas is a **maintainer of the Official MCP Registry** — PulseMCP is quasi-official infrastructure, not just a media site. Not a gateway and not a host: it's data + distribution.

### Feature surface

**Directory & curation**
- 17,620+ MCP servers, hand-reviewed/curated, updated daily; sources: manual submissions + automated scraping/crawling with manual curation + Official MCP Registry ingestion + enrichment feeds (download counts, popularity).
- Client directory (394+ apps/tools that act as MCP clients) — unique; nobody else catalogs the client side.
- Use Case Showcase: 20+ end-to-end MCP use cases (server+client pairings solving real problems).
- Newsletter as the ecosystem's de facto changelog.

**Sub-Registry API (v0.1) — the productized asset** (base `https://api.pulsemcp.com`)
- Implements the **Generic MCP Registry API spec** (the official registry's API shape) with namespaced `_meta` extensions — i.e., drop-in compatible with anything that speaks the official registry API.
- Endpoints: `GET /v0.1/servers` (cursor pagination, `limit` 1–100, `updated_since` RFC3339 incremental sync, `search` substring, `version=latest`), `GET /v0.1/servers/{name}/versions`, `GET /v0.1/servers/{name}/versions/{version}`, plus `/health`, `/ping`, `/version`.
- Auth: `X-API-Key` + `X-Tenant-ID` (tenant isolation; partner/B2B model, contact-us onboarding).
- Rate limits: 200/min, 5,000/hr, 10,000/day with `X-RateLimit-*` headers; explicit "no SLA for live calls — cache locally"; documented sync strategies (full ETL with `updated_since`, or latest-only cache) and soft-delete semantics (`status: deleted`).
- **Enrichments** in `_meta["com.pulsemcp/server"]` / `server-version`: visitor estimates (7d/28d/all-time), `isOfficial` (maintained by the service owner), source provenance (official registry vs pulsemcp), status lifecycle (active/deprecated/deleted + statusChangedAt/message), `isLatest`, standardized icons (src/mime/sizes/theme).
- **Premium enrichments** (paid tier): `isSelfHosted` detection for remotes; **`authOptions[]`** per remote/package (open / oauth with RFC 9728 Protected Resource Metadata / api_key with source location / other) — i.e., machine-readable "how do I authenticate to this server"; **`tools[]`** — actual MCP Tool objects (name/description/inputSchema) **discovered by live-connecting to servers**. This is verified ground truth, not scraped READMEs.
- OpenAPI spec downloadable (`openapi_v01.yaml`); immutability semantics documented (official-registry files immutable; pulsemcp files mutable with updatedAt).

**Agent access**
- Community MCP server (`orliesaurus/pulsemcp-server`, MIT) lets agents list/search PulseMCP servers with filtering and pagination; PulseMCP's own data also surfaces via the official registry ecosystem.

### Pricing
Directory/newsletter free. Sub-Registry API is partner/B2B: API-key onboarding via hello@pulsemcp.com; "premium enrichments" tier (live-tested tool lists, auth options); no public price list.

### OSS / language
Site and pipeline closed-source; API conforms to the open Generic Registry API spec; community MCP server MIT. Implementation language not published (founders publicly use Goose agents to run curation ops).

### Performance claims
None (no latency numbers). Scale: 17,620+ servers, 394+ clients, daily updates; rate limits as above.

### Weaknesses / complaints
- No gateway, no hosting, no runtime: pure metadata — depends on others (gateways, marketplaces) as customers; doesn't own the request path.
- Hand-review doesn't scale to 17k+ entries; "hand-reviewed" is increasingly agent-assisted (they say Goose does the tedious bits).
- API is gated/contact-sales (no self-serve key), read-only, v0.1, explicit no-SLA — not dependable as a live backend.
- Visitor-estimate popularity is proxy data (their own page traffic), not real install/usage telemetry.
- Tiny team; newsletter-driven brand could fade as the Official Registry matures and absorbs the aggregation role (their own maintainers are building that registry).

### Agent-experience (AX) notes
The enrichment schema is exactly what an agent needs to choose and connect to a server autonomously: machine-readable `authOptions` (can I auth without a human?), live-verified `tools[]` with inputSchemas (what can it do, really?), `isOfficial`/visitor counts (trust signal), `isSelfHosted` (can I actually reach it?). Spec-compatibility with the official Generic Registry API means one client implementation reads both. But: no MCP-native first-party endpoint, no self-serve keys.

---

## Cross-cutting synthesis for our gateway

**Table stakes across this category** (users now assume these):
- Searchable registry with categories, popularity signals, icons, README rendering, official/verified badges
- One-click / one-command install into major clients (Claude, Cursor, etc.) + deep links
- Remote (Streamable HTTP) endpoints with managed OAuth; stdio→HTTP wrapping
- A CLI that can search, add, list, remove servers
- Programmatic registry API with pagination + filtering; official-registry interop
- Per-server metadata: tools exposed, auth requirements, hosting status, maintenance status

**Best ideas worth stealing**
1. Smithery's **namespace aggregation endpoint** (one MCP URL = whole prefixed toolbox) and **scoped short-TTL service tokens with metadata policies** — the right credential shape for handing tools to agents.
2. Smithery's **Uplink** (WebSocket tunnel exposing a local stdio server as a cloud connection) — kills the local/remote divide for dev loops.
3. Glama's **tool-level search across the entire registry** (capability-first discovery) and **per-tool enable/disable at the gateway**.
4. Glama's **no-login browser inspector with URL-persisted state**, and MCP-aware health checks (handshake-verifying, not HTTP 200).
5. PulseMCP's **live-verified `tools[]` + machine-readable `authOptions` enrichments** and `updated_since` ETL semantics — registry data agents can act on without a human.
6. Smithery's agent-readable docs stack: `.md` URL suffix, llms.txt, docs-as-an-MCP-server.
7. Glama's combined LLM-gateway (OpenAI-compatible) + MCP control plane — validates the unified-gateway thesis; nobody does it open-source.

**Structural gaps an OSS single-binary gateway exploits**
- All three are closed-source SaaS control planes; none can be self-hosted (Smithery's only OSS piece is an AGPL CLI). Enterprises burned by the Smithery supply-chain near-miss (3,000+ servers exposed) and the free-hosting rug-pull want the gateway inside their VPC.
- Registry trust is the open wound: 20–66% of scanned servers have security findings, lookalike/typosquat servers pass moderation, popularity counts are gameable. A gateway that pins versions, verifies tool schemas at runtime, and enforces per-tool policy locally is the antidote.
- Nobody publishes performance numbers — proxy latency/overhead is an open benchmark battlefield.
- PulseMCP proves the registry-data layer can be consumed via a standard spec (Generic Registry API): an OSS gateway should *consume* official+PulseMCP+Smithery+Glama registries behind one search rather than build a directory.
