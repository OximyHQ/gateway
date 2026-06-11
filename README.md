<div align="center">

# Oximy Gateway

**The unified, fastest, open-source LLM + MCP gateway.**
One binary. Embedded dashboard. Agent-first.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](./LICENSE)
[![Status](https://img.shields.io/badge/status-alpha-orange)](./docs/2026-06-10-oximy-gateway-design.md)

</div>

---

Oximy Gateway is a single Rust binary that puts **all of your AI traffic** — both
LLM calls and MCP tool calls — through one shared governance spine. One command
boots it. One bearer key authenticates it. One budget covers tokens *and* tools.

```
oximy-gateway up
```

> **Status: alpha.** The architecture is settled and the core is implemented (spine,
> LLM plane, MCP gateway, virtual keys, guardrails, dashboard). Expect breaking
> changes before 1.0. See the [phase roadmap](#status--roadmap) below.

---

## Why Oximy Gateway

**No other open-source gateway combines these three things:**

1. **LLM + MCP unified** — one virtual key's USD budget covers model tokens *and*
   MCP tool calls. One audit log spans both. One guardrail policy applies to
   prompts *and* tool I/O. One telemetry store answers "what did this agent session
   cost?"

2. **Compiled-language performance with batteries-included governance** — Rust
   core; sub-1ms p99 self-overhead target; and we publish the *fully-loaded*
   numbers (policies on, streaming TTFT, MCP path) that nobody else does. The
   `x-overhead-duration-ms` header on every response makes the benchmark a live
   product feature.

3. **A control plane agents can fully operate** — admin-MCP server, AXI-grade CLI,
   config-as-code diff/apply. Agents install MCP servers, mint scoped sub-keys, set
   guardrails, and query their own telemetry.

**And it is independent.** Apache-2.0, no license keys, no rug-pulls. We never
claw back a shipped OSS feature. One-command migration from LiteLLM/Portkey.

---

## 60-Second Quickstart

**1. Install**

```bash
# macOS (Homebrew)
brew install oximyhq/tap/oximy-gateway

# Linux / macOS (installer script)
curl -fsSL https://raw.githubusercontent.com/OximyHQ/gateway/main/install.sh | sh

# Cargo (from source)
cargo install --git https://github.com/OximyHQ/gateway oximy-gateway
```

**2. Set at least one provider key**

```bash
export OPENAI_API_KEY=sk-...
# Optionally also:
# export ANTHROPIC_API_KEY=sk-ant-...
# export GEMINI_API_KEY=AIza...
# export OPENROUTER_API_KEY=sk-or-...
```

**3. Boot**

```bash
oximy-gateway up
```

On first boot you will see:

```
  ┌─ First boot ──────────────────────────────────
  │  A default admin key was created. It is shown ONCE:
  │
  │     ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  │
  │  Use it as your Bearer token for the API and dashboard.
  │  Store it now — it cannot be recovered.
  └───────────────────────────────────────────────

  Oximy Gateway is running.

  Dashboard:  http://127.0.0.1:8080/
  API base:   http://127.0.0.1:8080/v1
  Health:     http://127.0.0.1:8080/health
  Models:     http://127.0.0.1:8080/v1/models (auth required)
```

Save that key. It is never shown again.

**4. Make your first request**

```bash
export OXIMY_KEY=ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# curl
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $OXIMY_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello from Oximy Gateway!"}]
  }'
```

```python
# Python — OpenAI SDK, just change base_url
from openai import OpenAI

client = OpenAI(
    api_key="ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    base_url="http://127.0.0.1:8080/v1",
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello from Oximy Gateway!"}],
)
print(response.choices[0].message.content)
# Response includes: response.usage.cost  (exact USD)
```

```javascript
// Node — OpenAI SDK, same pattern
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
  baseURL: "http://127.0.0.1:8080/v1",
});

const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages: [{ role: "user", content: "Hello from Oximy Gateway!" }],
});
console.log(response.choices[0].message.content);
```

Every response carries exact cost in `usage.cost` (USD) and gateway overhead in
`x-overhead-duration-ms`.

**5. Open the dashboard**

The browser opens automatically at `http://127.0.0.1:8080/`. Log in with your
admin key to see request logs, spend by key/model, and provider status.

---

## Feature Overview

### LLM Plane

- **OpenAI-compatible ingress** — `POST /v1/chat/completions`, `/v1/responses`,
  `/v1/messages` (Anthropic dialect), streaming SSE
- **30+ provider egress transports** — OpenAI, Anthropic, Gemini, OpenRouter, Groq,
  Together, Fireworks, DeepSeek, xAI, Mistral, Perplexity, Cerebras, and more via
  `OPENAI_BASE_URL` override
- **1000+ models** — hot-reloading model registry; new models are data, not code
- **`GET /v1/models`** — machine-readable catalog with pricing, context window, and
  capability flags
- **Exact cost tracking** — cached-token math, streaming usage, aborted-stream
  usage all reconciled; `usage.cost` in every response

### MCP Plane

- **`POST /mcp`** — authenticated JSON-RPC 2.0 MCP gateway
- **Server federation** — register N upstream MCP servers (HTTP or stdio); tools
  are namespaced as `server__tool`
- **Per-key tool ACLs** — restrict which tools a virtual key can call
- **Audit log** — every tool call is recorded on the shared spine audit stream
- **Tool description hashing** — alerts when an upstream server silently changes a
  tool definition (rug-pull guard)

### Governance Spine

- **Virtual keys** — `oximy-gateway keys create --budget 10.00 --models gpt-4o`
- **USD budgets** — hard fail-closed before the upstream call; atomic
  reserve/commit/refund; never fail-open
- **Rate limits** — RPM and TPM per key
- **Model allowlists** — restrict a key to specific models
- **Guardrails** — built-in PII detection, secrets scanning (OpenAI keys, AWS
  access keys, GitHub tokens, Slack tokens, GitLab tokens), keyword banning, regex
  denylist, JSON schema validation; `Enforce` / `ObserveOnly` / `DryRun` modes

### Observability

- **`GET /metrics`** — authenticated Prometheus / OpenMetrics endpoint
- **Request log** — every request recorded off the hot path (async, non-blocking)
- **Response headers** — `x-overhead-duration-ms`, `x-served-by`,
  `x-fallback-fired`, `x-idempotency-key`
- **`GET /health`** — unauthenticated liveness probe

### Operations

- **Single binary** — no Kubernetes, no Postgres required for basic use; SQLite
  default, Postgres optional
- **Embedded dashboard** at `GET /` — thin REST-API client, no server-side
  rendering, boots with the binary
- **`--no-open`** flag for headless / CI use
- **Data directory** — `--dir`, `$OXIMY_GATEWAY_DIR`, or platform default
  (`~/.local/share/oximy-gateway` on Linux,
  `~/Library/Application Support/com.oximy.oximy-gateway` on macOS)

---

## Architecture

A single governance core (`gateway-spine`) owns identity, virtual keys, budgets,
policy, audit, and telemetry. The LLM plane and the MCP plane are adapters on that
spine — so unification is structural, not cosmetic.

```
   OpenAI / Anthropic /        ┌─────────────────────────────────────────┐
   Gemini / Responses  ───────▶│ LLM Ingress ─┐                          │
                               │               ▼                         │
   MCP clients (Claude         │          ┌──────────────┐               │
   Code / Cursor / ...) ──────▶│ MCP In ─▶│ SHARED SPINE │               │
                               │          │ keys·budgets·policy·audit·   │
                               │          │ telemetry·pricing·cache·RL   │
                               │          └──────┬───────┘               │
                               │  LLM Egress: 30+ providers              │
                               │  MCP Egress: federated servers          │
                               └─────────────────────────────────────────┘
                                        oximy-gateway (one binary)
```

### Workspace layout

| Crate | Responsibility |
|---|---|
| `gateway-spine` | identity, keys, budgets, RBAC, audit, policy, pricing registry |
| `gateway-llm` | LLM ingress adapters + egress transports + translation core |
| `gateway-mcp` | MCP federation, bridging, virtual servers, OAuth broker, tool ACL |
| `gateway-route` | fallback / retries / hedging / LB / cache-affinity / breakers |
| `gateway-cache` | exact + semantic + provider prompt-cache passthrough |
| `gateway-telemetry` | embedded columnar store, OTel GenAI/MCP semconv, Prometheus |
| `gateway-guard` | guardrail hooks (PII, secrets, keyword, regex, schema, webhook) |
| `gateway-config` | one schema'd config source: dump/diff/apply, hot reload |
| `gateway-control` | admin REST API + admin-MCP server + AXI CLI |
| `gateway-dash` | embedded web dashboard (thin client of the API) |
| `oximy-gateway` | the binary: wires it all; `oximy-gateway up` |

---

## Guides

| Guide | Contents |
|---|---|
| [Quickstart](./docs/guide/quickstart.md) | Install, boot, first request, dashboard tour, mint a key |
| [Configuration](./docs/guide/configuration.md) | Env vars, `oximy-gateway.json`, hot reload |
| [Providers](./docs/guide/providers.md) | Supported providers, env keys, base-URL override |
| [Keys & Budgets](./docs/guide/keys-budgets.md) | Virtual keys, USD budgets, rate limits, allowlists |
| [Guardrails](./docs/guide/guardrails.md) | PII/secrets detection, enforcement modes |
| [MCP Gateway](./docs/guide/mcp.md) | MCP federation, `POST /mcp`, tool ACLs |
| [Observability](./docs/guide/observability.md) | Prometheus metrics, request logs, cost headers |
| [Deployment](./docs/guide/deployment.md) | Binary, Docker, reverse proxy, data dir |

---

## Status & Roadmap

| Phase | Scope | Status |
|---|---|---|
| **P1** | Shared spine + LLM core — OpenAI/Anthropic/Gemini ingress, Tier-1 providers, virtual keys, USD budgets, exact cost, request logs, single binary + embedded dashboard | **In progress** |
| **P2** | MCP plane — federation, transport bridging, virtual servers, OAuth 2.1, per-key tool ACLs, tool-call dollar metering | In progress |
| **P3** | Agent-first control plane — admin-MCP server, AXI CLI, config diff/apply, attenuated sub-keys, agent-queryable telemetry | Planned |
| **P4** | Differentiators — request hedging, mid-stream failover, semantic cache, guardrail dry-run, conformance suite + honest fully-loaded benchmarks | Planned |
| **P5** | OSS ops + breadth — remaining modalities (images/audio/rerank), plugin ecosystem, signed releases, migration tooling from LiteLLM/Portkey | Planned |

---

## License

[Apache-2.0](./LICENSE). No license keys, ever. OSS features never shrink.

See [SECURITY.md](./SECURITY.md) and [CONTRIBUTING.md](./CONTRIBUTING.md).
The design rationale lives in
[`docs/2026-06-10-oximy-gateway-design.md`](./docs/2026-06-10-oximy-gateway-design.md).
