# Dimension Deep-Dive: OSS Project Operations

**For:** team building a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane)
**Date:** 2026-06-10
**Scope:** How top OSS infra projects run release engineering, versioning/changelogs, CI, testing, docs, community, security, telemetry, and packaging — with concrete evidence from LiteLLM, TensorZero, Helicone (incl. Helicone AI Gateway), and Kong.

---

## 1. Release Engineering

### LiteLLM (BerriAI/litellm — Python, ~50k stars, 1,355+ releases)
The most evolved (and most battle-scarred) release pipeline among AI gateways:

- **Three-tier release ladder** (docs.litellm.ai/docs/proxy/release_cycle):
  1. **Nightly** (`1.x.x-dev.N` / `-nightly` tags) — auto-published whenever CI passes; no manual review.
  2. **Release Candidate** (`1.x.x-rc.N`) — CI pass + manual review + a **7-day early-tester window** for issue reports.
  3. **Stable** (`1.x.x`) — RC promoted after a second round of manual testing; **stable Docker images undergo a 12-hour load test before publication**.
- **Cadence:** stable release **every week, typically Sunday**; each scheduled stable bumps MINOR (1.84.0 → 1.85.0); PATCH reserved for hotfixes; MAJOR for breaking changes. Once a new stable ships, **older stables lose support** (90 days extended support across major transitions only).
- **Versioning cleanup (v1.84.0, 2026):** moved to strict SemVer 2.0 / PEP 440; dropped the legacy `-stable` Docker suffix; Docker publishes both bare (`1.84.0`) and v-prefixed (`v1.84.0`) tags; PyPI uses bare PEP 440 only. Legacy `main-stable` rolling tag being retired June 30, 2026; `:latest` is the canonical rolling pointer.
- **Supply-chain incident → CI/CD v2 (March 24, 2026):** LiteLLM suffered a supply-chain breach, paused all releases for a week, did forensic review with Mandiant + Veria Labs, and rebuilt the pipeline. v1.83.0 was the first release from "CI/CD v2": **isolated build environments, ephemeral credentials**, all Docker images **signed with cosign** (one published key, verifiable). This is the cautionary tale of the category: a security product's own release pipeline got popped.
- Artifacts: PyPI (`litellm`, `litellm[proxy]`), GHCR Docker (plus `litellm-database` variant images), Helm chart in-repo (`helm/litellm`), Terraform configs.

### TensorZero (Rust, ~10k+ stars)
- **CalVer**: `YYYY.MM.PATCH` (e.g. `2025.01.0`, `2025.01.1`) — monthly minor, patch for fixes. A deliberate signal that "there are no breaking-change majors; we ship continuously." Documented in a public `RELEASE_GUIDE.md` in-repo.
- **Per-release artifact fan-out:** PyPI client (GitHub Actions triggered on release), npm client, **3 Docker images (gateway / UI / evaluations)**, all **multi-platform (amd64+arm64) with provenance attestations and SBOM** via buildx, plus a **Helm chart publish workflow** (`helm-publish.yml`).
- Some steps still manual (version bump across `pyproject.toml` + gateway `status.rs`, tag + release notes by hand) — even strong teams under-automate here.
- Docs sync is a PR against a dedicated `docs` branch.

### Helicone AI Gateway (Rust, Apache-2.0, ~600 stars)
- Rust single-binary gateway; distributed via **Docker Hub (`helicone/ai-gateway`) AND npm (`npx @helicone/ai-gateway`)** — the npx path is a clever zero-install trial channel for a non-JS binary. SemVer tags + changelog + `RELEASE.md`.
- Main Helicone platform (TypeScript monorepo) self-hosts via docker-compose script (`helicone-compose.sh`) plus a separate **Helm chart repo (Helicone/HELM)**; they publicly blogged "How We Simplified Helicone's Self-Hosting in 30 Days" — self-host friction was bad enough to warrant a dedicated project.

### Kong (Lua/OpenResty on Nginx; Apache-2.0 OSS core)
- Builds with **Bazel** (since 3.1; retired the separate `kong-build-tools` repo) — hermetic, reproducible builds of the entire OpenResty stack.
- Distribution at enterprise grade: **official Docker Hub image** (`_/kong`), **Helm charts** (`kong/kong`, `kong/ingress` umbrella, `kong/gateway-operator`), and **versioned apt/yum repos hosted on Cloudsmith** with per-major repos (`gateway-36`, etc.) and one-line `setup.deb.sh`/`setup.rpm.sh` scripts.

