# Agent Sandboxing & Egress Control Runtimes — Competitive Intelligence (Gap-Fill)

**Dimension report** for a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane). Subject: isolated execution runtimes for agent/tool code and their network egress policy surfaces — E2B, Daytona, Modal Sandboxes, Vercel Sandbox, Blaxel, plus DIY patterns (Firecracker, gVisor, Anthropic sandbox-runtime).

Research date: 2026-06-10.

---

## 1. Why this category is gateway-adjacent

The gateway's policy plane already answers "may this agent call this MCP tool / this model?" Sandboxes answer the sibling question: "may this agent's *generated code* reach this network destination?" Every vendor in this category has independently converged on the **same primitive the gateway needs**: allow/deny lists + an egress proxy that can log, filter, and transform outbound requests. The strategic finding is that **sandbox egress control is becoming a policy API, not just an infra setting** — Vercel and E2B both now let an external controller (which could be the gateway) update network policy on a *running* sandbox and forward traffic to a proxy the controller owns. That is precisely the integration point a gateway policy plane should claim.

A second convergence: **credential brokering**. Vercel (GA-ish, Pro/Enterprise) and E2B (private beta) inject secrets into outbound requests *at the proxy, outside the sandbox boundary*, so untrusted agent code can authenticate to APIs without ever holding the key. This is functionally identical to what an LLM/MCP gateway does when it holds provider keys and virtual keys. The gateway can be the brokering endpoint for all of these sandboxes.

---

## 2. E2B

**What it is:** "The Enterprise AI Agent Cloud" — on-demand Firecracker microVM sandboxes for running AI-generated code. The best-known name in the category (used by Manus, Perplexity, HF, etc.).

**OSS / language:** Open source, Apache-2.0. SDKs in TypeScript and Python; infra (e2b-dev/infra) is Go/Terraform/Nomad. Cloud-first; self-hosting is possible but heavy ("requires more than just your laptop — GCP, Nomad, Firecracker, Postgres" per the maintainers, in response to Simon Willison).

**Isolation:** Firecracker microVM per sandbox (own kernel, KVM hardware isolation). <200ms claimed start (~150ms typical in third-party benchmarks); snapshot pause/resume in 5–30ms.

**Feature surface:**
- Sandbox lifecycle: create/connect/kill, up to 24h runtime (Pro; Hobby caps at 1h), pause/resume persistence (filesystem + memory state).
- Templates: SDK-defined custom sandbox templates, built/versioned/cached ahead of time.
- In-sandbox API: filesystem ops, `commands.run()` (PTY/streaming), public URLs for ingress.
- Desktop Sandbox: virtual Linux desktops for computer-use agents.
- MCP: docs include an MCP gateway that runs *inside* sandboxes with type-safe access to 200+ tools from the Docker MCP Catalog or custom MCPs — i.e., E2B positions the sandbox as the place where MCP tools run.
- Fragments: open-source Next.js template for Claude-Artifacts-style apps.
- Dashboard: usage/cost monitoring, team members, API keys (Next.js + Supabase).
- GitHub Actions integration; CPU-only on managed cloud (GPU requires self-host OSS on bare metal).

