# MCP Gateway

The MCP (Model Context Protocol) plane is co-equal to the LLM plane in Oximy
Gateway. Both flow through the same governance spine — one virtual key's budget,
one audit log, one guardrail policy — across both LLM tokens and MCP tool calls.

---

## What it is

`POST /mcp` is an authenticated JSON-RPC 2.0 endpoint that federates N upstream
MCP servers behind a single address. Clients (Claude Code, Cursor, VS Code with
Copilot, Codex CLI, Windsurf, etc.) connect to the gateway once. The gateway
handles:

- **Authentication** — same bearer token as `/v1/*`; tool calls are tied to a
  virtual key with its own budget and ACL
- **Federation** — tools from all registered upstream servers are presented as a
  single unified list
- **Namespacing** — tool names are prefixed with the server name: a tool called
  `search` on a server named `docs` appears as `docs__search`
- **ACLs** — per-key tool allowlists restrict which tools a key can call
- **Audit** — every tool call is recorded on the shared spine audit log
- **Rug-pull detection** — tool description hashes are tracked; a silent change in
  an upstream tool definition triggers an alert

---

## Protocol version

The gateway speaks **MCP 2025-11-25** (current stable). It performs the
`initialize` handshake with upstream servers when they are registered. Clients
that send `notifications/*` frames (fire-and-forget, no response expected) receive
HTTP 202.

---

## Connecting a client

Configure your MCP client to use `http://127.0.0.1:8080/mcp` with a bearer token.

### Claude Code (`~/.claude.json`)

```json
{
  "mcpServers": {
    "oximy": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp",
      "headers": {
        "Authorization": "Bearer ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
      }
    }
  }
}
```

### Cursor (`.cursor/mcp.json` in your project)

```json
{
  "mcpServers": {
    "oximy": {
      "url": "http://127.0.0.1:8080/mcp",
      "headers": {
        "Authorization": "Bearer ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
      }
    }
  }
}
```

### VS Code (settings.json)

```json
{
  "mcp.servers": {
    "oximy": {
      "url": "http://127.0.0.1:8080/mcp",
      "headers": {
        "Authorization": "Bearer ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
      }
    }
  }
}
```

---

## Registering upstream MCP servers

### At startup via `OXIMY_MCP_SERVERS`

```bash
export OXIMY_MCP_SERVERS='[
  {
    "name": "docs",
    "url": "https://mcp.example.com/mcp"
  },
  {
    "name": "filesystem",
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
  }
]'
oximy-gateway up
```

Each entry has either:
- `url` — an HTTP streamable MCP endpoint
- `command` + `args` — a stdio process to spawn

A server that fails to connect at startup is logged and skipped. The gateway
boots regardless.

### Via the dashboard

Go to **MCP** in the dashboard sidebar, click **Add server**, and fill in the
name and URL (or command). The server is registered and its tools are listed
immediately.

### Via CLI (planned, P3)

```bash
oximy-gateway mcp add --name docs --url https://mcp.example.com/mcp
oximy-gateway mcp add --name filesystem --command npx -- -y @modelcontextprotocol/server-filesystem /tmp
oximy-gateway mcp list
oximy-gateway mcp remove docs
```

### Via `oximy-gateway.json`

```json
{
  "mcp_servers": [
    {
      "name": "docs",
      "url": "https://mcp.example.com/mcp"
    },
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  ]
}
```

---

## Making MCP requests

The `POST /mcp` endpoint accepts standard JSON-RPC 2.0 frames.

### List available tools

```bash
curl http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "docs__search",
        "description": "Search documentation",
        "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }
      },
      {
        "name": "filesystem__read_file",
        "description": "Read a file from the filesystem",
        "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
      }
    ]
  }
}
```

Tools are returned in `server__tool` namespaced form. Only tools the bearer key
is allowed to call are listed.

### Call a tool

```bash
curl http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
      "name": "docs__search",
      "arguments": { "query": "authentication" }
    }
  }'
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [
      { "type": "text", "text": "Authentication docs: ..." }
    ]
  }
}
```

### Initialize handshake

Some clients send an `initialize` frame before other calls. The gateway handles
this:

```bash
curl http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 0,
    "method": "initialize",
    "params": {
      "protocolVersion": "2025-11-25",
      "capabilities": {},
      "clientInfo": { "name": "my-client", "version": "1.0.0" }
    }
  }'
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": "2025-11-25",
    "capabilities": { "tools": {} },
    "serverInfo": { "name": "oximy-gateway", "version": "0.x.y" }
  }
}
```

---

## Tool ACLs

Restrict which tools a virtual key can call. If a key has a tool allowlist, any
tool not on the list is hidden from `tools/list` and rejected by `tools/call`.

### Via dashboard

**MCP** tab → select a key → edit its tool allowlist.

### Via `oximy-gateway.json`

```json
{
  "keys": [
    {
      "id": "key_readonly",
      "name": "readonly-agent",
      "budget_usd": 5.00,
      "tool_allowlist": ["docs__search", "filesystem__read_file"]
    }
  ]
}
```

### Via CLI (planned, P3)

```bash
oximy-gateway keys update key_readonly \
  --tool-allowlist "docs__search,filesystem__read_file"
```

### Effect on tool listing

A key with `tool_allowlist = ["docs__search"]` calling `tools/list` sees:

```json
{
  "result": {
    "tools": [
      { "name": "docs__search", ... }
    ]
  }
}
```

`filesystem__read_file` is not returned, even if it exists on the federation.
`tools/call` for a tool not on the allowlist returns:

```json
{
  "error": {
    "code": -32603,
    "message": "tool not permitted for this key: filesystem__read_file"
  }
}
```

---

## Audit log

Every tool call — including rejected ones — is recorded with:

- Timestamp
- Key ID (the `actor`)
- Tool name (the `target`)
- Arguments (sanitized; secrets are redacted if the secrets guardrail is active)
- Outcome (`ok` or error message)

View tool-call audit events in the dashboard under **Requests > MCP calls**.

---

## Rug-pull detection

When the gateway registers an upstream MCP server, it computes a SHA-256 hash of
each tool's description and input schema. On subsequent refreshes, if a tool
description changes silently, the gateway logs a warning:

```
WARN tool description hash changed: docs__search (was a1b2c3..., now d4e5f6...)
```

This alerts you to upstream server changes that could affect how an agent
interprets and calls the tool — a class of supply-chain attack sometimes called
a "rug pull."

---

## What is coming (later phases)

- **Dollar metering** (P2) — MCP tool calls counted against the same USD budget as
  LLM tokens; `tools/call` deducted from the key's remaining budget
- **OAuth 2.1 brokering** (P2) — inbound OAuth resource server, outbound
  credential injection so secrets never reach the client
- **Admin-MCP server** (P3) — the gateway exposes its own admin API as an MCP
  server so agents can install servers, mint keys, and query telemetry over MCP
- **Semantic tool discovery** (P4) — `find_tool` / `call_tool` for on-demand
  discovery to manage context window pressure
