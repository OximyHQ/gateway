# Configuration

Oximy Gateway is configured through **environment variables** (provider keys and
process-level settings) and an optional **`oximy-gateway.json` config file**
(providers, virtual keys, routes, model overrides, guardrail attachments).

The config file and the database are two projections of the same state. You can
manage everything through the dashboard, the CLI, or a checked-in config file —
they all go through the same diff/apply engine.

---

## Environment variables

### Provider API keys

Set these before starting the gateway. The gateway registers providers
automatically based on which keys are present.

| Variable | Provider |
|---|---|
| `OPENAI_API_KEY` | OpenAI |
| `ANTHROPIC_API_KEY` | Anthropic |
| `GEMINI_API_KEY` | Google Gemini |
| `OPENROUTER_API_KEY` | OpenRouter (OpenAI-compatible) |
| `GROQ_API_KEY` | Groq |
| `TOGETHER_API_KEY` | Together AI |
| `FIREWORKS_API_KEY` | Fireworks AI |
| `DEEPSEEK_API_KEY` | DeepSeek |
| `XAI_API_KEY` | xAI (Grok) |
| `MISTRAL_API_KEY` | Mistral AI |
| `PERPLEXITY_API_KEY` | Perplexity |
| `CEREBRAS_API_KEY` | Cerebras |

### OpenAI-compatible base URL override

```bash
# Point the OpenAI transport at any OpenAI-compatible endpoint
export OPENAI_BASE_URL=https://my-proxy.example.com/v1
export OPENAI_API_KEY=my-key
```

This lets you use Ollama, vLLM, SGLang, TGI, Azure OpenAI, or any
OpenAI-compatible self-hosted model via the `openai` provider slot.

### Process settings

| Variable | CLI flag equivalent | Default | Description |
|---|---|---|---|
| `OXIMY_GATEWAY_PORT` | `--port` | `8080` | Port to bind |
| `OXIMY_GATEWAY_HOST` | `--host` | `127.0.0.1` | Interface to bind |
| `OXIMY_GATEWAY_DIR` | `--dir` | platform default | Data directory |

**Platform defaults for the data directory:**
- Linux: `~/.local/share/oximy-gateway`
- macOS: `~/Library/Application Support/com.oximy.oximy-gateway`
- Windows: `%APPDATA%\oximy\oximy-gateway`

The state file (`gateway.json`) lives inside the data directory and stores hashed
keys and configuration that persists across restarts.

### Route overrides

```bash
# JSON map of model name → Route object
export OXIMY_ROUTES='{"gpt-4o":{"targets":[{"provider_id":"openai","model":"gpt-4o"},{"provider_id":"openrouter","model":"openai/gpt-4o"}],"strategy":"failover"}}'
```

Route targets are tried in order. `"failover"` tries the next target on error.
See [Providers](./providers.md) for available `provider_id` values.

### MCP server registration

```bash
# JSON array of upstream MCP servers to federate at startup
export OXIMY_MCP_SERVERS='[
  {"name":"docs","url":"https://mcp.example.com/mcp"},
  {"name":"local","command":"my-mcp-server","args":["--flag"]}
]'
```

Each entry has either a `url` (HTTP streamable MCP endpoint) or a `command`
(stdio process to spawn). A server that fails to connect is logged and skipped —
a bad upstream server never prevents the gateway from booting.

---

## The `oximy-gateway.json` config file

The config file is the declarative form of your entire gateway setup. It is
optional: the gateway runs fine with only env vars. But for reproducible
deployments, checked-in configuration, and GitOps workflows, it is the right tool.

### Loading the config file

```bash
# Apply a config file at startup
oximy-gateway up --config oximy-gateway.json

# Validate without applying
oximy-gateway config validate oximy-gateway.json

# Dry-run: show what would change
oximy-gateway config diff oximy-gateway.json

# Apply changes without restarting
oximy-gateway config apply oximy-gateway.json
```

The gateway watches the file for changes and hot-reloads it. Bad configs are
rejected (the last good config keeps serving).

### Full schema example

```json
{
  "providers": [
    {
      "id": "openai",
      "api_key": "${OPENAI_API_KEY}"
    },
    {
      "id": "anthropic",
      "api_key": "${ANTHROPIC_API_KEY}"
    },
    {
      "id": "gemini",
      "api_key": "${GEMINI_API_KEY}"
    },
    {
      "id": "openrouter",
      "api_key": "${OPENROUTER_API_KEY}",
      "base_url": "https://openrouter.ai/api"
    },
    {
      "id": "local-ollama",
      "api_key": "ollama",
      "base_url": "http://localhost:11434/v1"
    }
  ],
  "keys": [
    {
      "id": "key_alice",
      "name": "alice-dev",
      "budget_usd": 50.00,
      "model_allowlist": ["gpt-4o", "gpt-4o-mini"],
      "rpm": 60,
      "tpm": 100000
    },
    {
      "id": "key_ci",
      "name": "ci-pipeline",
      "budget_usd": 5.00,
      "model_allowlist": ["gpt-4o-mini"]
    }
  ],
  "routes": [
    {
      "model": "gpt-4o",
      "targets": [
        {"provider_id": "openai", "model": "gpt-4o"},
        {"provider_id": "openrouter", "model": "openai/gpt-4o"}
      ],
      "strategy": "failover"
    }
  ],
  "registry_overrides": [
    {
      "model_id": "my-fine-tune",
      "provider": "local-ollama",
      "input_per_mtok_micros": 0,
      "output_per_mtok_micros": 0
    }
  ],
  "guardrails": [
    {
      "id": "default",
      "rules": [
        {"type": "secrets", "mode": "enforce"},
        {"type": "pii", "mode": "observe_only"}
      ]
    }
  ]
}
```

### Env-var interpolation

Any string value in the config file can reference an environment variable:

```json
{ "api_key": "${OPENAI_API_KEY}" }
```

A missing variable is a hard error at load time (fail-closed). This keeps secrets
out of config files while making them explicit.

### `dump` / `diff` / `apply`

These commands let you treat the gateway config like a database migration:

```bash
# Export current live state to a file
oximy-gateway config dump > oximy-gateway.json

# Show what a new file would change (does not apply anything)
oximy-gateway config diff oximy-gateway-new.json

# Apply changes from a file to the running gateway
oximy-gateway config apply oximy-gateway-new.json
```

`diff` output shows ordered typed changes (e.g., `+ key key_alice`, `~ provider openai`).
`apply` is idempotent — re-applying an unchanged config is a no-op.

### Provider key encryption

Provider API keys in the state file are encrypted at rest with XChaCha20-Poly1305
(AEAD). The encryption key is derived from a master key stored separately. For
production deployments, consider injecting the master key via an env var or a
secrets manager rather than storing it alongside the state file.

---

## Configuration precedence

When the same setting appears in multiple places, this order applies
(higher wins):

1. CLI flags (`--port`, `--host`, `--dir`)
2. Environment variables (`OXIMY_GATEWAY_PORT`, etc.)
3. `oximy-gateway.json` config file
4. Built-in defaults

---

## Logging

The gateway uses structured logging via `tracing`. Set the `RUST_LOG` environment
variable to control verbosity:

```bash
RUST_LOG=info oximy-gateway up          # default
RUST_LOG=debug oximy-gateway up         # verbose
RUST_LOG=oximy_gateway=debug oximy-gateway up  # debug only the gateway binary
```
