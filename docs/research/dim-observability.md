# Dimension Deep-Dive: Observability in AI Gateways

Competitive-intelligence report for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).
Researched 2026-06-10. Sources: official docs (LiteLLM, Portkey, Helicone, Bifrost, Kong, Envoy AI Gateway, Cloudflare, Vercel, TensorZero, Langfuse), OpenTelemetry spec/blog, GitHub issues, third-party comparisons.

---

## 1. The standard that matters: OTel GenAI Semantic Conventions

The single most important fact for this dimension: **the industry has converged on OpenTelemetry GenAI semantic conventions as the lingua franca**, and gateways that emit them natively interoperate for free with Datadog, Grafana, Honeycomb, New Relic, Langfuse, Phoenix, MLflow, etc. Datadog added native support in OTel v1.37; Grafana collects LLM traces in Loki. Most of the conventions are still **experimental/"Development" status** as of mid-2026, gated behind `OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental`.

### Span model
- Hierarchy: `invoke_agent` (agent span) → `chat`/inference spans per LLM call → `execute_tool` spans per tool invocation. Also `embeddings`, `create_agent`.
- Core attributes: `gen_ai.request.model`, `gen_ai.response.model`, `gen_ai.provider.name`, `gen_ai.operation.name` (chat | completion | embedding | rerank | image_generation | messages), `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.response.finish_reasons`.

