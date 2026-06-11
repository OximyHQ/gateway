# AGENTS.md — Oximy Gateway

Guidance for AI agents (and humans) working in this repo. Oximy Gateway is
**agent-first**: agents are expected to build, operate, and extend it.

## What this is

A unified, open-source **LLM + MCP gateway** in Rust — single binary, embedded
dashboard, agent-operable control plane. Architecture is settled in
`docs/2026-06-10-oximy-gateway-design.md`; read it before non-trivial work.

## Repo map

- `crates/gateway-spine` — the governance core; **its invariants are sacred** (see below).
- `crates/gateway-llm` / `gateway-mcp` — the two protocol planes (adapters on the spine).
- `crates/gateway-{route,cache,telemetry,guard,config,control,dash}` — supporting subsystems.
- `crates/oximy-gateway` — the binary.
- `docs/` — design doc, phase plans (`docs/plans/`), competitive research (`docs/research/`).

## Spine invariants — do NOT violate

These are the anti-LiteLLM discipline. A change that breaks one of these is a bug,
even if tests pass:

1. **Fail-closed budgets** — hard-block *before* the upstream call; never fail-open.
2. **No double-billing** — one idempotency key reused across all retries/failovers;
   cost committed once from provider-reported usage.
3. **No overspend under concurrency** — atomic reserve → commit (true-up) → refund.
4. **Auth-by-default** — admin API/UI/metrics authenticated out of the box.
5. **Cost-correctness is security-grade** — cached-token, streaming, and
   aborted-stream usage all reconciled.

## How to work here

- **Build:** `cargo build` · **Test:** `cargo test` · **Lint:** `cargo clippy --all-targets -- -D warnings` · **Format:** `cargo fmt`
- `#![forbid(unsafe_code)]` is on in every crate — keep it.
- TDD: write the failing test first (see the project's test-driven workflow), then
  implement. Translation work **must** be covered by golden-fixture conformance
  tests against real client request shapes.
- One coherent config, not plugin confetti. New capability → extend the schema'd
  config, surfaced identically through API = CLI = MCP = dashboard = Git.
- Keep crates focused; if a file grows large it's probably doing too much.

## CLI / AX conventions (when adding commands)

- `--json` on every command; semantic exit codes (e.g. distinct "already exists");
  idempotent mutations; `--dry-run`; definitive empty states; next-step hints.

## Before opening a PR

- `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` all green.
- No secrets in code or logs. Sign-off (DCO) on commits.
