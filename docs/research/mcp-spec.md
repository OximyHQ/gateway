# Official MCP Spec Evolution + Registry — Competitive Intelligence Report

Research date: 2026-06-10. Subject: modelcontextprotocol.io specification (latest stable revision **2025-11-25**; release candidate **2026-07-28**, locked 2026-05-21, final ships 2026-07-28) and the official MCP Registry (registry.modelcontextprotocol.io, still in **preview** with a v0.1 API freeze). This is the "dimension" report defining what a spec-max MCP gateway must support.

---

## 1. Spec revision timeline

| Revision | Status | Headline |
|---|---|---|
| 2024-11-05 | Deprecated transport era | Original spec; HTTP+SSE transport (separate SSE + POST endpoints, `endpoint` event) |
| 2025-03-26 | Superseded | Introduced Streamable HTTP transport, OAuth (AS-on-server model), audio content |
| 2025-06-18 | Superseded | OAuth 2.1 resource-server split (RFC 9728 Protected Resource Metadata), elicitation (form), structured tool output, resource links, removed JSON-RPC batching |
| **2025-11-25** | **Current stable** | OIDC discovery, Client ID Metadata Documents, URL-mode elicitation, sampling-with-tools, icons, experimental Tasks, SSE polling, JSON Schema 2020-12, SDK tiering, formal governance |
| **2026-07-28 (RC)** | RC locked 2026-05-21, final 2026-07-28 | **Stateless protocol core** (no initialize handshake, no session header), Extensions framework, MCP Apps, Tasks→extension, auth hardening (6 SEPs), formal deprecation policy (Roots/Sampling/Logging deprecated) |

Versioning: date-based (`YYYY-MM-DD`); negotiated at initialize (≤2025-11-25) and carried on every HTTP request via `MCP-Protocol-Version` header. Missing header ⇒ server assumes `2025-03-26`. Invalid/unsupported ⇒ 400.

---

## 2. Transports (2025-11-25)

Two standard transports; messages are UTF-8 JSON-RPC 2.0 (batching removed since 2025-06-18). Custom transports allowed if they preserve JSON-RPC framing and lifecycle.

### stdio
- Client launches server as subprocess; newline-delimited JSON-RPC on stdin/stdout; stderr free-form for ALL logging levels (clarified in 2025-11-25 — not just errors). Clients SHOULD support stdio whenever possible.

### Streamable HTTP (replaces HTTP+SSE from 2024-11-05)
- **Single MCP endpoint** (e.g. `/mcp`) supporting POST + GET (+ optional DELETE for session termination).
- Every client JSON-RPC message = a new HTTP POST. `Accept: application/json, text/event-stream` required.
- Server replies to a request with either a single JSON object (`application/json`) or an **SSE stream** (`text/event-stream`); clients MUST support both.
- POST of notifications/responses ⇒ `202 Accepted`, no body.
- Server-initiated messages: client MAY open GET → SSE stream; server MAY send requests/notifications there; 405 if unsupported.
- Server MAY interleave server→client requests/notifications before the response on a POST-initiated stream (this is how nested sampling/elicitation arrive mid-tool-call).
- **Resumability**: SSE event `id`s act as per-stream cursors, globally unique per session; resume via GET + `Last-Event-ID`; server replays only messages from THAT stream; resumption is always GET regardless of original stream origin.
- **Polling SSE (SEP-1699, new in 2025-11-25)**: server may prime the client with an empty event-ID event, then deliberately drop the connection (sending `retry: <ms>`) without terminating the logical stream — clients poll/reconnect. Kills the long-lived-connection requirement.
- Disconnect ≠ cancel; cancel requires explicit `CancelledNotification`.
- **Session management**: server MAY return `MCP-Session-Id` on InitializeResult (cryptographically secure, visible-ASCII); client MUST echo it on every request; 400 if missing when required; 404 ⇒ client must re-initialize fresh session; client SHOULD DELETE to end session (server MAY 405).
- **Security hard requirements**: validate `Origin` (403 on invalid — DNS-rebinding defense, made MUST in 2025-11-25), bind localhost servers to 127.0.0.1, authenticate all connections.
- **Backwards compat**: servers wanting old-client support keep legacy SSE+POST endpoints; clients probe POST-initialize first, fall back to GET expecting the legacy `endpoint` event.

