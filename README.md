<div align="center">

# Oximy Gateway

**The unified, fastest, open-source LLM + MCP gateway.**
One binary. Embedded dashboard. Agent-first.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](./LICENSE)
[![Status](https://img.shields.io/badge/status-pre--alpha%20scaffold-orange)](./docs/2026-06-10-oximy-gateway-design.md)

</div>

---

Oximy Gateway is one place to route, govern, observe, and secure **all** of your
organization's AI traffic — both **LLM calls** (OpenAI, Anthropic, Gemini,
Bedrock, Vertex, and 1000+ models) and **MCP tool calls** — through a single
shared governance spine.

It is the empty intersection in today's market: **(a) LLM + MCP unified in one
OSS single binary, (b) a control plane that agents can fully operate, and (c)
compiled-language performance with batteries-included governance.**

```bash
# (planned) one command boots the gateway and opens the dashboard
oximy-gateway up
```

## Why

- **Unified governance.** One virtual key's USD budget covers model tokens *and*
  tool calls. One audit log spans both. One guardrail policy applies to prompts
  *and* tool I/O. One telemetry store answers "what did this agent session cost?"
- **Fast.** Rust core; target sub-1ms p99 self-overhead — and we publish the
  *fully-loaded* numbers (policies on, streaming TTFT, MCP path) nobody else does.
- **1000+ models.** ~30 provider adapters + a hot-reloading model registry +
  cost-tracked passthrough = every model, new ones live the day they ship.
- **Agent-first.** Operate the whole gateway over an admin-MCP server and an
  AXI-grade CLI. Agents install servers, mint scoped sub-keys, set guardrails,
  and query their own telemetry.
- **Independent & honest.** Apache-2.0, no license keys, no rug-pulls, and we
  never claw back a shipped OSS feature. One-command migration from LiteLLM/Portkey.

## Status

**Pre-alpha scaffold.** The architecture is settled (see the design doc); the
crates compile as a workspace skeleton. Implementation proceeds in phases:

| Phase | Scope |
|---|---|
| **P1** | Shared spine + LLM core (OpenAI/Anthropic/Responses ingress, Tier-1 providers, keys, budgets, exact cache, cost, logs, single binary + dashboard) |
| **P2** | MCP plane on the same spine (federation, virtual servers, OAuth, tool ACL + dollar metering) |
| **P3** | Agent-first control plane (admin-MCP, AXI CLI, config diff/apply, attenuated sub-keys) |
| **P4** | Differentiators (hedging, mid-stream failover, semantic cache, guardrail dry-run, conformance + honest benchmarks) |
| **P5** | OSS ops + breadth completion (modalities, plugin ecosystem, signed releases, migration tooling) |

## Architecture

A single core (`gateway-spine`) owns identity, virtual keys, budgets, policy,
audit, and telemetry. The **LLM plane** and **MCP plane** are adapters on that
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

See [`docs/2026-06-10-oximy-gateway-design.md`](./docs/2026-06-10-oximy-gateway-design.md)
for the full design, and [`docs/research/`](./docs/research/) for the 65-agent
competitive study behind it.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `gateway-spine` | identity, keys, budgets, RBAC, audit, policy, pricing registry |
| `gateway-llm` | LLM ingress adapters + egress transports + translation core |
| `gateway-mcp` | MCP federation, bridging, virtual servers, OAuth broker, tool ACL |
| `gateway-route` | fallback / retries / hedging / LB / cache-affinity / breakers |
| `gateway-cache` | exact + semantic + provider prompt-cache passthrough |
| `gateway-telemetry` | embedded columnar store, OTel GenAI/MCP semconv, Prometheus |
| `gateway-guard` | guardrail hooks + WASM plugin host |
| `gateway-config` | one schema'd config source: dump/diff/apply, hot reload |
| `gateway-control` | admin REST API + admin-MCP server + AXI CLI |
| `gateway-dash` | embedded web dashboard (thin client of the API) |
| `oximy-gateway` | the binary: wires it all; `oximy-gateway up` |

## License

[Apache-2.0](./LICENSE). No license keys, ever. See [SECURITY.md](./SECURITY.md)
and [CONTRIBUTING.md](./CONTRIBUTING.md).
