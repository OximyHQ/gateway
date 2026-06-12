# Oximy Gateway — Manual Test Suite

A copy-paste runbook to verify everything on the review checklist. Each test says
**what it proves**, the **command**, the **expected result**, and **where to see it
in the dashboard**.

> **How testing works here.** `oximy-gateway` the binary is only a launcher
> (`up` / `version` / `keys`). Every *feature* is exercised through the
> **OpenAI-compatible HTTP API** (`/v1/*`), the **MCP endpoint** (`/mcp`), and the
> **embedded dashboard** at `/` (a thin client over that same API). Any agent / SDK
> that speaks OpenAI or MCP can drive it — that is the "agent-first" design.

> **Build note.** Tool calls, multimodal, native Gemini, per-tool ACLs, and policy
> durability are fixed on branch `fix/dataplane-tools-gemini-acl`. Build that branch:
> `cargo build --release --bin oximy-gateway`. Items that need the branch are tagged
> **[branch]**; everything else works on stock `v0.1.0` too.

---

## 0. Setup

```bash
cd /Users/harsh/Downloads/Personal_Projects/OXIMY/primary/gateway

# Provider keys (already in .env.local for this checkout). At least one is required.
set -a; . ./.env.local; set +a            # exports OPENROUTER_API_KEY, GEMINI_API_KEY

# Register an upstream MCP server at boot (needed for §10–11). stdio example:
export OXIMY_MCP_SERVERS='[{"name":"everything","command":"npx","args":["-y","@modelcontextprotocol/server-everything"]}]'

# Boot (headless, local data dir). First boot prints a one-time admin key (ogw_…).
./target/release/oximy-gateway up --no-open --dir ./.oximy-data
```

Copy the printed admin key, then in a **second terminal**:

```bash
export BASE=http://127.0.0.1:8080
export OXIMY_KEY=ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx   # <-- paste your admin key
```

Quick liveness check:

```bash
curl -s $BASE/health                                   # {"status":"ok","version":"0.1.0"}
curl -s $BASE/v1/models -H "Authorization: Bearer $OXIMY_KEY" | jq '.data|length'   # 5484
curl -s -o /dev/null -w '%{http_code}\n' $BASE/v1/models          # 401 (auth required)
```

Open the dashboard at **http://127.0.0.1:8080/** and log in with the admin key.

---

## 1. Dashboard review + agent-first (API parity)

**Proves:** every dashboard surface has an equivalent API call (agents can do
anything the UI can). Click each surface, then run its API twin:

| Dashboard surface | API equivalent |
|---|---|
| Overview | `curl -s $BASE/v1/admin/overview -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| Usage | `curl -s "$BASE/v1/usage?group_by=model" -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| Keys | `curl -s $BASE/v1/admin/keys -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| Logs | `curl -s "$BASE/v1/admin/logs?limit=20" -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| Models | `curl -s $BASE/v1/models -H "Authorization: Bearer $OXIMY_KEY" \| jq '.data[0]'` |
| Providers | `curl -s $BASE/v1/admin/providers -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| MCP Servers | `curl -s $BASE/v1/admin/mcp -H "Authorization: Bearer $OXIMY_KEY" \| jq` |
| Playground | `POST /v1/chat/completions` (see §3) |

**Agent-first CLI** (offline key management against the same data dir):

```bash
./target/release/oximy-gateway version
./target/release/oximy-gateway keys --dir ./.oximy-data list
./target/release/oximy-gateway keys --dir ./.oximy-data create --name cli-demo --budget-usd 5 --models openai/gpt-4o-mini
```

> ⚠️ CLI key ops are **offline** — they write the state file; the running server picks
> them up on its next restart. For live key changes use the admin API (§7). The CLI in
> v0.1.0 is only `up` / `version` / `keys {create,list,revoke}` (no `mcp` / `config`).

> **Agent-over-MCP caveat:** the gateway exposing *its own admin* as an MCP server
> (so an agent installs servers / mints keys over MCP) is **planned (P3)**, not in
> v0.1.0. Today "agent-first" = the OpenAI API + the MCP *federation* endpoint (§10).

---

## 2. Adding a provider (without code)

**Proves:** how to add a provider beyond a bare `export`, and documents the current gap.

Providers auto-register from env keys. To add an **OpenAI-compatible** provider with
no code, point the OpenAI transport at any compatible endpoint via `OPENAI_BASE_URL`
(Ollama, vLLM, Azure OpenAI, LiteLLM, another gateway), then reboot:

```bash
# Example: a local Ollama (or any OpenAI-compatible server)
export OPENAI_BASE_URL=http://localhost:11434/v1
export OPENAI_API_KEY=ollama          # value ignored by Ollama, but required
# re-run `oximy-gateway up --dir ./.oximy-data`, then:
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"llama3.2","messages":[{"role":"user","content":"hi"}]}'
```

Confirm it registered:

```bash
curl -s $BASE/v1/admin/providers -H "Authorization: Bearer $OXIMY_KEY" | jq
```

> ⚠️ **Known gap (matches your "pending" note):** there is **no dashboard "Add
> provider" button and no `POST /v1/admin/providers`** in v0.1.0 — the Providers
> screen and endpoint are **read-only**. Non-env provider registration via the
> config file's `providers` block is also **not honored** (only `routes` +
> `model_overrides` are). So "add a provider from the UI" is a missing feature.

---

## 3. Make an LLM call

**Proves:** the core proxy + exact cost tracking.

```bash
curl -s -D /tmp/h $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Say PONG"}],"max_tokens":10}' \
  | jq '{content:.choices[0].message.content, cost:.usage.cost}'
