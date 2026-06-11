# Changelog

All notable changes to Oximy Gateway are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/) from 1.0 onward.

## [Unreleased]

### Added
- Initial repository scaffold: Apache-2.0 license, 11-crate Cargo workspace
  (`gateway-spine`, `gateway-llm`, `gateway-mcp`, `gateway-route`, `gateway-cache`,
  `gateway-telemetry`, `gateway-guard`, `gateway-config`, `gateway-control`,
  `gateway-dash`, and the `oximy-gateway` binary).
- Design doc and 65-agent competitive research under `docs/`.
- CI (build/test/clippy/fmt) and nightly workflow skeletons.
- `oximy-gateway` CLI skeleton (`up`, `version`, `help`).
- **`gateway-spine`** (P1.1): integer-only µUSD money, exact-integer cost, model
  registry with unknown-model-NULL discipline, virtual keys, atomic budget ledger
  (fail-closed reserve/commit/release, proven no-overspend under concurrency),
  RPM/TPM/parallel rate limiter, audit log.
- **`gateway-llm`** (P1.2): unified LLM request/response/stream types, SSE decoder,
  the `Provider` trait (idempotency-key threaded for no-double-billing), and
  OpenAI / Anthropic / Gemini egress transports (mocked-HTTP conformance tests).
- **`gateway-cache`** (P1.5): exact-match response cache (tenant-scoped, 200s-only,
  per-request controls, HIT/MISS/age, streaming replay, hit-rate/$-saved stats,
  L1 + optional Redis L2 seam) and atomic model-registry hot reload.
- **`gateway-telemetry`** (P1.7): async off-hot-path request-log/spend sink,
  in-memory spend store with grouped queries, authenticated Prometheus surface,
  OTel + Oximy-export trait seams (default-off).
- **`gateway-config`** (P1.6, config half): schema-validated config model with
  JSON Schema, env-var interpolation, validate/dry-run, AEAD-encrypted secrets,
  and a decK-style diff/apply engine.
- **`gateway-control`** (P1.4): the Axum HTTP server — `/v1/chat/completions`
  (+SSE streaming), `/v1/responses`, `/v1/messages`, `/v1/embeddings`, `/v1/models`,
  authenticated `/metrics`. Full request lifecycle (auth → allowlist → rate-limit →
  budget reserve → guard → provider egress → commit actual cost → telemetry) with
  `SpineError`/`ProviderError` → HTTP mapping (incl. upstream-4xx passthrough) and
  auth-before-body (unauthenticated → 401 regardless of content-type).
- **`oximy-gateway up`** (P1.8a): zero-config first boot (auto-generate + print a
  default admin key once), provider registration from env (OpenAI, Anthropic,
  Gemini, **OpenRouter** + `OPENAI_BASE_URL` override), TCP serve, `/health`, and an
  embedded HTML status page. One command boots a live gateway.
- Telemetry wired into the lifecycle: every request logged off the hot path +
  recorded into authenticated Prometheus metrics (`gateway_requests_total`,
  `gateway_cost_micros_total`).
- 8 milestone implementation plans under `docs/plans/` (~18k lines, TDD).
- **284 tests** passing across the workspace; clippy `-D warnings` and fmt clean.
- **Verified end-to-end against real LLMs** via OpenRouter (gpt-4o-mini,
  claude-3.5-haiku, deepseek-chat, llama-3.3-70b): real completions + streaming,
  exact integer-µUSD cost tracking, and auth/budget/model governance enforced.