### Tooling norms for a new single-binary project
- **Go:** GoReleaser is the standard — one YAML produces cross-platform archives, Homebrew tap formulas, Docker images, and Linux packages (deb/rpm/apk via embedded nFPM), plus signing/SBOM hooks; runs in GitHub Actions on tag push. Now also supports Rust and Zig.
- **Rust:** cargo-dist (now community-maintained "astral-sh/cargo-dist" lineage) generates GitHub Actions release workflows, shell/PowerShell installers, Homebrew formulas, and MSI from `Cargo.toml` metadata.
- Expected matrix of channels for a single-binary infra tool in 2026: `curl | sh` installer + Homebrew + Docker (multi-arch) + Helm + GitHub Releases binaries (+ optionally npm/PyPI wrappers as trial channels, per Helicone).

---

## 2. Versioning & Changelogs

- **Three live patterns:** strict weekly SemVer with tiered promotion (LiteLLM), CalVer monthly (TensorZero), classic SemVer with human changelog (Helicone gateway, Kong).
- **Changelog automation norms (JS/monorepo world):**
  - **Changesets** — contributor writes an intent file per PR; bot opens a "Version Packages" PR; best for monorepos and deliberate, human-written changelog entries; decouples versioning from commit-message discipline.
  - **release-please** (Google) — conventional-commits-driven; bot maintains a rolling Release PR; more automated, less intentional.
  - **semantic-release** — fully automatic from commit messages; risk of accidental majors from mislabeled commits.
- **Release notes as product:** LiteLLM publishes narrative per-release pages on its Docusaurus site (`docs.litellm.ai/release_notes/v1.84.0/...`) with themes ("Reliability hardening + multi-pod budget accuracy"), not just commit lists. Helicone runs a marketing-grade changelog page plus **weekly email digests (Mondays 10:00 UTC)** and Slack alert integration.
- Kong ships formal CHANGELOG per minor with deprecation windows; versioned doc sets per gateway major.

---

## 3. CI Matrices, Nightlies & Testing Against Live Providers

**TensorZero is the reference implementation** for AI-gateway testing ops. Its `.github/workflows/` inventory:

- `general.yml` (primary suite), `client-tests.yml`, `clickhouse-tests.yml`, `inference-cache-tests.yml`, `ui-tests.yml` + `ui-tests-e2e.yml` (Playwright), `db-only-boot-e2e.yml`, `k3d.yml` (Kubernetes cluster test).
- **Live-provider E2E:** `live-tests.yml`, `live-batch-tests.yml`, `live-tests-config-in-database.yml` — "the E2E tests involve every supported model provider, so you need every possible credential to run the entire test suite"; subsets via `cargo test-e2e xyz`. Credentials are env vars in CI secrets.
- **Scheduled/nightly:** `daily-tests.yml` (nightly run), `batch-completion-cron.yml`, `optimization-test-cron.yml` (perf/optimization crons).
- **Merge queue** with `cancel-merge-queue-on-job-failure.yml` — they gate merges on heavy suites without serializing PR review.
- **Mock provider:** in-repo `mock-provider-api` service (`TENSORZERO_INTERNAL_MOCK_PROVIDER_API=http://localhost:3030`) for batch/optimization workflows without burning live credits.
- Quality stack: `cargo-nextest`, `cargo-deny` (dependency/license scanning), pre-commit hooks, pyright + ruff for Python client, CodeQL + a dedicated `security.yml`.
- Tests can run against **two observability backends** (ClickHouse and Postgres) via env switch — matrix across storage backends, not just OS/arch.

**LiteLLM:** CI gates = Black, Ruff, MyPy, circular-import and import-safety checks (Google Python Style Guide); nightly tags auto-publish on green CI; RC perf testing was still "being implemented soon" as of the versioning blog — i.e., even at 50k stars the perf gate arrived late.

**VCR-cassette norm (record/replay):** the standard pattern for deterministic provider tests — record real HTTP interactions once (`vcrpy` / Ruby VCR), scrub auth headers (each provider passes keys differently), commit YAML cassettes, run CI in `none` record mode (replay only, fail on any request drift). Widely used for LLM SDK/integration tests to eliminate flake and cost; complemented (not replaced) by a small nightly live-provider suite that catches real upstream API drift.

**Implication for a new gateway:** three-layer test pyramid is the proven shape — (1) hermetic unit + cassette/mock-provider integration tests on every PR, (2) merge-queue/nightly live-provider E2E with real credentials across every supported provider, (3) scheduled load/perf benchmarks (LiteLLM's 12-hour pre-stable load test; TensorZero's published benchmark harness).

---

## 4. Docs Operations

