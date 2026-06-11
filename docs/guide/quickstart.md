# Quickstart

This guide gets you from zero to a working gateway in about five minutes.

---

## 1. Install

### macOS (Homebrew)

```bash
brew install oximyhq/tap/oximy-gateway
```

### Linux / macOS (installer script)

```bash
curl -fsSL https://raw.githubusercontent.com/OximyHQ/gateway/main/install.sh | sh
```

This installs the `oximy-gateway` binary to `~/.local/bin` (or `/usr/local/bin`
on macOS). Add it to your `PATH` if it is not already there.

### From source (requires Rust toolchain)

```bash
cargo install --git https://github.com/OximyHQ/gateway oximy-gateway
```

### Docker

```bash
docker pull ghcr.io/oximyhq/gateway:latest
```

See [Deployment](./deployment.md) for the full Docker run command.

### Verify

```bash
oximy-gateway version
# oximy-gateway 0.x.y
```

---

## 2. Set provider keys

The gateway reads provider keys from environment variables. Set at least one:

```bash
export OPENAI_API_KEY=sk-...
```

You can add more at any time without restarting (set the env var and restart the
process, or use the dashboard to hot-reload config):

```bash
export ANTHROPIC_API_KEY=sk-ant-...
export GEMINI_API_KEY=AIza...
export OPENROUTER_API_KEY=sk-or-...
```

The gateway starts fine with no keys set, but chat requests will fail until at
least one provider is configured.

---

## 3. Boot the gateway

```bash
oximy-gateway up
```

On **first boot**, the gateway creates a data directory, generates an admin key,
prints it once, and opens the dashboard in your browser:

```
  ┌─ First boot ──────────────────────────────────────────────────────────
  │  A default admin key was created. It is shown ONCE:
  │
  │     ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  │
  │  Use it as your Bearer token for the API and dashboard.
  │  Store it now — it cannot be recovered.
  └───────────────────────────────────────────────────────────────────────

  Oximy Gateway is running.

  Dashboard:  http://127.0.0.1:8080/
  API base:   http://127.0.0.1:8080/v1
  Health:     http://127.0.0.1:8080/health
  Models:     http://127.0.0.1:8080/v1/models (auth required)
```

**Save the admin key now.** The secret is stored as a hash in the state file and
cannot be recovered. If you lose it, rotate it via the dashboard or delete the
state file and restart.

On subsequent boots, the gateway loads the existing state and starts normally
(no key is printed).

### Options

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--port` | `OXIMY_GATEWAY_PORT` | `8080` | Port to bind |
| `--host` | `OXIMY_GATEWAY_HOST` | `127.0.0.1` | Interface to bind |
| `--dir` | `OXIMY_GATEWAY_DIR` | platform default | Data directory |
| `--no-open` | — | false | Don't open the dashboard in a browser |

Example — bind to all interfaces on port 9000, no browser:

```bash
oximy-gateway up --host 0.0.0.0 --port 9000 --no-open
```

---

## 4. Make your first request

Any OpenAI-compatible client works. Just point `base_url` at the gateway and use
your admin key as the bearer token.

### curl

```bash
export OXIMY_KEY=ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "What is 2 + 2?"}]
  }'
```

The response is standard OpenAI format plus an extra `usage.cost` field (exact USD)
and a `x-overhead-duration-ms` header:

```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "model": "gpt-4o",
  "choices": [
    {
      "message": { "role": "assistant", "content": "4" },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 24,
    "completion_tokens": 1,
    "total_tokens": 25,
    "cost": 0.000071
  }
}
```

### Python (OpenAI SDK)

```python
from openai import OpenAI

client = OpenAI(
    api_key="ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    base_url="http://127.0.0.1:8080/v1",
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "What is 2 + 2?"}],
)
print(response.choices[0].message.content)
# "4"
```

### Node (OpenAI SDK)

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
  baseURL: "http://127.0.0.1:8080/v1",
});

const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages: [{ role: "user", content: "What is 2 + 2?" }],
});
console.log(response.choices[0].message.content);
```

### Anthropic dialect (Claude clients)

Clients that send `POST /v1/messages` (the Anthropic request format) also work:

```bash
curl http://127.0.0.1:8080/v1/messages \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-3-5-sonnet-20241022",
    "max_tokens": 100,
    "messages": [{"role": "user", "content": "Hi"}]
  }'
```

### Streaming

Add `"stream": true` to any chat request:

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Count to 5"}],"stream":true}'
```

You receive SSE chunks (`text/event-stream`) followed by `data: [DONE]`.

---

## 5. Dashboard tour

The browser opens at `http://127.0.0.1:8080/` automatically. Log in with your
admin key.

The dashboard is a thin client of the REST API — everything visible here is also
accessible programmatically.

**What you will find:**

- **Overview** — total requests, spend by model, recent errors at a glance
- **Requests log** — per-request view: model, latency, token counts, cost, key,
  cache status
- **Keys** — list, create, and revoke virtual keys; set budgets and model allowlists
- **Providers** — which providers are active (derived from which env keys are set)
- **Models** — the registry: model IDs, owning provider, context window, pricing
- **MCP** — registered upstream MCP servers, tool list, ACL configuration
- **Metrics** — link to the raw Prometheus endpoint

---

## 6. Mint a key for a teammate

Rather than sharing your admin key, create a scoped virtual key:

```bash
# $10 budget, only allowed to call gpt-4o and gpt-4o-mini
oximy-gateway keys create \
  --name "alice-dev" \
  --budget 10.00 \
  --models gpt-4o,gpt-4o-mini
```

Output:

```
Key created.
  ID:     key_abc123
  Name:   alice-dev
  Secret: ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx  (shown once)
  Budget: $10.00
  Models: gpt-4o, gpt-4o-mini
```

Alice uses that key as her bearer token. When she hits $10 in spend the gateway
returns `429` with a budget-exceeded error before forwarding anything to the
provider.

See [Keys & Budgets](./keys-budgets.md) for the full key management guide.

---

## Next steps

- [Configuration](./configuration.md) — configure providers via `oximy-gateway.json`
- [Providers](./providers.md) — full provider list and how to enable each
- [Keys & Budgets](./keys-budgets.md) — scoped keys, rate limits, model allowlists
- [Guardrails](./guardrails.md) — block PII and secrets by default
- [MCP Gateway](./mcp.md) — connect MCP tool servers
- [Observability](./observability.md) — Prometheus metrics and request logging
- [Deployment](./deployment.md) — running in production