grep -i '^x-served-by\|^x-cache' /tmp/h
```

**Expected:** `content:"PONG"`, a non-zero `usage.cost`, header `x-served-by: openrouter/openai/gpt-4o-mini`, `x-cache: MISS`.
**Dashboard:** Logs (new row), Overview (spend ticks up), Playground (try it in-browser).

**Streaming:**

```bash
curl -sN $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Count 1 to 5"}],"stream":true,"max_tokens":30}' \
  | grep '^data:' | head
```

**Expected:** a series of `data: {...delta...}` frames ending with `data: [DONE]`.

---

## 4. Make a tool call  **[branch]**

**Proves:** function-calling translation (request tools → provider → tool_calls back).

```bash
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' -d '{
  "model":"openai/gpt-4o-mini",
  "messages":[{"role":"user","content":"Weather in Tokyo?"}],
  "tools":[{"type":"function","function":{"name":"get_weather","description":"Get current weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],
  "tool_choice":"required","max_tokens":80
}' | jq '{finish:.choices[0].finish_reason, tool_calls:.choices[0].message.tool_calls}'
```

**Expected:** `finish:"tool_calls"` with a `get_weather` call and `arguments:"{\"city\":\"Tokyo\"}"`.
Repeat with `"model":"anthropic/claude-3.5-haiku"` to confirm cross-dialect tool calls.
**On stock v0.1.0 this returns plain text with `tool_calls:null` (the bug this branch fixes).**

---

## 5. Make a multimodal LLM call  **[branch]**

**Proves:** image input survives the gateway.

```bash
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' -d '{
  "model":"openai/gpt-4o-mini",
  "messages":[{"role":"user","content":[
    {"type":"text","text":"What animal is in this image? One word."},
    {"type":"image_url","image_url":{"url":"https://upload.wikimedia.org/wikipedia/commons/thumb/3/3a/Cat03.jpg/120px-Cat03.jpg"}}
  ]}],"max_tokens":15
}' | jq '{content:.choices[0].message.content, error:.error}'
```

**Expected:** `content:"Cat."` (or similar). **On stock v0.1.0:** `400 "invalid type: sequence, expected a string"`.

---

## 6. The same, across several providers

**Proves:** one key + one API reaches many providers. OpenRouter alone fans out to
OpenAI / Anthropic / DeepSeek / Google; native Gemini is a separate egress.

```bash
for M in "openai/gpt-4o-mini" "anthropic/claude-3.5-haiku" "deepseek/deepseek-chat" "google/gemini-2.5-flash"; do
  printf '%-32s ' "$M"
  curl -s -D /tmp/h $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
    -H 'Content-Type: application/json' \
    -d "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say OK\"}],\"max_tokens\":10}" \
    | jq -rc '.choices[0].message.content // .error.message'
  grep -i '^x-served-by' /tmp/h | tr -d '\r'
done
```

**Native Gemini [branch]** (note the `served-by: google/...`, and use a big `max_tokens`
because Gemini 2.5 flash is a *thinking* model that spends tokens before replying):

```bash
curl -s -D /tmp/h $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"google/gemini-flash-latest","messages":[{"role":"user","content":"Capital of Japan? One word."}],"max_tokens":1000}' \
  | jq -r '.choices[0].message.content'
grep -i '^x-served-by' /tmp/h     # x-served-by: google/google/gemini-flash-latest
```

---

## 7. Virtual keys + budget enforcement

**Proves:** scoped keys with a hard USD budget, enforced fail-closed.

```bash
# Mint a key with a tiny budget (admin API; live immediately)
TINY=$(curl -s $BASE/v1/admin/keys -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' -d '{"name":"tiny","budget_usd":0.00002}' | jq -r '.secret')

# Spend it with UNIQUE prompts (identical prompts hit the cache = $0 and never exhaust)
for i in 1 2 3 4 5; do
  curl -s -o /dev/null -w "call $i -> %{http_code}\n" $BASE/v1/chat/completions \
    -H "Authorization: Bearer $TINY" -H 'Content-Type: application/json' \
    -d "{\"model\":\"openai/gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"nonce $i $RANDOM\"}],\"max_tokens\":10}"
done
```

**Expected:** first call(s) `200`, then **`429`** once the budget is spent (fail-closed).
**Dashboard:** Keys → the `tiny` row shows a near-full budget bar.

> The budget is **durable**: stop the server, restart it, and the exhausted key still
> returns `429` (spend lives in SQLite, not memory).

---

## 8. Rate limits + model allowlist

**Proves:** per-key model restriction (and rpm/tpm, via the admin API).

```bash
ALLOW=$(curl -s $BASE/v1/admin/keys -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"name":"only-mini","budget_usd":5,"models":["openai/gpt-4o-mini"],"rpm":60,"tpm":100000}' | jq -r '.secret')

# Allowed model -> 200
curl -s -o /dev/null -w "allowed: %{http_code}\n" $BASE/v1/chat/completions \
  -H "Authorization: Bearer $ALLOW" -H 'Content-Type: application/json' \
  -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"hi"}],"max_tokens":5}'