- **LiteLLM:** Docusaurus 3, docs split into a **separate repo** (`BerriAI/litellm-docs`) serving docs.litellm.ai; release-notes blog co-hosted; publishes **`/llms.txt`** at the docs root.
- **Helicone:** Mintlify (docs.helicone.ai) — gets `/llms.txt` + `/llms-full.txt` auto-generated, plus Mintlify's **docs MCP server** (AI tools can search the docs directly) for free.
- **TensorZero:** docs on tensorzero.com/docs, synced from the monorepo via a `docs` branch + PR flow; includes a dedicated **Benchmarks page** and per-competitor comparison pages (`/docs/comparison/litellm`) — comparison pages as first-class docs is an SEO + evaluation-stage play worth copying.
- **Kong:** custom docs platform (developer.konghq.com) with versioned doc sets per gateway release and a Plugin Hub catalog.
- **2026 norm:** docs are an AI interface. Expected: `/llms.txt` (sitemap for LLMs), `/llms-full.txt` (entire docs in one file), markdown content negotiation, and a docs MCC/MCP server so coding agents query docs at generation time. Mintlify auto-provides all of this; Docusaurus needs plugins.

---

## 5. Community Operations

- **Chat:** Discord is table stakes (LiteLLM, Helicone, TensorZero all have one); TensorZero runs **both Slack and Discord**; LiteLLM added Slack for support; Kong skips chat in favor of **Kong Nation** (Discourse forum) + GitHub Discussions.
- **Contribution funnel:** `good-first-issue` labels (TensorZero explicitly curates them in CONTRIBUTING.md); pre-commit + make targets to lower setup friction; "small PRs welcome, large changes discuss first" norms.
- **Kong's mature layer:** a points-based **Contributor Program** (swag rewards), **Kong Champions** advocate tier, a **Plugin Hub** that accepts community plugins via PR — ecosystem-as-moat; community claims 160k+ members.
- **Helicone:** YC-brand badge in README ("YC W23"), public build-in-the-open blog posts (self-hosting journey) as community content.
- **LiteLLM:** founder emails in the README; enterprise Slack/support as the paid tier of community.

---

## 6. Security Policy & Posture

- **Minimum bar:** SECURITY.md / security@ email (TensorZero: security@tensorzero.com via CONTRIBUTING; notably no rich standalone SECURITY.md found — a gap even good projects have).
- **Supply-chain controls now expected after the LiteLLM incident:** cosign-signed images (LiteLLM), SBOM + provenance attestations on Docker builds (TensorZero), isolated build envs + ephemeral publish credentials (LiteLLM CI/CD v2), CodeQL + cargo-deny in CI (TensorZero).
- **The LiteLLM CVE record is the category's warning label:** CVE-2026-42208 (SQLi, CVSS 9.3, exploited within 36 hours of disclosure), CVE-2026-42271 (command injection, CVSS 8.7, in CISA KEV), chained with CVE-2026-48710 to unauthenticated RCE (combined CVSS 10.0) — attackers specifically target gateways because they hold **all provider keys**. A gateway is a credential vault; design and message accordingly (secret encryption at rest, no eval/exec config surfaces, authn on every admin route, fast CVE response SLA).
- OpenSSF coordinated-disclosure guide (ossf/oss-vulnerability-guide) is the template norm for SECURITY.md + maintainer runbooks.

---

## 7. Telemetry Norms

- Industry-consensus rules for OSS infra telemetry: **anonymous by design** (random local UUID, no email/machine fingerprint), **transparent docs page listing exactly what's sent**, multiple opt-outs including the cross-tool **`DO_NOT_TRACK=1`** env var (Netdata et al.) and a product-specific flag (`TELEMETRY_ENABLED=false` à la OpenLIT), never block functionality on telemetry.
- Common backends: PostHog (self-described ethical-OSS guidance; admits ~90% opt out), Scarf (download/install analytics without runtime phoning home — good compromise for a binary).
- Helicone gateway leans the other way: telemetry is **OpenTelemetry-native observability of itself** (logs/metrics/traces) — instrument the gateway for the operator first; vendor analytics second and optional.

---

## 8. Performance Publication Norms