### 2026-07-28 RC transport changes (the big one)
- **Stateless core**: `initialize`/`initialized` handshake and `Mcp-Session-Id` are GONE. Protocol version, client info, and capabilities travel in `_meta` on **every request**. Any server instance can serve any request — designed for round-robin LBs, serverless, autoscaling.
- Server→client requests become **multi-step exchanges**: instead of pushing over a persistent SSE stream, servers return `InputRequiredResult` (prompt + opaque state); client answers with echoed state; any instance can handle the retry.
- New routing headers **`Mcp-Method`** and **`Mcp-Name`** so LBs/gateways/rate-limiters can route/limit per operation without parsing bodies.
- List + resource-read results gain **`ttlMs` / `cacheScope`** caching metadata (Cache-Control-modeled) — gateways can legally cache tool lists.
- **W3C Trace Context** (`traceparent`, `tracestate`, `baggage`) formally documented in `_meta` (SEP-414) for cross-SDK/gateway distributed tracing.
- Tool input/output schemas upgraded to **full JSON Schema 2020-12** incl. `oneOf/anyOf/allOf`, conditionals, `$ref`.
- Conformance suite requirements (SEP-2484).

---

## 3. Authorization (OAuth 2.1) — 2025-11-25

Optional but normative for HTTP transports; stdio uses environment credentials instead.

- **Roles**: MCP server = OAuth 2.1 *resource server only*; authorization server is separate (huge change from 2025-03-26 when the MCP server was expected to BE the AS).
- **Standards stack**: OAuth 2.1 draft-13, RFC 8414 (AS metadata), RFC 7591 (DCR), RFC 9728 (Protected Resource Metadata), draft client-id-metadata-document-00, OIDC Discovery 1.0 (new in 2025-11-25).
- **Discovery chain a client must implement**: unauthenticated request → 401 + `WWW-Authenticate: Bearer resource_metadata="…", scope="…"` → fetch PRM (or fall back to `/.well-known/oauth-protected-resource[/path]` probing) → pick AS from `authorization_servers` → probe AS metadata in priority order (oauth-authorization-server with path-insertion → openid-configuration path-insertion → openid-configuration path-appending).
- **Client registration, 3 mechanisms with priority order**: (1) pre-registered creds, (2) **Client ID Metadata Documents** (client_id = HTTPS URL to a JSON metadata doc; advertised by `client_id_metadata_document_supported: true`; SHOULD-level, the new default for stranger-to-stranger), (3) Dynamic Client Registration (RFC 7591, demoted to backwards-compat MAY), (4) prompt the user.
- **PKCE**: mandatory, S256; clients MUST refuse to proceed if `code_challenge_methods_supported` absent from AS metadata (including OIDC providers).
- **Resource Indicators (RFC 8707)**: `resource` param MUST be sent on authorization + token requests with the canonical MCP server URI; servers MUST validate audience; **token passthrough explicitly forbidden** — an MCP server must never forward the client's token upstream.
- **Scopes**: `WWW-Authenticate` scope hints; incremental/step-up consent (SEP-835): 403 + `error="insufficient_scope"` + required scopes → client re-authorizes with the expanded set, with retry limits. `scopes_supported` = minimal baseline.
- Bearer token in `Authorization` header on EVERY request (never query string); 401 invalid/expired, 403 insufficient scope, 400 malformed.
- Security considerations spelled out: token theft, refresh-token rotation for public clients, open-redirect, CIMD SSRF + localhost-redirect impersonation, confused deputy for proxy servers (proxy MUST get per-client user consent), audience validation.
- **Authorization extensions** live in a separate `ext-auth` repo (optional, additive, composable, independently versioned) — e.g. enterprise SSO patterns.
- **2026-07-28 RC hardening (6 SEPs)**: mandatory `iss` validation (RFC 9207), OIDC `application_type` at registration, credentials bound to issuing AS, refresh-token/scope-accumulation guidance, clarified `.well-known` docs.

---

## 4. Server features (what servers expose)

