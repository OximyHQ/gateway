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
- 8 milestone implementation plans under `docs/plans/` (~18k lines, TDD).
- 221 tests passing across the workspace; clippy `-D warnings` and fmt clean.
