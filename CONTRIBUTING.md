# Contributing to Oximy Gateway

Thanks for your interest. Oximy Gateway is Apache-2.0 and built in the open.
Contributions of all kinds are welcome: provider adapters, model-registry entries,
guardrail rules, CLI polish, documentation, and tests.

## Before you start

- **Read the architecture:** [`docs/2026-06-10-oximy-gateway-design.md`](./docs/2026-06-10-oximy-gateway-design.md)
- **Read the working conventions:** [`AGENTS.md`](./AGENTS.md) — especially the
  **spine invariants**, which are non-negotiable
- **DCO sign-off required:** all commits must include `Signed-off-by`
  (`git commit -s` adds it automatically)
- **License:** contributions are accepted under Apache-2.0

## Development setup

Requirements: Rust (version pinned in `rust-toolchain.toml`).

```bash
# Build the workspace
cargo build

# Run all tests
cargo test

# Lint (warnings are errors in CI)
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt --all

# Check formatting without writing
cargo fmt --all -- --check
```

`#![forbid(unsafe_code)]` is enforced in every crate.

## Workflow

1. **Test-driven.** Write the failing test, then implement. For translation and
   streaming work, golden-fixture conformance tests against real client request
   shapes are required.

2. **Focused PRs.** One concern per PR; keep crates small and single-purpose. A
   large change that touches multiple crates should be split or discussed first in
   an issue.

3. **Config-as-code discipline.** A new capability must be surfaced identically
   through the REST API, CLI, admin-MCP, dashboard, and `oximy-gateway.json` —
   never one surface without the others.

4. **Spine invariants.** Changes that touch `gateway-spine` or the request
   lifecycle must not violate the five invariants listed in `AGENTS.md`
   (fail-closed budgets, no double-billing, no overspend under concurrency,
   auth-by-default, cost-correctness). A change that breaks one of these is a bug
   even if tests pass.

5. **Benchmarks.** Performance-sensitive changes should include a run of the in-
   repo benchmark harness; regressions block release. Use `x-overhead-duration-ms`
   in integration tests to catch latency regressions.

## Good first issues

Look for the `good-first-issue` label on the issue tracker. Good entry points:
- Adding a provider adapter (copy an existing one, wire up the env key)
- Adding model-registry entries (pricing data, no code required)
- CLI command polish (better output formatting, `--json` flags)
- Documentation improvements and example additions

## Commit messages

Use conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:`). Keep the
subject line under 72 characters. Reference the relevant issue or design doc
section when the change is non-obvious.

## PR checklist

Before opening a pull request:

- [ ] `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` all green
- [ ] No secrets, credentials, or provider API keys in code or logs
- [ ] DCO `Signed-off-by` on all commits (`git commit -s`)
- [ ] Golden-fixture tests added for any translation or streaming change
- [ ] Spine invariants not violated

## Code of Conduct

Be respectful and constructive. Harassment, abuse, or discrimination of any kind
is not tolerated. Report concerns to conduct@oximy.com.
