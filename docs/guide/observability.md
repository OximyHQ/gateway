# Observability

Oximy Gateway provides three observability surfaces: **response headers** (per-
request, zero configuration), **Prometheus metrics** (authenticated endpoint), and
a **request log** (embedded async store, viewable in the dashboard).

Telemetry is always written off the hot path — a slow or failing telemetry write
never blocks or fails a request.

---

## Response headers

Every response from `/v1/*` and `/mcp` includes gateway-specific headers:

| Header | Description |
|---|---|
| `x-overhead-duration-ms` | Gateway processing time in milliseconds (excludes upstream latency) |
| `x-served-by` | Provider and model that served the request (e.g., `openai/gpt-4o`) |
| `x-fallback-fired` | `true` if a failover route was used |
| `x-idempotency-key` | The idempotency key used for this request (reused across retries) |

`x-overhead-duration-ms` is the always-on benchmark feature: you can measure
gateway overhead on every real production request without any additional tooling.

### Cost in the response body

Every non-streaming chat response includes `usage.cost` (exact USD):

```json
{
  "usage": {
    "prompt_tokens": 1024,
    "completion_tokens": 256,
    "total_tokens": 1280,
    "cost": 0.005120
  }
}
```

Cost is calculated from provider-reported token counts using the model's registered
pricing, including:
- Input tokens at full rate
- Cached input tokens at the cache-read discount
- Cache-creation tokens at the cache-write rate
- Output tokens at the output rate

For streaming responses, `usage.cost` is not in the stream body (provider usage is
in the final chunk). The cost appears in the request log after the stream closes.

---

## Prometheus metrics

```bash
GET /metrics
Authorization: Bearer <any valid key>
```

The `/metrics` endpoint is authenticated with the same bearer tokens as `/v1/*`.
This prevents accidental exposure of request volumes and cost data (a known issue
in other gateways that left `/metrics` unauthenticated).

Content type: `application/openmetrics-text` (Prometheus scrape-compatible).

### Available metrics

| Metric | Type | Description |
|---|---|---|
| `gateway_requests_total` | Counter | Total requests, labeled by model, provider, status |
| `gateway_request_duration_ms` | Histogram | End-to-end request latency |
| `gateway_overhead_duration_ms` | Histogram | Gateway self-overhead (matches `x-overhead-duration-ms`) |
| `gateway_tokens_total` | Counter | Total tokens, labeled by type (input/output/cache_read/cache_write) |
| `gateway_cost_usd_total` | Counter | Total cost in USD, labeled by key and model |
| `gateway_dropped_rows` | Counter | Telemetry rows dropped due to channel backpressure |
| `gateway_guardrail_verdicts_total` | Counter | Guardrail outcomes, labeled by guardrail type and verdict |
| `gateway_mcp_calls_total` | Counter | MCP tool calls, labeled by server, tool, and outcome |
| `gateway_budget_exhausted_total` | Counter | Requests rejected due to budget exhaustion |
| `gateway_rate_limited_total` | Counter | Requests rejected due to rate limits |

### Scraping with Prometheus

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: oximy-gateway
    static_configs:
      - targets: ['127.0.0.1:8080']
    authorization:
      credentials: ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### Grafana dashboard

A pre-built Grafana dashboard (JSON) is available at
`docs/grafana/oximy-gateway-dashboard.json` (coming in P4). It covers request
volume, latency percentiles, cost by model/key, guardrail violations, and budget
exhaustion rates.

---

## Request log

Every request is logged asynchronously to an embedded in-memory store (with
optional durable persistence). Each log row contains:

| Field | Description |
|---|---|
| `ts_ms` | Timestamp (milliseconds since Unix epoch) |
| `kind` | Request type: `llm` or `mcp` |
| `key_id` | Virtual key that authorized the request |
| `model` | Model ID requested |
| `provider` | Provider that served it |
| `usage` | Token counts (input, output, cache_read, cache_write) |
| `cost` | Exact USD cost |
| `latency_ms` | End-to-end latency |
| `status` | HTTP status code |
| `served_by` | Provider/model string |
| `fallback_fired` | Whether a failover occurred |
| `cache_status` | `hit`, `miss`, or `bypass` |

View request logs in the **Requests** tab of the dashboard. Logs are searchable
and filterable by key, model, status, date range, and cost.

---

## Health check

```bash
GET /health
```

Unauthenticated liveness probe. Returns 200 with:

```json
{
  "status": "ok",
  "version": "0.x.y"
}
```

Use this endpoint in load balancer health checks and Kubernetes liveness probes.
No authentication required — health checks must not fail because a token expired.

---

## Structured logging

The gateway emits structured JSON logs to stderr via `tracing`. Control verbosity
with `RUST_LOG`:

```bash
RUST_LOG=info oximy-gateway up        # request summaries (default)
RUST_LOG=debug oximy-gateway up       # full request/response detail
RUST_LOG=warn oximy-gateway up        # warnings and errors only
```

Log lines include: timestamp, level, target (crate/module), and structured fields
(request ID, provider, model, latency).

---

## OTel GenAI semconv export (optional)

The `gateway-telemetry` crate is built to emit OpenTelemetry spans following the
GenAI semantic conventions (`gen_ai.*` attributes) and MCP semconv (when
standardized). This is wired as an optional export adapter — **default-off** to
preserve the standalone posture.

To enable (planned, P4):

```json
{
  "telemetry": {
    "otel": {
      "enabled": true,
      "endpoint": "http://localhost:4317",
      "service_name": "oximy-gateway"
    }
  }
}
```

When enabled, spans are emitted for every LLM call and MCP tool call with
`gen_ai.system`, `gen_ai.request.model`, `gen_ai.usage.input_tokens`,
`gen_ai.usage.output_tokens`, and `gen_ai.usage.cost` attributes. This integrates
with any OTel-compatible backend (Jaeger, Tempo, Honeycomb, Oximy's own
ClickHouse substrate, etc.).

---

## Cost attribution

Cost is tracked per virtual key. Aggregate spend by key is available:

- In the dashboard: **Keys** tab shows `spent / budget` for each key
- Via Prometheus: `gateway_cost_usd_total{key_id="..."}` counter
- In the request log: every row has the key ID and cost

For multi-tenant setups, create one virtual key per tenant. Their spend is
automatically isolated in the request log and metrics.