- **Tools**: `tools/list` (paginated, cursor-based), `tools/call`, `listChanged` notifications; structured output (`outputSchema` + `structuredContent`); tool annotations (readOnlyHint, destructiveHint, idempotentHint, openWorldHint — untrusted hints); content types: text, image, audio, resource_link, embedded resource; **execution errors returned in-band (`isError: true`) not as protocol errors** so the model can self-correct (SEP-1303); tool-name guidance (SEP-986); icons (SEP-973).
- **Resources**: `resources/list`, `resources/read`, `resources/templates/list` (RFC 6570 URI templates), subscribe/unsubscribe + `updated` notifications, listChanged; mimeType, size, annotations (audience, priority, lastModified).
- **Prompts**: `prompts/list`, `prompts/get` with arguments; multimodal message content; listChanged.
- **Completions**: `completion/complete` — argument autocompletion for prompts/resource templates (capability `completions`).
- **Logging**: `logging/setLevel` + `notifications/message` (RFC 5424 levels). *Deprecated in the 2026-07-28 RC → stderr/OTel.*
- **Tasks (experimental, SEP-1686, new in 2025-11-25)**: durable request tracking — call returns a task handle; poll status; deferred result retrieval. *RC moves Tasks out of core into an extension*: `tools/call` returns a handle, client drives via `tasks/get` / `tasks/update` / `tasks/cancel`; `tasks/list` removed.
- **Pagination**: opaque cursor model across all list operations.
- **Utilities**: ping, cancellation (`notifications/cancelled`), progress (`notifications/progress` with progressToken, message, total), `_meta` extensibility.

## 5. Client features (what clients expose to servers)

- **Sampling** (`sampling/createMessage`): server-initiated LLM calls through the client — no server API keys; model preferences = cost/speed/intelligence priorities (0-1) + substring model hints, client makes final choice; text/image/audio content; **sampling-with-tools (SEP-1577, new in 2025-11-25)**: `tools` + `toolChoice` (auto/required/none) under `sampling.tools` capability, multi-turn tool loop, parallel tool_use, strict tool_result message-purity rules for cross-provider compat (OpenAI tool role / Gemini function role); `includeContext` soft-deprecated; human-in-the-loop SHOULD on every leg. *Whole feature deprecated in 2026-07-28 RC → direct LLM provider APIs.*
- **Elicitation** (`elicitation/create`): two modes.
  - **Form mode**: flat-object JSON Schema subset (string w/ formats email|uri|date|date-time, number, boolean, single+multi-select enums w/ titles via oneOf/anyOf const, defaults — SEP-1330/1034); accept/decline/cancel three-action model; MUST NOT collect secrets via forms.
  - **URL mode (SEP-1036, new in 2025-11-25)**: out-of-band browser flows for secrets/payments/third-party OAuth; `elicitationId`, `notifications/elicitation/complete`, dedicated error `-32042 URLElicitationRequiredError` with retryable elicitation list; heavy client rules (no prefetch, explicit consent, show full URL, open in non-inspectable browser context, punycode warnings) and server anti-phishing rules (bind elicitation to user identity, never pre-authenticated URLs). This is the spec-blessed pattern for **server-as-OAuth-client to third-party APIs** without token passthrough.
