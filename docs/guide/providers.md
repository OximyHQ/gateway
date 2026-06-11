# Providers

Oximy Gateway separates **providers** (API wire formats, ~30 implementations) from
**models** (a hot-reloading registry with 1000+ entries). Adding a new model is
data; adding a new provider is code — but you probably do not need to add a
provider because almost every model is reachable through an existing one.

---

## How provider registration works

At startup the gateway reads environment variables. For each key that is set and
non-empty, it registers the corresponding provider. Providers not registered simply
do not appear in `/v1/models` and cannot be used for routing.

---

## Native providers

These have a typed egress transport with full dialect support (tool calls, vision,
streaming, prompt-cache accounting).

### OpenAI

```bash
export OPENAI_API_KEY=sk-...
```

Models registered by default: `gpt-4o`, `gpt-4o-mini`. The registry is extended
with a hot-reloading model list — any model available on OpenAI's API is
addressable once its pricing entry exists.

**Base URL override** — useful for Azure OpenAI, self-hosted models, or any
OpenAI-compatible proxy:

```bash
export OPENAI_BASE_URL=https://my-azure-endpoint.openai.azure.com/openai
export OPENAI_API_KEY=my-azure-key
```

The override replaces the entire base URL, including the path prefix. Set it to
`http://localhost:11434/v1` to point at a local Ollama instance.

### Anthropic

```bash
export ANTHROPIC_API_KEY=sk-ant-...
```

Models registered by default: `claude-3-5-sonnet-20241022`.

The Anthropic transport speaks the `/v1/messages` wire format natively and
forwards `anthropic-version` and `anthropic-beta` headers unchanged. This means
Claude Code and other Anthropic-native clients work without any translation.

Prompt-cache accounting is fully supported: `cache_read_input_tokens` and
`cache_creation_input_tokens` are tracked separately and priced correctly.

### Gemini

```bash
export GEMINI_API_KEY=AIza...
```

Models registered by default: `gemini-1.5-pro`.

---

## OpenAI-compatible providers

These providers expose an OpenAI-compatible API. The gateway reuses the OpenAI
egress transport and points it at the provider's base URL.

### OpenRouter

```bash
export OPENROUTER_API_KEY=sk-or-...
```

OpenRouter is an OpenAI-compatible aggregator. Through it you can reach
Anthropic, DeepSeek, Meta Llama, Mistral, and dozens of other models with a
single key.

Models registered by default via OpenRouter:
- `openai/gpt-4o-mini` — $0.15/$0.60 per MTok
- `openai/gpt-4o` — $2.50/$10.00 per MTok
- `anthropic/claude-3.5-haiku` — $0.80/$4.00 per MTok
- `deepseek/deepseek-chat` — $0.28/$0.88 per MTok
- `meta-llama/llama-3.3-70b-instruct` — $0.12/$0.30 per MTok

Any model available on OpenRouter is addressable by its full slug
(e.g., `google/gemini-pro-1.5`) once the gateway has a pricing entry for it.

### Groq

```bash
export GROQ_API_KEY=gsk_...
```

Groq serves open-weight models (Llama, Mixtral, Gemma) via a fast inference API
with an OpenAI-compatible surface.

### Together AI

```bash
export TOGETHER_API_KEY=...
```

### Fireworks AI

```bash
export FIREWORKS_API_KEY=...
```

### DeepSeek

```bash
export DEEPSEEK_API_KEY=sk-...
```

### xAI (Grok)

```bash
export XAI_API_KEY=...
```

### Mistral AI

```bash
export MISTRAL_API_KEY=...
```

### Perplexity

```bash
export PERPLEXITY_API_KEY=pplx-...
```

### Cerebras

```bash
export CEREBRAS_API_KEY=...
```

---

## Self-hosted models (Ollama, vLLM, SGLang, TGI)

Any OpenAI-compatible local server works via `OPENAI_BASE_URL`:

```bash
# Ollama
export OPENAI_BASE_URL=http://localhost:11434/v1
export OPENAI_API_KEY=ollama   # Ollama ignores the key but the SDK requires one

# vLLM
export OPENAI_BASE_URL=http://localhost:8000/v1
export OPENAI_API_KEY=vllm-key
```

Request your local model by its name:

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2","messages":[{"role":"user","content":"Hello"}]}'
```

---

## The model registry

Every model is a row in an in-memory registry. The registry holds:

- **Model ID** — the string clients pass in `"model"`
- **Provider** — which egress transport to use
- **Pricing** — input/output/cache-read/cache-write per million tokens (in
  microdollars, to avoid floating-point drift)
- **Context window** — maximum input tokens
- **Capabilities** — tools, vision, streaming

The registry is loaded at startup and hot-reloads from an external model list
(based on `models.dev` data + local overrides) when the source file changes.

### Checking available models

```bash
curl http://127.0.0.1:8080/v1/models \
  -H "Authorization: Bearer $OXIMY_KEY"
```

Response:

```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-4o",
      "object": "model",
      "owned_by": "openai",
      "context_window": 128000,
      "pricing": {
        "input_per_mtok_micros": 2500000,
        "output_per_mtok_micros": 10000000
      }
    }
  ]
}
```

`input_per_mtok_micros` is microdollars per million tokens. Divide by 1,000,000 to
get dollars per million tokens (e.g., 2,500,000 µ$ = $2.50).

### Adding a model via config

```json
{
  "registry_overrides": [
    {
      "model_id": "my-fine-tune",
      "provider": "local-ollama",
      "input_per_mtok_micros": 0,
      "output_per_mtok_micros": 0,
      "context_window": 8192
    }
  ]
}
```

---

## How 1000+ models work

The gateway keeps two things strictly separate:

1. **Provider = API shape.** ~30 egress transports cover the wire formats that
   essentially every model speaks. A model goes through whichever transport
   matches its provider.

2. **Model = a registry row.** 1000+ models are entries in the pricing/capability
   registry, not code. New models go live as registry updates, not deployments.

The escape hatch is a **passthrough route**: a brand-new provider endpoint works
immediately via the nearest OpenAI-compatible transport, with full cost tracking,
before a typed adapter exists. You lose translation niceties (cross-dialect tool
calls, etc.) but never availability.

---

## Provider-specific notes

### Prompt caching

Anthropic and OpenAI both support server-side prompt caching, where repeated
prefix tokens are served at a discounted rate. The gateway tracks
`cache_read_input_tokens` and `cache_creation_input_tokens` separately, prices
them at the provider-specific discount, and includes the correct amounts in
`usage.cost`. Cache-hit/miss status appears in the `x-cache-status` response
header (planned, P4).

### Streaming

All providers that support streaming (`supports_streaming: true` in the registry)
return SSE chunks via `text/event-stream`. The gateway normalizes chunk/delta
semantics across providers. Usage tokens are emitted in the final chunk.

### Tool / function calling

The gateway translates between OpenAI tool-call format and Anthropic/Gemini
native formats. Parallel tool calls are supported. The translation layer emits
explicit `UnsupportedOperationError` rather than silently dropping unsupported
parameters.

### Vision / multimodal

Models with `supports_vision: true` accept image parts in the `messages` array
(base64 or URL). The gateway forwards these to the provider in the provider's
native format.
