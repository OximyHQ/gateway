# Contributing to Oximy Gateway

Thanks for your interest. Oximy Gateway is Apache-2.0 and built in the open.

## Ground rules

- **Read first:** `docs/2026-06-10-oximy-gateway-design.md` (architecture) and
  `AGENTS.md` (working conventions + the **spine invariants you must not violate**).
- **DCO sign-off:** all commits require `Signed-off-by` (`git commit -s`).
- **License:** contributions are accepted under Apache-2.0.

## Development

```bash
cargo build                                   # build the workspace
cargo test                                    # run tests
cargo clippy --all-targets -- -D warnings     # lint (warnings are errors in CI)
cargo fmt --all                               # format
```

Requirements: Rust (pinned in `rust-toolchain.toml`). `#![forbid(unsafe_code)]`
is on everywhere.

## Workflow

1. **Test-driven.** Write the failing test, then implement. Translation/streaming
   changes require golden-fixture conformance coverage.
2. **Focused PRs.** One concern per PR; keep crates small and single-purpose.
3. **Config-as-code discipline.** A new capability is surfaced identically through
   API, CLI, MCP, dashboard, and Git — never one without the others.
4. **Benchmarks.** Performance-sensitive changes should run the in-repo harness;
   regressions block release.

## Good first issues

Look for the `good-first-issue` label once the issue tracker opens. Provider
adapters, model-registry entries, and CLI command polish are good entry points.

## Code of Conduct

Be respectful. Harassment or abuse is not tolerated. Report concerns to
conduct@oximy.com.