- **Roots** (`roots/list` + listChanged): filesystem boundary URIs (file:// today) the server may operate in. *Deprecated in 2026-07-28 RC → tool params / resource URIs / server config.*

## 6. Lifecycle & capability negotiation (≤2025-11-25)

initialize (protocolVersion, capabilities, clientInfo w/ new optional `description` field aligned to registry server.json) → InitializeResult → `notifications/initialized`. Capabilities: server = prompts, resources(+subscribe), tools, logging, completions, experimental(tasks); client = sampling(+tools,+context), elicitation(form/url), roots(+listChanged). The RC kills this entire phase — capabilities ride in `_meta` per-request.

---

## 7. Official MCP Registry

- **What**: official centralized **metadata** repository for publicly accessible MCP servers — registry.modelcontextprotocol.io. Backed by Anthropic, GitHub, PulseMCP, Microsoft. Launched preview 2025-09-08; **API freeze at v0.1 (2025-10-24)**; still labeled preview as of June 2026 (no durability guarantees, GA "later"). Open source, community-driven, **Go 1.24 + PostgreSQL**, ~6.9k stars, permissive license.
- **server.json**: standardized manifest — reverse-DNS name (`io.github.user/server`), package pointer (npm/PyPI/NuGet/Docker/MCPB) OR remote URL, runtime args/env vars, transport, description, version. JSON Schema validated; same format the spec's `Implementation.description` aligns to.
- **Namespacing/auth**: GitHub OAuth or GitHub OIDC (CI publishing) for `io.github.*`; DNS or HTTP challenge for custom domains (`com.example/*`). Only namespace owners can publish.
- **Publishing**: `mcp-publisher` CLI; OpenAPI-spec'd REST API for reads.
- **Architecture stance**: registry → consumed by **downstream aggregators/marketplaces** (hourly-ish ETL), NOT directly by host apps; metadata deliberately unopinionated (no ratings/curation); subregistries + private registries implement the same **OpenAPI interface** for client compat; official codebase explicitly *not designed for self-hosting*; no private servers (host your own registry for those).
- **Trust model**: namespace auth + field validation + manual takedown (moderation policy); security scanning delegated to package registries and aggregators — **the registry does not scan server code**.
- **Known problems**: duplicate-version publishing allowed → CI re-publish floods (one analysis: ~64.7M entries over ~1.7k unique npm/PyPI packages across the meta-registry ecosystem); spam/typosquatting concerns; malicious servers in (mostly unofficial) registries — e.g. trojanized Oura Ring server (Feb 2026), Snyk found 36.8% of 3,984 scanned agent skills had findings, 76 confirmed malicious; future plans = stricter rate limiting, AI spam detection, community reporting.

---

## 8. What a spec-max gateway MUST support (the checklist)

**Today (2025-11-25):**
1. Streamable HTTP both directions: POST request→JSON-or-SSE, GET server-push stream, SSE resumability (`Last-Event-ID`, per-stream event-ID cursors), polling-SSE with `retry`, 202 semantics, DELETE session termination.
2. Session affinity or shared session store for `MCP-Session-Id` (the #1 gateway pain today); 404→re-init dance.
3. `MCP-Protocol-Version` header handling + multi-version negotiation (and assume 2025-03-26 when absent); legacy HTTP+SSE fallback for old servers/clients.
4. Full OAuth 2.1 RS behavior: serve/proxy RFC 9728 PRM, pass through `WWW-Authenticate` challenges (incl. `insufficient_scope` step-up), audience validation per RFC 8707, **never** pass tokens through upstream; confused-deputy consent if the gateway proxies with a static client ID; ideally act as CIMD-capable OAuth client and token-vault.
5. Proxy nested server→client traffic mid-call: sampling (incl. multi-turn tool loops), elicitation (form + URL mode incl. `-32042` retry + completion notifications), roots, progress, cancellation, logging, list_changed — a tools-only proxy is NOT spec-max (LiteLLM's gateway gap is exactly this, GH issue #23761).
6. Tools/resources/prompts/completions passthrough with pagination cursors, structured output, icons, resource subscriptions.
7. Experimental Tasks (poll/deferred results).
8. Origin validation, HTTPS, DNS-rebinding defenses.

**Within ~7 weeks (2026-07-28 final):**
9. Stateless core: `_meta`-carried version/capabilities, no handshake; `InputRequiredResult` multi-step exchanges replacing stream-push.
10. Route/rate-limit on `Mcp-Method` / `Mcp-Name` headers; honor `ttlMs`/`cacheScope` for list/read caching (gateways finally get spec-legal caching).
11. W3C Trace Context propagation in `_meta` (gateway = natural trace hop).
12. Extensions negotiation (reverse-DNS IDs in capability maps, ext-* repos, independent versioning); MCP Apps (sandboxed-iframe UI templates — a gateway should at minimum pass through, at best security-review templates); Tasks as extension (`tasks/get|update|cancel`).
13. Deprecation lifecycle handling: keep Roots/Sampling/Logging working ≥12 months while steering new traffic to replacements; auth hardening (iss validation, AS-bound credentials).
14. Conformance suite (SEP-2484) — run it in CI.

**Enterprise (2026 roadmap, deliberately underdefined — open territory):** audit trails, SSO-integrated auth, "gateway behavior" (authorization propagation, session affinity) is literally a named roadmap workstream, config portability across clients. Most will land as extensions, not core.

---

## 9. Complaints / weaknesses (community signal)

- Stateful Streamable HTTP sessions fight LBs/autoscalers/restarts — the dominant production complaint, severe enough that the protocol core is being rewritten stateless for 2026-07-28.
- OAuth: spec is fine, SDK/reference implementations long assumed server==AS; enterprise IdP (Okta/Entra) integration requires workarounds; token lifecycle with third-party AS is painful.
- "Worst documented technology I have ever encountered" (HN) — docs/spec sprawl across revisions, SEPs, blog posts.
- Sampling/elicitation/roots barely adopted by clients → servers can't rely on them; two of three now deprecated in the RC, validating the skepticism.
- Gateways in the middle break sessions and can't rate-limit per-operation without body inspection (fixed by Mcp-Method/Mcp-Name only in RC).
- Registry: preview-for-9-months, no data durability guarantee, duplicate-version spam, no code security scanning, no self-host support for the official codebase.
- Security ecosystem: 66% of 1,808 scanned MCP servers had findings (AgentSeal); Register headline "design flaw puts 200k servers at risk"; NSA published an MCP security baseline — enterprises treat raw MCP as untrusted without a gateway.
- Breaking-change churn: 5 revisions in ~20 months; 2026-07-28 removes the handshake entirely — every SDK/gateway must dual-stack for a long tail of old servers.

## 10. Agent-experience (AX) notes

- The protocol IS the agent surface: JSON-RPC, machine-readable capability negotiation, `llms.txt` index on modelcontextprotocol.io, JSON Schema everywhere (2020-12 default, full composition in RC).
- Registry is API-first: OpenAPI spec, `mcp-publisher` CLI, GitHub-OIDC CI publishing — built for automation, with subregistries expected to mirror the API shape (a gateway should EXPOSE this same OpenAPI shape for its internal catalog).
- RC's `Mcp-Method`/`Mcp-Name` headers + `ttlMs`/`cacheScope` + Trace Context in `_meta` are explicit machine-control affordances designed FOR gateways/infra, not humans.
- Tool-error-as-result (not protocol error) is a deliberate model-self-correction design; tool annotations (read-only/destructive/idempotent) exist for agent-side policy engines — a gateway can enforce policy on them.
- Direction of travel: human-in-the-loop primitives (sampling approval, form elicitation) are losing to agent-native patterns (direct LLM APIs, URL-mode out-of-band auth, Tasks for long-running work, MCP Apps for UI).

## 11. Strategic implications for a new OSS gateway

1. Build the data plane stateless-first against the 2026-07-28 RC; treat 2025-11-25 session mode as a compatibility shim — incumbents carry stateful architecture debt.
2. The auth story (RFC 9728 PRM serving, CIMD client, token vault for URL-mode third-party OAuth, audience enforcement, anti-passthrough) is the hardest 20% and the moat — most gateways only do API-key injection.
3. Full bidirectional proxying (sampling/elicitation/tasks) is rare in gateways (LiteLLM open issue) — instant differentiation, but weight effort toward Tasks/URL-elicitation since sampling/roots are deprecated.
4. `Mcp-Method`-based routing + `ttlMs` caching + Trace Context = the first spec-legal way to do per-tool rate limits, caching, and tracing — implement day one.
5. Ship a private registry implementing the official OpenAPI interface (the official codebase explicitly refuses self-hosting) + ETL from the official registry with dedup/scan — that's an unserved, spec-aligned niche.
6. Run the official conformance suite (SEP-2484) in CI and advertise spec-max compliance per revision; nobody can credibly claim this yet.

### Key sources
- https://modelcontextprotocol.io/specification/2025-11-25 (+ /changelog, /basic/transports, /basic/authorization, /client/elicitation, /client/sampling)
- https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/
- https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/
- https://modelcontextprotocol.io/registry/about ; https://github.com/modelcontextprotocol/registry
- https://blog.modelcontextprotocol.io/posts/2025-09-08-mcp-registry-preview/
- https://github.com/BerriAI/litellm/issues/23761 (gateway elicitation/sampling gap)
- https://safedep.io/the-state-of-mcp-registries/ ; https://agentseal.org/blog/mcp-server-security-findings ; https://www.theregister.com/2026/04/16/anthropic_mcp_design_flaw/
- https://www.stackone.com/blog/mcp-where-its-been-where-its-going/ ; https://www.speakeasy.com/blog/nsa-mcp-security-baseline