### Content capture (the redaction boundary in the standard itself)
- **Default: NO prompt/completion/tool-argument content captured** — metadata only. This is a deliberate privacy stance baked into the spec.
- Opt-in via `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` with granular values: `NO_CONTENT` (default) | `SPAN_ONLY` | `EVENT_ONLY` | `SPAN_AND_EVENT`.
- When enabled, structured attributes `gen_ai.system_instructions`, `gen_ai.input.messages`, `gen_ai.output.messages` carry full messages, tool schemas, tool args/results. (Older approach: `gen_ai.{role}.message` / `gen_ai.choice` log events — the spec is mid-transition from events to span attributes; backends like Langfuse had to add explicit support for `gen_ai.input.messages`, see langfuse/langfuse#8840.)

### Metrics (all "Development" status)
| Metric | Type | Unit | Required attrs |
|---|---|---|---|
| `gen_ai.client.token.usage` | Histogram | {token} | operation.name, provider.name, `gen_ai.token.type` (input/output/total) |
| `gen_ai.client.operation.duration` | Histogram | s | operation.name, provider.name |
| `gen_ai.client.operation.time_to_first_chunk` | Histogram | s | streaming only |
| `gen_ai.client.operation.time_per_output_chunk` | Histogram | s | streaming only |
| `gen_ai.server.request.duration` | Histogram | s | — |
| `gen_ai.server.time_to_first_token` | Histogram | s | streaming only |
| `gen_ai.server.time_per_output_token` | Histogram | s | streaming only |

### Competing/adjacent convention: OpenInference
Envoy AI Gateway chose **OpenInference** (Arize's OTel-compatible spec) for traces rather than raw GenAI semconv; it records TTFT as a **span event** inside the streaming span and standardizes prompt/response/parameter attributes. TensorZero lets you pick the export format: `export.otlp.traces.format` = GenAI semconv **or** OpenInference. A new gateway should support both (it's a serializer flag, and Phoenix/Arize users expect OpenInference).

**Implication for a new gateway:** emit GenAI semconv natively (spans + the 7 metrics above), make content capture an explicit opt-in tri-state (off / spans / events), and offer OpenInference as an alternate trace format. This is now table stakes among the newest entrants (Envoy AI GW, Bifrost, LiteLLM's OTEL v2) and a gap in older ones.

---

## 2. Product-by-product observability surface

### LiteLLM (OSS, Python, MIT-licensed core + enterprise)
The widest but messiest surface — a callback architecture fanning out to **20+ logging integrations** (Langfuse, Datadog, OTel, S3/GCS buckets, Helicone, SigNoz, etc.).
- **Prometheus**: the deepest metrics surface of any gateway. Categories: spend/tokens per key/team/user (`litellm_spend_metric`, `litellm_total_tokens_metric`, plus token-type detail: cached input, reasoning, audio tokens), budget metrics (`litellm_remaining_team_budget_metric`, `litellm_api_key_budget_remaining_hours_metric`), rate-limit remaining (`litellm_remaining_api_key_requests_for_model`), proxy-level counters (`litellm_proxy_total_requests_metric`, `litellm_proxy_failed_requests_metric`, `litellm_callback_logging_failures_metric` — note: they meter their own logging-pipeline failures), pod health (`litellm_in_flight_requests`), deployment health (`litellm_deployment_state` 0/1/2, cooldowns, successful/failed fallbacks), latency split (`litellm_request_total_latency_metric` vs `litellm_overhead_latency_metric` vs `litellm_llm_api_latency_metric` vs `litellm_llm_api_time_to_first_token_metric`), Redis/DB queue internals.
- Labels: end_user, hashed_api_key, api_key_alias, model, team, user_email, requested_model vs litellm_model_name (alias vs actual), api_base, status_code, exception_class, route, optional `stream`.
- Config: `prometheus_metrics_config` for per-group metric/label allowlists; `custom_prometheus_metadata_labels`; `custom_prometheus_tags` with wildcard matching on headers (e.g. `User-Agent: RooCode/*` — labeling traffic by coding agent!); `prometheus_initialize_budget_metrics` cron; `PROMETHEUS_MULTIPROC_DIR` for multi-worker.
- **OTel v2** (opt-in `LITELLM_OTEL_V2=true`): one trace per request spanning HTTP → auth → guardrails → LLM call → DB writes, GenAI semconv, presets for Arize/Phoenix/Langfuse/Weave. This "trace the gateway internals too" shape is worth copying.
- **Redaction**: `turn_off_message_logging: true` (global), per-key override, per-request headers (`x-litellm-enable-message-redaction`), `logging_only` PII masking mode (Presidio masks the *logged* copy, not the actual LLM request), `mask_input`/`mask_output` metadata. Replaces content with "redacted by litellm" but preserves cost/tokens/identity metadata.
- Latency-overhead self-reporting: timing headers on every response showing proxy-added ms.
- **Weaknesses (documented GitHub issues)**: `/metrics` unauthenticated by default and exposes multi-tenant PII via labels (#24530); any API key can read all-tenant metrics (#13644); double-counted request counters (#19929); budget metrics show `inf` (#20528); Bedrock requests not counted (#17415); error spam for non-premium users from enterprise-gated metric paths (#7817); redaction bugs — `turn_off_message_logging` sometimes fails to redact output (#9507) and doesn't redact `proxy_server_request` stored to DB (#16336); DEBUG logging serializes payloads synchronously (2–5s on 2MB payloads); community-measured throughput drop 16→9 req/s with LiteLLM in path (#21046).

### Portkey (gateway OSS/MIT-ish; observability is the paid cloud)
The most complete *product* around observability; observability itself is **not** in the OSS gateway.
- Logs view: every request with full prompt/response, tokens, cost, latency, provider, model, custom metadata; "40+ details" per log; 15+ dashboard filters.
- Traces: multiple LLM calls grouped under one trace ID; per-step latency+cost breakdown for agent/RAG fan-out.
- Analytics: 21+ metrics; cost per model/user/day; alert thresholds (Slack ping when GPT-4o daily spend > $200, email when p95 latency > 8s).
- Feedback API: attach weighted feedback values to any request/trace.
- **OTLP ingestion backend**: `https://api.portkey.ai/v1/otel` — Portkey acts as a full OTel backend so app-level traces and gateway-enriched LLM logs land in one place; gateway calls auto-enriched with provider config, cache status, retry attempts, prompt versions. This "gateway = OTel sink for your whole app" move is distinctive.
- **MCP gateway observability** (most concrete in market): every MCP request logged with Tool, Parameters, Response, User, Team, Timestamp, Latency, Status, MCP Server; dashboards for tool popularity, error rates, latency percentiles, adoption by team/server/individual; filterable audit trail; trace correlation between LLM calls and tool calls in one view.
- Note: Portkey is reportedly headed to Palo Alto Networks (2026) — incumbent churn creates an opening.

### Helicone (OSS, but acquired by Mintlify Mar 2026 → maintenance mode)
Proxy-native observability; "observability by default, zero config."
- Every request logged with full I/O, tokens, latency, cost, metadata; filter/search across millions of requests.
- **Sessions via headers** — the cleanest session-grouping API in the market: `Helicone-Session-Id` (thread UUID), `Helicone-Session-Path` (hierarchical `/task/research/web_search` parent-child paths), `Helicone-Session-Name` (human label). Groups LLM calls + vector DB queries + tool calls into one agent-flow tree; session-level metrics (avg latency, total cost); "group by function, not by time" design principle.
- **HQL (Helicone Query Language)**: SQL-ish queryable logs (gated to Pro $79/mo and select workspaces).
- Custom properties via headers; webhooks with property-based filtering; Slack/email alerts; user analytics; reports.
- **Weaknesses**: logging is post-hoc (you see issues only after they hit users); HQL gated; product now frozen (maintenance mode) — its users are up for grabs, and its header-based session UX is the thing to steal.

### Bifrost / Maxim (OSS, Go, Apache-2.0)
Closest architectural comp to the proposed product (single Go binary, built-in dashboard).
- Built-in dashboard at `localhost:8080/logs`: token usage, cost, model breakdown, latency; filterable by virtual key and time window. Zero-config logging of every request.
- Native Prometheus at `/metrics` (optional basic auth): `bifrost_upstream_requests_total`, `bifrost_success/error_requests_total`, `bifrost_upstream_latency_seconds`, `bifrost_input/output_tokens_total`, `bifrost_cost_total` (USD!), `bifrost_cache_hits_total` (direct vs semantic label), `bifrost_provider_key_up`, `bifrost_key_rotation_events_total`, **streaming**: `bifrost_stream_first_token_latency_seconds`, `bifrost_stream_inter_token_latency_seconds`, `bifrost_active_requests`, `bifrost_request_retries`.
- Labels include routing decisions: routing_engine_used, routing_rule_id/name, selected_key_id/name, fallback_index, team/customer — governance dimensions as metric labels.
- **Custom metric labels injected per-request via `x-bf-dim-*` headers** (e.g. `x-bf-dim-team: engineering`) — elegant, agent-friendly.
- OTEL plugin exports OTLP traces with GenAI semconv to Grafana/Datadog/New Relic/Honeycomb. Async pipeline, claimed zero added latency.

### Kong AI Gateway (OSS core + Konnect paid analytics)
- AI plugins emit structured "AI analytics" logs (prompt+response metadata per request) through standard Kong logging plugins (HTTP Log, Datadog, Splunk, Prometheus).
- Token consumption (input/output/system), latency, error rates, cost per model/prompt-type.
- OTel instrumentation with per-request AI span attributes; aggregated metrics for **AI, MCP, and A2A protocols** (notable: they treat MCP and A2A as first-class metered protocols).
- Konnect Advanced Analytics: pre-built LLM dashboards, historical comparisons, real-time traffic maps showing client→model flows.
- Weakness: the good dashboards live in Konnect (SaaS, paid); OSS users assemble Kibana/Grafana themselves.

### Envoy AI Gateway (OSS, Go/Envoy, CNCF orbit)
The reference implementation for standards-correct gateway observability.
- Metrics exactly per GenAI semconv: `gen_ai.client.token.usage` (token.type label), `gen_ai.server.request.duration`, `gen_ai.server.time_to_first_token`, `gen_ai.server.time_per_output_token`; default attrs gen_ai.operation.name / original.model / request.model / response.model / provider.name. Covers chat, completions, embeddings, rerank, **Anthropic /v1/messages**, streaming + non-streaming.
- v0.3+ OTel tracing in **OpenInference** conventions: full request/response capture in spans, TTFT as span event — works out-of-box with Arize Phoenix.
- No bundled dashboard — Grafana/SkyWalking territory.

### Cloudflare AI Gateway (closed SaaS)
- Analytics: requests, tokens, cached, errors, cost per provider — dashboard + **GraphQL analytics API**.
- Logging: full prompt/response payload storage with per-request opt-out header `cf-aig-collect-log-payload: false` (metadata-only logging — clean pattern), log retention config, DLP actions visible in logs, up to 10M logs/gateway via Workers Logpush with **RSA-encrypted log export**.
- Evaluations: build datasets from filtered logs, run perf/speed/cost evaluations — log→dataset→eval loop inside the gateway.
- Custom metadata, custom costs per request. Weakness: closed, CF-lock-in, log storage caps.

### Vercel AI Gateway (closed SaaS)
What a clean minimal dashboard looks like: Usage charts (requests by model, **TTFT chart**, input/output token counts, spend over time); Requests summaries grouped by project and by API key — each row shows request count, avg tokens, **P75 duration, P75 TTFT**, cost; sortable/exportable detailed log. Team-level vs project-level scoping. Longer retention paywalled (Observability Plus).

### TensorZero (OSS, Rust)
- Observability is storage-first: every inference + feedback written to **ClickHouse**; UI + programmatic access over it; the stored data feeds optimization/eval/experimentation loops (observability data as training data — distinctive framing).
- OTLP **traces only** (no metrics/logs over OTLP); `export.otlp.traces.enabled=true`; **gRPC-only** endpoint (documented limitation); spans for function → variant → model → model-provider hierarchy; W3C traceparent propagation; per-request extra span/resource attributes via `tensorzero-otlp-traces-extra-attribute-*` headers; format switch GenAI semconv ↔ OpenInference.

### Observability backends gateways must export to
- **Langfuse** (OSS, 19k+ stars, the default answer): data model = traces / observations (generation|span|event) / sessions / scores; session replay; user-level cost; agent graph view (beta); native OTel ingestion. Self-host pain: v3 requires Postgres + ClickHouse + Redis + S3 + worker (~6 containers) — heavily complained about for small teams; ClickHouse data-loss footguns (#10778).
- **Datadog LLM Observability**: native GenAI semconv ingestion (OTel v1.37+).
- **Grafana**: LLM traces in Loki/Tempo; community Grafana dashboard JSONs are the de-facto deliverable (LiteLLM ships them).
- **SigNoz / OpenObserve / Phoenix / MLflow**: all consume OTLP GenAI; a gateway that emits compliant OTLP gets them all free.

---

## 3. Cross-cutting patterns

### What the best dashboards show (composite of Vercel, Bifrost, Portkey, Helicone, Konnect)
1. Spend over time + cost per model/user/team/key (cost is the #1 question).
2. Requests by model; error rate by provider with status-code breakdown.
3. **TTFT chart** and P75/P95/P99 duration — percentiles, not averages.
4. Input/output token counts (plus cached/reasoning token splits — LiteLLM is ahead here).
5. Cache hit rate (direct vs semantic), fallback/retry counts, provider health (up/cooldown).
6. Per-key/per-project summary tables (count, avg tokens, P75 duration, P75 TTFT, cost).
7. Drill-down: row → full request/response with metadata, timing breakdown, replay.
8. Session/trace tree for agent flows with per-step cost+latency.

### Streaming observability
Consensus triple: **TTFT** (queue+prefill), **inter-token/time-per-output-token latency** (decode cadence), **output token count** (distinguishes long completions from stalls). Delivered as Prometheus histograms (Bifrost, Envoy, LiteLLM) and as span events in the trace waterfall (OpenInference). Nobody yet renders a true "token-flow waterfall" for a single streamed response in OSS — a UI gap.

### Redaction / privacy controls (the leaders' feature set)
- Global "don't log message content" switch + per-key override + per-request header (LiteLLM).
- Per-request payload-storage opt-out header (Cloudflare `cf-aig-collect-log-payload`).
- Logging-only PII masking (mask the stored copy, not the wire request) via Presidio-class engines (LiteLLM `logging_only` mode).
- OTel-level tri-state content capture (NO_CONTENT/SPAN_ONLY/EVENT_ONLY).
- Encrypted log export (Cloudflare Logpush RSA).
- Known failure mode to avoid: redaction that misses secondary copies (LiteLLM redacts `messages` but leaked `proxy_server_request` in DB).

### Session / conversation grouping
Three mechanisms in the wild: (a) **header-based** — Helicone-Session-Id/Path/Name (best DX, agent-friendly, hierarchy via path); (b) **trace-ID-based** — Portkey trace IDs grouping steps; (c) **backend-native** — Langfuse sessions + session replay + session-level scores. Gateways win by accepting headers and propagating W3C traceparent (TensorZero does) so gateway spans join the app's distributed trace.

### Alerting / SLOs
Thin everywhere. Portkey: threshold alerts on spend/latency→Slack/email. Helicone: Slack/email alerts + property-filtered webhooks. LiteLLM: budget metrics + Grafana alerting left to user. No gateway ships first-class SLO objects (burn-rate, error-budget) for LLM traffic — open territory; the primitives (histograms with provider/model labels) exist.

### Agent experience (AX) observations
- **MCP-queryable observability is emerging as the agent-native interface**: Grafana MCP server (query metrics/logs, manage dashboards/alerts from an agent), Grafana Cloud hosted MCP, community Langfuse MCP server (agents query traces, triage exceptions, analyze sessions). No gateway ships its own "query my own telemetry over MCP" yet — a gateway with a built-in MCP control plane could let agents ask "why was p95 slow yesterday?" against its own store. Clear differentiation opportunity.
- Header-based everything is the agent-friendly config surface: Helicone session headers, Bifrost `x-bf-dim-*` metric labels, TensorZero `tensorzero-otlp-traces-extra-attribute-*`, LiteLLM redaction headers. Agents can attach observability context without SDKs.
- Machine-readable query surfaces: Cloudflare GraphQL analytics, Helicone HQL, Langfuse Metrics API, Portkey Analytics API. Vercel logs export. A first-class SQL/HQL-like endpoint over the gateway's own log store is high-value for agents.
- LiteLLM's `User-Agent: RooCode/*` custom Prometheus tags = precedent for *metering by coding agent* — labeling traffic by which agent (Claude Code, Cursor, Codex) generated it.
- Self-observability: LiteLLM meters its own callback failures and returns overhead-timing headers per response — agents can detect gateway degradation programmatically.

### MCP gateway observability specifically
Portkey defines the bar: per-tool-call logs (tool, params, response, user, team, latency, status, server), tool-popularity/error/latency dashboards, adoption by team, audit trail, LLM↔tool trace correlation. Kong meters MCP and A2A as protocols. Generic MCP-manager products (MCP Manager, MintMCP, TrueFoundry) sell tracing+audit as the core. OTel has no stable MCP semconv yet — whoever ships good default conventions for `execute_tool` spans through a gateway sets the standard.

---

## 4. Synthesis for the new gateway

**Table stakes** (everyone has it; users assume it): per-request logs with full I/O + tokens + cost + latency; Prometheus `/metrics`; cost tracking per key/team/model; OTLP trace export; TTFT + duration percentiles; integration recipes for Langfuse/Datadog/Grafana; content-logging opt-out; custom metadata/properties on requests.

**Best-in-class moves worth stealing**:
- Envoy/Bifrost: exact GenAI-semconv metrics incl. `time_to_first_token` / `time_per_output_token` histograms.
- LiteLLM OTEL v2: one trace per request including gateway internals (auth, guardrails, DB writes) — plus overhead-timing response headers.
- Helicone: session headers (Id/Path/Name) with hierarchical paths; queryable log SQL (HQL).
- Bifrost: `x-bf-dim-*` per-request custom metric labels; cost-in-USD as a Prometheus counter; built-in zero-config dashboard in the binary.
- Cloudflare: per-request `collect-log-payload: false` header; logs→datasets→evaluations loop; GraphQL analytics API.
- Portkey: OTLP *ingestion* (gateway as the app's OTel backend); MCP tool-call observability schema; threshold alerts.
- TensorZero: ClickHouse-backed inference store with dual-format (semconv/OpenInference) OTLP export and per-request extra-attribute headers.
- LiteLLM: token-type detail (cached/reasoning/audio), budget/rate-limit-remaining gauges, deployment-health state metric.

**Where incumbents are weak (the openings)**:
1. Metrics security: LiteLLM's default-open multi-tenant `/metrics` is a known CVE-shaped hole — ship authenticated, tenant-scoped metrics by default.
2. Observability paywalls: Portkey/Kong/Vercel gate the good dashboards; Helicone gates HQL; OSS + built-in dashboard in one binary (Bifrost-style but deeper) undercuts all of them.
3. Backend ops burden: Langfuse v3 self-host = 6 containers; a single binary with embedded store + OTLP-out is a massive simplification story.
4. Logging overhead: LiteLLM's synchronous serialization and ~44% throughput loss; async-by-design pipeline with self-metered overhead is a perf differentiator.
5. No one ships SLOs, no one ships MCP-queryable self-telemetry, no one renders streaming token waterfalls, MCP tool-call semconv is unclaimed, and Helicone (maintenance mode) + Portkey (PANW acquisition) leave users looking for a home.