# Disallowed model -> 403
curl -s -w "\ndisallowed: %{http_code}\n" $BASE/v1/chat/completions \
  -H "Authorization: Bearer $ALLOW" -H 'Content-Type: application/json' \
  -d '{"model":"anthropic/claude-3.5-haiku","messages":[{"role":"user","content":"hi"}],"max_tokens":5}'
```

**Expected:** allowed `200`; disallowed `403 "model … is not allowed for this key"`.
**Dashboard:** Keys → MODELS column shows `openai/gpt-4o-mini` **[branch]** (stock v0.1.0 shows "all").

---

## 9. Guardrails (basic)

**Proves:** content policy runs before the provider. The **secrets** scanner is on by
default in Enforce mode; **PII** is on in Observe-only (logs, does not block).

```bash
# Secret in the prompt -> blocked before any provider call
curl -s -w "\n%{http_code}\n" $BASE/v1/chat/completions -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"store this key sk-abcdefghijklmnopqrstuvwxyz0123456789"}],"max_tokens":10}'
```

**Expected:** `403 "blocked by guardrail: detected OpenAI API key in content"`.
Try also `ghp_…` (GitHub), `AKIA…` (AWS), `xoxb-…` (Slack).
**Dashboard:** Guardrails (config), Logs (the blocked attempt).

> ⚠️ **Limit:** in v0.1.0 only the two built-in defaults are active. Adding keyword /
> regex / custom guardrails via the config file is **not yet honored** (the config
> `guardrails` block is ignored). So "guardrails work" = the built-in secrets/PII pair.

---

## 10. MCP server: add an upstream, list, call, see usage

**Proves:** the MCP federation half. **How to register an upstream (your open question):**
the only way in v0.1.0 is the **`OXIMY_MCP_SERVERS` env var at boot** (set in §0).
`{name,url}` for an HTTP server, `{name,command,args}` for a stdio one. (CLI `mcp add`,
config-file `mcp_servers`, and a dashboard "Add server" are documented but **not
implemented** in v0.1.0.)

```bash
# Initialize + list federated tools (namespaced server__tool)
curl -s $BASE/mcp -H "Authorization: Bearer $OXIMY_KEY" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | jq -c '.result.serverInfo'

curl -s $BASE/mcp -H "Authorization: Bearer $OXIMY_KEY" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq -r '.result.tools[].name'

# Call a tool
curl -s $BASE/mcp -H "Authorization: Bearer $OXIMY_KEY" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"everything__echo","arguments":{"message":"hello via oximy"}}}' | jq -c '.result.content'

# Auth gate
curl -s -o /dev/null -w "no-auth -> %{http_code}\n" $BASE/mcp -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'