**Egress control (IMPORTANT CORRECTION to the brief):** The "E2B has no egress policies" claim is **outdated as of ~Nov 2025**. Current state:
- `allowInternetAccess` boolean (default on).
- `network.allowOut` / `network.denyOut` lists supporting IPs, CIDRs, and **domains incl. wildcards** (`*.mydomain.com`); allow wins over deny.
- Domain filtering works only for HTTP:80 (Host-header inspection) and TLS:443 (SNI inspection).
- `updateNetwork()` — **dynamic policy update on a running sandbox**.
- `network.rules` transforms (private beta): per-host HTTP header injection at the egress proxy = credential/secret brokering (tracked in GitHub issue #1160).
- Known wart: blocked TCP connections still *appear* to succeed from inside the sandbox (firewall drops packets after handshake-looking behavior); you must verify at application level.

**Pricing:** Usage-based per-second, ~$0.05/vCPU-hour ($0.0504), plus flat plan fees (Hobby free / Pro). Third-party benchmark puts a standard config at ~$0.0828/hr.

**Complaints:** "Way too expensive at scale" (HN); concurrency limits; API timeouts and confusing lifecycle behavior; pause/resume reconnect bugs (404/409 on resume, GH #899, state not persisting #884); no standard Docker image workflow; self-hosting complexity; 1h/24h runtime ceilings.

---

## 3. Daytona

**What it is:** "Secure and elastic infrastructure for running AI-generated code." Pivoted from dev-environment manager to agent sandbox runtime in 2025; positions on speed + stateful, long-lived agent workspaces.

**OSS / language:** AGPL-3.0 (relicensed from Apache-2.0 during the agent pivot — deliberate commercial moat). TypeScript + Go + Python + Ruby/Java monorepo. Self-hostable via Docker Compose; hybrid mode with customer-managed runners.

**Isolation:** Container-based by default (Docker/OCI) for speed, with optional **Kata Containers** (microVM-class) or **Sysbox** (rootless) runtimes when stronger isolation is needed. Warm pools from default snapshots.

**Performance claims:** Sub-90ms cold start marketing number; optimized configs quoted at 27ms; ~90ms in third-party benchmark (fastest of the big four; Blaxel claims 25ms).

**Feature surface:**
- SDKs: Python, TypeScript, Ruby, Go, Java; REST API + per-sandbox Toolbox API (daemon inside the sandbox: fs, git, process/code exec, computer use, log streaming, terminal sessions, LSP).
- Snapshots (OCI-image-like, persistent state), Volumes, unlimited persistence, unlimited runtime.
- Declarative image builder; preview URLs + custom preview proxy; SSH, web terminal, VNC.
- **MCP server** (official): Claude/Cursor/Windsurf agents drive sandboxes directly via MCP.
- Webhooks for lifecycle events; OpenTelemetry collection; log streaming.
- Enterprise: organizations, API keys, audit logs, billing mgmt; multi-region (managed cloud effectively us-east-1 today).

**Egress control:**
- Default: tier-based. Tier 1–2 orgs get a restricted-network sandbox that **cannot be overridden**; "essential services" (package registries, git hosts, CDNs, AI/ML platform APIs) whitelisted on all tiers; Tier 3–4 get full internet + custom config.
- `networkBlockAll` (bool) and `networkAllowList` — **IPv4 CIDR only, max 10 entries, no domains/hostnames/IPv6**. blockAll wins over allowList.
- Dynamic updates on running sandboxes (Tier 3/4 only). Enforced as iptables rules on the Runner.
- Open feature request for richer dynamic egress (GH #3357) — the firewall is the weakest of the four (no domain semantics, no transforms, no brokering).

**Pricing:** $0.0504/vCPU-hour (matches E2B exactly — price-floor signal); Hobby = one-time $100 credit + 20 concurrent sandboxes; Pro $150/mo lifts session length to 24h.

**Complaints:** SDK/lifecycle bugs at volume (hundreds of GH issues), workspace-creation/Git-clone failures, single-region managed cloud, 15-min auto-pause default judged too long, Docker default isolation not microVM-grade for hostile code, tunneling less refined than older platforms.

---

## 4. Modal Sandboxes

**What it is:** Sandbox primitive inside Modal's serverless GPU compute platform. The only one of the four where a sandbox can hold a **GPU**, and the most complete egress policy surface.

**OSS / language:** Proprietary platform; client SDK (Python, plus JS/Go clients) is open (modal-client, Apache-2.0); Rust-heavy infra. Not self-hostable.

**Isolation:** gVisor (user-space syscall interception) — strong but not hardware virtualization; same runtime family Anthropic uses for Claude remote execution. Sub-second starts; Modal's own 2026 engineering blog admits 300–800ms overhead is "problematic for interactive agent sessions."

**Feature surface:**
- `Sandbox.create()` with full Modal Function config: custom images (chained builder or registry), Volumes, Secrets as env, GPUs, regions.
- `sandbox.exec()` → ContainerProcess with stdin/stdout/stderr streaming and exit codes; entrypoints for long-running services; readiness probes (TCP + exec).
- Lifecycle: default 5-min timeout, configurable to 24h; idle timeouts; named sandboxes (unique per app), tags + `Sandbox.list()` filtering.
- Snapshots: memory snapshots (experimental, expire 7 days, **incompatible with GPUs**) and filesystem snapshots for >24h continuity.
- Docker-in-sandbox (alpha). Tunnels for ingress with encrypted/unencrypted/h2 ports, custom domains (Enterprise), connect tokens for authenticated HTTP/WebSocket ingress.
- Dashboard: Modal's standard observability (logs, metrics, cost per workspace).

**Egress control (best-in-class of the hosted four alongside Vercel):**
- Secure-by-default for ingress/resources, but **default outbound is unrestricted**.
- `block_network=True` — drop all egress.
- `outbound_cidr_allowlist` — CIDR-scoped egress, any protocol.
- `outbound_domain_allowlist` (beta) — TLS:443 domain allowlist with wildcards; **violations are blocked AND logged to the sandbox's output stream** (audit hook).
- `inbound_cidr_allowlist` for tunnel/connect-token ingress.
- CIDR + domain lists combine additively.

**Pricing:** Premium: ~$0.00003942 per physical core-second (a core = 2 vCPU), ~3x its standard function rate; ~$0.1193/hr benchmark config (~2.4x E2B/Daytona). $30/mo free credits. You pay for GPU capability and platform breadth.

**Complaints:** Cost premium; cold-start overhead for interactive agents (their own admission); Python-first ergonomics; snapshots experimental and GPU-incompatible; no self-host; egress controls not on by default.

---

## 5. Vercel Sandbox

**What it is:** Compute primitive for untrusted/AI-generated code on Vercel — Firecracker microVMs (Amazon Linux 2023), tightly integrated with Vercel's platform, AI Gateway, and agent workflows (used by Notion Workers at scale).

**OSS / language:** Proprietary service; SDK + CLI open on GitHub (vercel/sandbox). JS/TS SDK (`@vercel/sandbox`), Python SDK, and a `sandbox` CLI explicitly pitched for "manual testing, **agentic workflows**, debugging."

**Isolation:** Firecracker microVM per sandbox, own filesystem and network; root via sudo; can run **system-privileged processes: Docker, VPN clients, FUSE** — unusual for hosted sandboxes.

**Feature surface:**
- Runtimes: node26/24/22, python3.13; install anything.
- Lifecycle: default 5-min timeout, extendable (`extendTimeout`), max 45min Hobby / 5h Pro+Enterprise (the shortest ceiling of the four).
- Persistence: **persistent sandboxes by default** (auto-save on stop, resume in place), explicit snapshots (30-day default expiry), Drives (beta) for attachable persistent storage, tags.
- Auth: OIDC tokens (project-scoped, automatic in Vercel) or access tokens.
- Observability: logs, file edits, live previews; usage dashboard.

**Egress control — the reference design for a gateway integration:**
- Three runtime-switchable modes: `allow-all` (default), `deny-all` (kills everything incl. DNS), and **user-defined** (deny-by-default + allowed domains w/ wildcards + allowed CIDRs + denied CIDRs taking precedence).
- Domain matching via SNI; explicit Postgres-over-TLS support (firewall handles the Postgres STARTTLS-style handshake before applying domain policy).
- **Credentials brokering** (Pro/Enterprise): proxy outside the sandbox injects headers on matching egress; injected headers overwrite sandbox-set ones; secrets never enter the sandbox.
- **Requests proxying**: forward traffic for chosen domains to *a proxy you control* (`forwardURL`), with `vercel-forwarded-*` headers and a **sandbox-identity OIDC token** (team_id, project_id, sandbox_id, sandbox_name) so your proxy can authenticate/attribute every request; `defineSandboxProxy` helper validates it.
- **Matchers**: per-rule path/method/query/header matching (exact, startsWith, RE2 regex), first-match-wins ordering.
- TLS termination only for domains with transformation rules, via per-sandbox CA auto-trusted in the VM.
- Policies set at creation AND **live-updated on running sandboxes** ("start open to install deps, then lock down before running untrusted code" is a documented pattern).

**Pricing:** Active-CPU billing ($0.128/active-CPU-hr — I/O wait is free), $0.0212/GB-hr memory, $0.60/M creations, $0.15/GB network, $0.08/GB-mo snapshot storage. Hobby free tier (5 CPU-hr, 5,000 creations, 10 concurrent). Pro/Ent: 2,000 concurrent, 32 vCPU/64GB max, 32GB NVMe. Single region (iad1).

**Complaints/limits:** 5-hour max runtime (worst of the four for long agents); single region; node/python only as first-class runtimes; brokering/proxying paywalled to Pro+; platform lock-in to Vercel auth/billing; nominal $/hr highest in benchmark ($0.1492) though active-CPU billing often makes real cost lower.

---

## 6. Blaxel (brief)

"Perpetual sandbox platform": microVM sandboxes that suspend/resume like a laptop lid — **~25ms resume**, claimed fastest in category; persistent state forever; pay only while running. Notable agent-experience features: a **built-in MCP server in every sandbox** (remote tool calls = first-class agent control path), filesystem watch events, and co-located hosting for agents + MCP servers + model APIs to kill network round-trips. Priced at the same $0.0828/hr benchmark floor. Smaller/less proven than the four majors.

---

## 7. DIY patterns & adjacent OSS

- **Anthropic sandbox-runtime (`srt`, anthropic-experimental, OSS):** OS-level sandboxing without containers — sandbox-exec (macOS) / bubblewrap (Linux) for filesystem + **proxy-based network filtering**: built-in HTTP and SOCKS5 proxies enforcing domain allow/deny lists, **explicitly pluggable: "configure the sandbox to use your own proxy"**. Used by Claude Code's sandboxed Bash tool; sandboxes agents, local MCP servers, bash, arbitrary processes. Already had two published network-sandbox bypasses (e.g., SOCKS5 hostname null-byte injection) — evidence that proxy-based egress filtering is hard to get right and that audit/defense-in-depth matters.
- **Firecracker DIY:** AWS's microVM (Rust, Apache-2.0); what E2B/Vercel build on. Snapshot/restore in ms; you own the orchestration, networking (tap devices + nftables/iptables egress), and image pipeline.
- **gVisor DIY:** Google's user-space kernel (Go, Apache-2.0), runsc as containerd runtime class; what Modal and Claude's remote execution use; easier on stock Kubernetes than Firecracker.
- **microsandbox** (Apache-2.0): lightweight self-hosted microVM sandboxing, popular E2B alternative for laptop-scale use.
- **Cloudflare Sandboxes / Beam / Sprites.dev / Northflank:** second tier; Cloudflare criticized on HN for container pricing; 2–3s cold starts for Cloudflare/Beam in benchmarks.
- **agentsh / agentsh-modal:** emerging OSS shim putting per-command policy + egress mediation around agent shells — same policy-plane instinct, evidence of demand.

---

## 8. Cross-cutting comparison

| | E2B | Daytona | Modal | Vercel |
|---|---|---|---|---|
| Isolation | Firecracker microVM | Containers (opt. Kata/Sysbox) | gVisor | Firecracker microVM |
| Cold start (3rd-party) | ~150ms | ~90ms | sub-second (300–800ms) | "ms" (fast, unspecified) |
| Max runtime | 24h (+pause/resume) | Unlimited | 24h (+FS snapshots) | 5h |
| GPU | Self-host only | No | **Yes** | No |
| Egress: block-all | Yes | Yes | Yes | Yes (deny-all incl. DNS) |
| Egress: CIDR allowlist | Yes | Yes (IPv4 only, max 10) | Yes | Yes (+CIDR denylist) |
| Egress: domain rules | Yes (SNI/Host, wildcards) | **No** | Yes (beta, TLS-only, wildcards) | Yes (SNI, wildcards, +Postgres) |
| Live policy update | Yes (`updateNetwork`) | Tier 3/4 only | Not documented | Yes |
| Credential brokering | Private beta | No | No | Yes (Pro+, matchers) |
| Forward to your proxy | No (header transforms only) | No | No | **Yes (forwardURL + OIDC identity)** |
| Egress audit/logging | Weak (TCP false-success wart) | Not documented | Blocked-domain events logged | Via your own proxy |
| MCP control path | MCP gateway in sandbox | Official MCP server | No first-class MCP | CLI pitched for agents; KB guides |
| OSS | Apache-2.0 (heavy self-host) | AGPL-3.0 (Compose self-host) | Closed (OSS client) | Closed (OSS SDK/CLI) |
| Price (benchmark hr) | $0.0828 | $0.0828 | $0.1193 | $0.1492 nominal (active-CPU billing) |

**Market signals:** Price floor at $0.0504/vCPU-hr (E2B = Daytona exactly) → commoditizing compute; differentiation has moved to (1) state/persistence ergonomics, (2) egress/credential security, (3) agent-native control surfaces (MCP servers, CLIs, in-sandbox toolbox daemons).

---

## 9. Implications for the gateway

1. **Yes — the policy plane should govern agent egress with the same allow/deny/audit semantics as MCP tool calls.** Every serious runtime grew an egress policy API; none of them unify it with LLM/tool-call policy. A single policy document covering {model calls, MCP tool calls, sandbox egress} with one audit log is genuinely unclaimed territory.
2. **Concrete integration points that exist today:**
   - Vercel `forwardURL` request proxying → point it at the gateway; gateway gets full request + signed sandbox identity (OIDC) and can apply allow/deny/transform/log. This is the cleanest "gateway as egress policy enforcement point" hook in the market.
   - Anthropic srt's "bring your own proxy" → gateway ships an HTTP/SOCKS5 egress-proxy endpoint and instantly governs local Claude-Code-style agents.
   - E2B `updateNetwork()` / Daytona network-update API / Vercel live policies → gateway can *push* compiled policy (CIDR/domain lists) into sandboxes at session start and tighten mid-session ("install deps open, lock before untrusted run" pattern).
   - Modal's blocked-domain log events → ingestible audit signal.
3. **Credential brokering belongs in the gateway.** Vercel and E2B both rebuilt what an LLM gateway already is (a key-holding proxy). The gateway should be the brokering endpoint: sandbox egress for `api.openai.com`/MCP backends routes through the gateway, which injects virtual-key-resolved credentials and meters cost — unifying spend tracking across direct LLM calls and sandboxed agent calls.
4. **Lowest-common-denominator policy compiler:** domains+wildcards (Vercel/E2B/Modal/srt) and IPv4 CIDRs (everyone, Daytona max 10) — the gateway policy schema should compile down to each runtime's dialect and warn on lossy compilation (e.g., domain rules → Daytona unsupported).
5. **Audit honesty matters:** E2B's TCP false-success wart and srt's two published bypasses show enforcement claims need verification; a gateway that *observes* egress (proxy mode) rather than trusting sandbox-side filters has a defensible trust story.
6. **Agent-experience bar:** official MCP servers (Daytona, Blaxel, E2B's in-sandbox MCP gateway), CLIs documented for agent use (Vercel), llms.txt docs (E2B). The gateway's sandbox-policy surface should be MCP-first and CLI-first from day one.

---

## 10. Sources (primary)

- modal.com/docs/guide/sandbox-networking, /sandboxes, /sandbox-snapshots
- e2b.dev/docs, e2b.dev/docs/sandbox/internet-access, github.com/e2b-dev/infra, github.com/e2b-dev/E2B issues #1160/#899/#884
- daytona.io/docs/en/network-limits, github.com/daytonaio/daytona (+ issue #3357), daytona.io/docs/en/mcp
- vercel.com/docs/sandbox, /docs/sandbox/pricing, /docs/sandbox/concepts/firewall, changelogs (credentials injection; firewall proxying/filtering), Notion Workers case study
- blaxel.ai/sandbox, docs.blaxel.ai/Overview
- github.com/anthropic-experimental/sandbox-runtime; anthropic.com/engineering/claude-code-sandboxing; published bypass write-ups (penligent.ai, oddguan.com)
- superagent.sh/blog/ai-code-sandbox-benchmark-2026; northflank.com comparison series; zenml.io/blog/e2b-vs-daytona; pixeljets.com daytona-vs-microsandbox; HN threads on E2B pricing/reliability and Cloudflare container pricing