- **TensorZero:** publishes a benchmarks docs page + claims **<1ms p99 gateway overhead at 10k+ QPS** on a 4-vCPU box, with a head-to-head showing LiteLLM degrading at hundreds of QPS and failing at 1k QPS. Benchmarks vs named competitors, reproducible setup. This single page does enormous competitive work.
- **LiteLLM:** own benchmarks page + a defensive engineering blog ("Your Middleware Could Be a Bottleneck", ~30% overhead reduction via FastAPI middleware work); known issues: ~2s→4.5s response inflation reports, 1.7–4x throughput reduction vs direct, 6-minute tail latencies under perf tests, DB-coupled slowdowns at 1M+ logs (issue #12067), 1,000+ open issues as a talking point used against them.
- **Helicone AI Gateway:** markets "fastest, lightest" (Rust); third-party roundups cite Bifrost ~11µs-class overhead — sub-millisecond Rust/Go gateways are now the bar; Python-proxy overhead is a structural liability competitors weaponize.

---

## 9. Agent Experience (AX) Observations

- `/llms.txt` + `/llms-full.txt` on the docs site is already table stakes (LiteLLM ships it; Mintlify auto-generates for Helicone).
- **Docs-as-MCP-server** is the emerging differentiator: Mintlify generates an MCP server from docs so Claude/Cursor/ChatGPT query them live during generation. None of the gateways yet ship a first-party **control-plane MCP server** (manage keys, routes, budgets via MCP) — that's open ground for an agent-first gateway.
- Machine-readable release surfaces matter to agents: predictable tag schemes (bare + v-prefixed), `version.json`-style latest endpoints, signed artifacts verifiable in script, narrative release notes at stable URLs.
- TensorZero's "every config is a TOML file + everything testable headlessly" and LiteLLM's config.yaml-first proxy are both API/file-first (agent-operable); dashboards are secondary. A new gateway should treat CLI/MCP as the primary admin surface and make the dashboard a client of the same API.

---

## 10. Operational Playbook Synthesis (what "good" looks like for the new gateway)

1. **Release ladder:** nightly (auto on green CI) → RC (1-week soak) → weekly stable; pre-stable load test (LiteLLM's 12h bar); CalVer if you promise no breaking majors, SemVer otherwise — but pick one and document it on a public Release Cycle page.
2. **One-tag fan-out:** goreleaser/cargo-dist on tag push → GitHub Release binaries (multi-arch incl. darwin/arm64), `curl | sh` installer, Homebrew tap, multi-arch Docker with cosign signature + SBOM + provenance, Helm chart (OCI registry), optional npm/pip wrapper for `npx`-style trial.
3. **Testing:** cassette/mock-provider integration tests on PR; merge queue for heavy suites; nightly live-provider E2E across all providers with real credentials; scheduled perf cron publishing to a public benchmarks page with named comparisons.
4. **Docs:** Mintlify-or-equivalent with llms.txt/llms-full.txt + docs MCP server day one; narrative weekly release notes; comparison pages per competitor.
5. **Security:** SECURITY.md + security@ + private GitHub advisories from day one; isolated release env, ephemeral publish creds, signed everything — and market it (post-LiteLLM-incident, supply-chain hygiene is a selling point).
6. **Telemetry:** anonymous, documented, `DO_NOT_TRACK` + own env flag, consider Scarf for install analytics instead of runtime phoning home.
7. **Community:** Discord + curated good-first-issues + pre-commit/make bootstrap; plan a plugin/extension registry early (Kong's Plugin Hub is the endgame moat).

---

## Sources (primary)
- https://docs.litellm.ai/docs/proxy/release_cycle ; https://docs.litellm.ai/blog/cleaner-release-versions ; https://docs.litellm.ai/release_notes/v1.83.0/v1-83-0 ; https://github.com/BerriAI/litellm ; https://docs.litellm.ai/llms.txt ; https://docs.litellm.ai/blog/fastapi-middleware-performance ; https://github.com/BerriAI/litellm/issues/21046
- https://github.com/tensorzero/tensorzero (CONTRIBUTING.md, RELEASE_GUIDE.md, .github/workflows) ; https://www.tensorzero.com/docs/gateway/benchmarks
- https://github.com/Helicone/helicone ; https://github.com/Helicone/ai-gateway ; https://github.com/Helicone/HELM ; https://www.helicone.ai/blog/self-hosting-journey ; https://www.helicone.ai/changelog
- https://github.com/Kong/kong ; https://github.com/Kong/charts ; https://cloudsmith.io/~kong/repos ; https://konghq.com/community/open-source-contribution ; https://developer.konghq.com/plugins/
- https://goreleaser.com ; https://github.com/changesets/changesets ; https://oleksiipopov.com/blog/npm-release-automation/
- https://vcrpy.readthedocs.io ; https://anaynayak.medium.com/eliminating-flaky-tests-using-vcr-tests-for-llms-a3feabf90bc5
- https://www.mintlify.com/docs/ai/llmstxt ; https://www.mintlify.com/blog/generate-mcp-servers-for-your-docs
- https://posthog.com/blog/open-source-telemetry-ethical ; https://learn.netdata.cloud/docs/netdata-agent/anonymous-telemetry-events
- https://thehackernews.com/2026/06/litellm-flaw-cve-2026-42271-exploited.html ; https://github.com/ossf/oss-vulnerability-guide