# Federation status (the MCP dashboard surface)
curl -s $BASE/v1/admin/mcp -H "Authorization: Bearer $OXIMY_KEY" | jq -c '.servers[] | {name, healthy, tools:(.tools|length)}'
```

**Expected:** `serverInfo` for `oximy-gateway`; 12 `everything__*` tools; echo returns
`"Echo: hello via oximy"`; no-auth → `401`; the server shows `healthy:true`.
**Dashboard:** MCP Servers (the server + its tools).

**To use a real Linear MCP instead**, reboot with:
```bash
export OXIMY_MCP_SERVERS='[{"name":"linear","url":"https://mcp.linear.app/mcp"}]'
```
(then its tools appear as `linear__…`).

---

## 11. Per-tool controls — "NO DELETE on Linear"  **[branch]**

**Proves:** a virtual key can be restricted to specific tools; everything else is hidden
and blocked. **The model is an allow-list** (not a deny toggle): you list the tools the
key MAY call, and the dangerous one is simply excluded.

```bash
# Mint a key allowed ONLY the safe tools (echo + sum), i.e. NOT the "dangerous" one.
# For real Linear: tool_allowlist = every linear__ tool EXCEPT linear__delete_issue.
RO=$(curl -s $BASE/v1/admin/keys -H "Authorization: Bearer $OXIMY_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"name":"linear-readonly","budget_usd":1,"tool_allowlist":["everything__echo","everything__get-sum"]}' | jq -r '.secret')

# This key sees ONLY its 2 tools (not all 12)
curl -s $BASE/mcp -H "Authorization: Bearer $RO" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq -c '.result.tools|map(.name)'

# Allowed tool -> works
curl -s $BASE/mcp -H "Authorization: Bearer $RO" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"everything__echo","arguments":{"message":"ok"}}}' | jq -c '.result.content'

# Excluded tool (stand-in for linear__delete_issue) -> BLOCKED
curl -s $BASE/mcp -H "Authorization: Bearer $RO" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"everything__get-env","arguments":{}}}' | jq -c '.error'
```

**Expected:** list shows only `["everything__get-sum","everything__echo"]`; echo works;
the excluded tool returns `{"code":-32000,"message":"tool not allowed: everything__get-env"}`.
The admin key (no allowlist) still sees all 12 tools.

> **Real Linear mapping:** register the Linear MCP (§10), then create the agent's key
> with `tool_allowlist` = all `linear__*` tools **except** `linear__delete_issue`. Point
> the agent at `http://127.0.0.1:8080/mcp` with that key — delete is now impossible.

---

## 12. Policy durability across restart  **[branch]**

**Proves:** allowlists / rate-limits / tool-ACLs are not silently dropped on a bounce.

```bash
# (Using the keys from §8/§11.) Stop the server (Ctrl-C in terminal 1), then re-run:
#   oximy-gateway up --no-open --dir ./.oximy-data   (keep OXIMY_MCP_SERVERS set)

# Same restricted key must STILL enforce after the restart:
curl -s -o /dev/null -w "disallowed model -> %{http_code} (want 403)\n" $BASE/v1/chat/completions \
  -H "Authorization: Bearer $ALLOW" -H 'Content-Type: application/json' \
  -d '{"model":"anthropic/claude-3.5-haiku","messages":[{"role":"user","content":"hi"}],"max_tokens":5}'

curl -s $BASE/mcp -H "Authorization: Bearer $RO" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"everything__get-env","arguments":{}}}' | jq -c '.error'

# And the state file now persists the full policy:
jq '.keys | to_entries[].value | {id, model_allowlist, tool_allowlist, rpm, tpm}' ./.oximy-data/gateway.json
```

**Expected:** disallowed model still `403`; excluded tool still blocked; the JSON shows
`model_allowlist` / `tool_allowlist` / `rpm` / `tpm` populated.
**On stock v0.1.0** the disallowed model would return `200` after restart (the bug).

---

## 13. Cleanup

```bash
# List + revoke test keys
curl -s $BASE/v1/admin/keys -H "Authorization: Bearer $OXIMY_KEY" | jq -r '.keys[]?.id // .[]?.id'
curl -s -X POST $BASE/v1/admin/keys/<ID>/revoke -H "Authorization: Bearer $OXIMY_KEY" | jq

# Full reset (wipes all keys/state/spend; next boot prints a new admin key):
#   stop the server, then:  rm -rf ./.oximy-data
```

---

## Appendix — stock v0.1.0 vs this branch

| Feature | Stock v0.1.0 | `fix/dataplane-tools-gemini-acl` |
|---|---|---|
| LLM call, streaming, cost, cache | ✅ | ✅ |
| Budget enforcement (durable) | ✅ | ✅ |
| Guardrails (secrets/PII) | ✅ | ✅ |
| MCP federation + call + audit | ✅ | ✅ |
| Tool / function calling | ❌ dropped | ✅ |
| Multimodal (image input) | ❌ 400 | ✅ |
| Native Gemini (`google/*`) | ❌ 400 | ✅ |
| Per-tool ACL set + enforced | ❌ unwired | ✅ |
| Allowlist / rate-limit / ACL survive restart | ❌ reset to open | ✅ |
| Add provider via UI/API; config keys/guardrails/mcp; CLI `mcp`/`config`; agent-over-MCP admin | ❌ not implemented (env/API only) | ❌ (out of scope) |
