# Commercial MCP Gateway / Security Startups — Competitive Intelligence Report

**Subject:** Runlayer, MintMCP, Natoma, Operant AI MCP Gateway, Airia, MCP Manager (by Usercentrics)
**Date:** 2026-06-10
**Context:** Gap-fill research on the funded 2025–26 commercial MCP gateway wave, complementing the existing OSS/incumbent MCP gateway coverage. Audience: team building a new open-source AI gateway (unified LLM gateway + MCP gateway, single binary, dashboard, agent-first CLI/MCP control plane).

---

## Executive Summary

The 2025–26 commercial MCP gateway wave is **security-led, closed-source, and SaaS-first**. Every vendor in this set converges on the same core bundle: a **private/curated MCP registry with approval workflows**, **SSO/SCIM identity federation**, **per-tool-call audit logging (often SIEM-exportable)**, **role/attribute-based access control down to the tool level**, and **MCP-specific threat detection** (tool poisoning, tool shadowing, rug pulls, prompt injection, fake/lookalike MCP servers). Validation that this is a real market: Runlayer raised $11M from Khosla (Keith Rabois) + Felicis at stealth-exit in Nov 2025; **Snowflake signed a definitive agreement to acquire Natoma in May 2026**; Operant's gateway was featured in Gartner's MCP cybersecurity guide.

Key structural facts relevant to an OSS competitor:

1. **None of these six are open source.** Their detector stacks are closed and unversioned; the obot.ai comparison explicitly calls out that customers "cannot pin or inspect detector versions" — openness is a live competitive axis they all fail.
2. Almost all are **demo-gated**: public docs are thin (MintMCP is the partial exception), evaluation requires sales engagement. An OSS gateway that can be evaluated by `curl | sh` in 10 minutes attacks all six simultaneously.
3. They are converging from two directions: **security companies adding a gateway** (Operant, Runlayer, MCP Manager) and **agent platforms adding MCP governance** (Airia, MintMCP, Natoma). Nobody in this set unifies LLM gateway + MCP gateway; only MintMCP even mentions an "LLM proxy" in docs.
4. The **agent-facing surface is weak everywhere**: humans configure via dashboards; agents are subjects of policy, not operators of the gateway. Runlayer's Cursor Hooks partnership and MintMCP's Agent Monitor hook scripts are the closest things to agent-native integration. No vendor exposes an MCP control plane for managing the gateway itself.

---

## 1. Runlayer

**Positioning:** "Enterprise MCPs, Skills, & Agents" — all-in-one MCP security gateway. Zero-trust security standards for MCPs, skills, and agents.
**Funding/Status:** Exited stealth Nov 17, 2025 with **$11M seed from Khosla Ventures (Keith Rabois) and Felicis**, plus angels from 8 unicorns. Founders previously built Opendoor-adjacent infra (Rabois connection).
**Open source:** No. Closed-source detector stack, demo-gated docs.
**Implementation language:** Not public.

### Feature surface

**Threat detection (the headline differentiator)**
- Real-time threat-detection models for MCP-specific, skill-specific, and agentic attacks.
- Static + dynamic scans for: prompt injection, command injection, **tool poisoning**, **tool shadowing**, fake/lookalike MCP servers.
- Multi-tier detection system; claims "no noticeable performance impact" (no published numbers).
- Every MCP release auto-scanned for vulnerabilities, data leaks, and **permission drift** before approval.
- Real-time risk scoring per MCP request.

**Identity & access (framed as the core problem)**
- Native SSO (Okta, Entra, "all major IdPs"), SCIM, group sync.
- Conditional access + device-compliance checks reusing existing IdP posture.
- **ABAC across six dimensions: user, device, client, server, session, request.**
- Blocks developers from using personal API keys outside the identity system.
- 1Password integration for machine identities/credentials.

**Registry & catalog**
- Private registry for MCP servers + skills + agents with human-in-the-loop approval (scan results + risk scores presented to approver).
- Catalog of **18,000+ MCP servers, skills, plugins, agents**; one-click access to pre-approved entries; "approval in minutes not weeks."

**Observability**
- Raw request/response logging across MCPs, skills, agents; complete audit trails for GRC/incident response.
- Custom tags + "dynamic knowledge graphs" for compliance tracking.

**Deployment**
- Self-hosted in customer VPC **or** Runlayer cloud, "zero data egress" claim; "10-minute deployment."
- Local MCP & skills support **with CLI tools** (the one agent/dev-facing CLI mention in this cohort).

**Client coverage:** claims **300+ AI clients** (Cursor, VS Code, Claude Code, GitHub Copilot, ChatGPT, Claude Desktop, Windsurf).

**Agent-facing (AX) notes**
- **Official Cursor Hooks launch partner**: allow/deny MCP tool calls from inside Cursor via hooks — policy enforcement reaches into the IDE-agent loop, not just the network path.
- Proprietary "remix" and "subagent" constructs let IT compose custom automations above the MCP spec — powerful but **non-portable / lock-in** (third-party criticism).
- Skills are first-class governed artifacts alongside MCP servers — they're ahead of the cohort in treating the Claude-style skill/agent ecosystem as a governed surface.

**Compliance:** SOC 2, HIPAA, GDPR certified.
**Pricing:** Not disclosed; sales-led.

### Weaknesses (third-party + observed)
- Closed-source, unversioned, un-benchmarkable detector stack.
- Heavy marketing, thin public docs; product evaluation requires sales engagement.
- "Remix"/"subagent" artifacts are Runlayer-proprietary, not portable MCP servers (lock-in).
- No published performance or pricing data.

---

## 2. MintMCP

**Positioning:** "Enterprise Gateway for AI Agents" — MCP gateway + agent governance, optimized for the **AI coding-assistant** use case (Cursor/Claude Code/Copilot). Public Cursor partnership.
**Open source:** No. SaaS-first; on-prem via enterprise contact.
**Implementation language:** Not public.

### Feature surface

**Gateway core / signature capability**
- **STDIO-to-remote transformation**: supply a standard STDIO MCP server config; MintMCP runs it in managed infrastructure (container lifecycle, scaling, monitoring) and exposes it as a remote, OAuth-protected, logged, compliant service. This is their wedge.
- **Virtual MCP servers (VMCPs)**: admin-curated bundles of connectors/tools per role (e.g. a "Sales Intelligence" VMCP); users connect from ChatGPT/Claude, auth once, and the gateway enforces which connectors — even which CRM records — they can reach.
- Documented request path: client → MintMCP validates identity → evaluates org policies → returns **curated tool manifest** for that VMCP → on tool call, maps to connector config, **injects credentials**, forwards downstream → relays response while recording telemetry and enforcing **post-call policies**.
- MCP store: browsable approved-server catalog, single-click connect, pre-configured credentials. Claims 1,000+ pre-built connectors (one comparison cites a "10,000+ server catalog" claim).
- Custom hosted connectors: own databases (PostgreSQL, MongoDB), internal APIs, custom tools; data warehouses (Snowflake, BigQuery, Databricks), comms (Slack, Teams), docs (Drive, SharePoint, Confluence).

**Agent Monitor (second product)**
- Real-time visibility into coding-agent activity via **hook scripts capturing file reads, command execution, and MCP calls**.
- Behavioral rule creation/enforcement; risky-action detection and blocking.

**LLM proxy**
- Docs list an "LLM proxy" section — the only vendor in this cohort gesturing at unified LLM+MCP gateway scope. Depth unknown.

**Auth/governance:** SSO; role-based tool sets; gateway-layer access-policy enforcement; centralized credential management; user **and agent** identities.
**Logging/compliance:** every tool call logged; SOC 2 Type II; HIPAA-aligned with BAA; pen-tested; encryption in transit/at rest; data residency options; uptime SLA; **OTEL export (Enterprise tier)**.

**Pricing (publicly published — rare in this cohort)**
- **Teams: $1,250/month for 50 seats; +$25/seat/month after** — includes role-based MCP bundles, user+agent identities, custom hosted connectors, agent observability, 30-day audit-log retention.
- **Enterprise: custom (100+ seats)** — adds SSO/SAML, SCIM, configurable audit logs, OTEL export, SLAs, CSM.

### Weaknesses
- Managed-SaaS by default; on-prem only via vendor contact.
- Closed-source, unversioned detector stack.
- Catalog vetting methodology undocumented.
- Scope skewed to coding-assistant patterns; broader agentic deployments "may find it undersized" (obot.ai).
- 30-day audit retention on Teams tier is thin for compliance buyers.

### AX notes
- Best-documented architecture in the cohort (public docs: intro, quickstart, gateway architecture, Agent Monitor, security, LLM proxy).
- Agent identities are first-class alongside user identities.
- Hook-script-based agent monitoring is filesystem/command-level, not just MCP-level — closest analog to an agent-runtime sensor.
- Still dashboard-driven config; no public API/CLI/policy-as-code surface documented.

---

## 3. Natoma  — **being acquired by Snowflake (definitive agreement, May 2026)**

**Positioning:** "AI Agent Enablement for the Enterprise" — MCP gateway with the strongest **shadow-AI discovery** story and **desktop MCP management**. Founded 2024 (identity-security pedigree).
**Open source:** No.
**Implementation language:** Not public.

### Feature surface

**Gateway & connection management**
- Verified MCP server library (claims **1,000+ MCP servers** supported); custom MCP server support across cloud, desktop, self-hosted.
- **Desktop MCP servers via stdio** — manages local file-access/dev-tool servers on user machines (unique emphasis in cohort).
- **Single managed endpoint / centralized configuration endpoint** for all enabled connections across AI clients — kills config drift.
- Clients: ChatGPT, Claude, Cursor, internal agents.

**Shadow AI discovery (the differentiator)**
- Finds and inventories all MCP connections active across the org, including unsanctioned ones; **claims an average of 225 unmanaged shadow-AI instances detected per enterprise**.
- Block-or-govern workflow for discovered instances.

**Authorization**
- **ABAC using the Cedar policy language** (the only vendor naming a real, open policy engine — closest thing to policy-as-code in this cohort).
- Identity-aware controls tied to user, group, device context; OAuth 2.1; SSO/SAML/SCIM.
- Managed credentials **or BYO credentials**.
- **Role-based Profiles** and **intent-based Profiles** (context-aware tool distribution).
- Data-filter controls inspecting/blocking tool-call inputs and outputs by content pattern (DLP); blocks lateral movement, privilege escalation, unauthorized data modification.

**Audit/SIEM**
- Tamper-evident, structured logs per tool call: user identity, AI client, tool name, inputs, outputs, outcome.
- SIEM forwarding: Splunk, Microsoft Sentinel, CrowdStrike Falcon, others; EDR/MDM/IAM stack integration.
- Activity dashboard: usage trends, anomalies, policy violations.

**Deployment:** cloud, VPC, on-prem (data residency), hybrid; proxy support for constrained networks.
**Compliance:** SOC 2, GDPR, CCPA.
**Performance claims:** **1.8M tool calls/day processing capacity; 99.9% uptime** (published — rare).
**Pricing:** Not public.

### Weaknesses
- Acquired by Snowflake → likely to be folded into Snowflake's agentic stack; independent roadmap and non-Snowflake-aligned customers at risk (opportunity for everyone else).
- No public pricing; no public API/CLI docs.
- Closed source despite using open Cedar underneath.

### AX notes
- Cedar-based ABAC is the most agent-legible/automatable policy layer in the cohort.
- Discovery agent on desktops implies an endpoint component, but no agent-operable API surface documented.

---

## 4. Operant AI — MCP Gateway (part of AI Gatekeeper platform)

**Positioning:** Runtime AI security company; MCP Gateway is an expansion of the **AI Gatekeeper** platform — "runtime firewall for LLMs and agents." Featured in Gartner's MCP cybersecurity guide. Coined/markets the "Shadow Escape" zero-click MCP exploit.
**Open source:** No (company is a CNCF + OWASP + Coalition for Secure AI member, but product is closed).
**Implementation language:** Not public; Kubernetes-native deployment suggests Go-ecosystem tooling.

### Feature surface — organized as Discovery / Detections / Defense

**MCP Discovery**
- Auto-catalog MCP tools and discover AI agents in real time across environments — local dev tools (GitHub Copilot, Claude Desktop) through Kubernetes, AWS Bedrock, Azure, Google Vertex AI.
- Live traffic graphs, metrics, telemetry of access patterns; shadow MCP detection (clients, servers, tools).

**MCP Detections**
- Prompt injection, jailbreaks, tool poisoning, unauthorized access patterns, context-aware data tampering, sensitive-data leakage between agents and tools, supply-chain vulnerabilities.
- "Shadow Escape" zero-click exploit detection.
- OWASP LLM Top-10 threat-vector mappings.

**MCP Defense**
- **Trust zones** with real-time blocking of untrusted servers/tools; trust-score mapping and trust-boundary enforcement.
- **Inline auto-redaction** and flow-blocking for sensitive data/IP.
- Least-privilege execution, fine-grained tool permissions, rate limiting, encryption of MCP communications.
- **AI Non-Human Identity (NHI)** coverage; lateral-movement blocking; blast-radius reduction for compromised agents.
- AI-DR (Detection & Response) for live cloud + AI workloads; AI Security Graphs.

**Deployment:** Kubernetes-native + endpoint protection; "works in minutes, scales as you need" (no hard numbers).
**Compliance:** SOC 2 Type II.
**Pricing:** Not public.

### Weaknesses
- Explicitly an **additive security layer, not a full gateway replacement** — no registry/catalog/curation story, weaker on developer enablement.
- Deployment model (SaaS vs self-hosted) not documented publicly; closed detection engine, no versioning transparency.
- Doesn't replace API security/WAF/SIEM (their own framing) — narrow lane.
- No public docs depth; sales-led.

### AX notes
- Built for SecOps humans (dashboards, graphs, SIEM), not for agents; no CLI/API/MCP control surface documented.

---

## 5. Airia

**Positioning:** Full enterprise **agent orchestration platform** that added a Secure MCP Gateway (launched Sep 25, 2025). Claims "largest enterprise-ready MCP catalog" — **surpassed 1,000 pre-configured integrations Feb 2026**. Also sells via Azure Marketplace.
**Open source:** No.
**Implementation language:** Not public.

### Feature surface

**MCP Gateway**
- Execution layer giving agents governed real-time access to 1,000+ pre-configured integrations (SaaS, internal tools, databases, CRMs, APIs): GitHub, Atlassian, Slack, Microsoft, Twilio, Stripe, HubSpot, MongoDB, Notion, etc.
- Zero-trust architecture: proxy security, granular permissions, intelligent filtering, audit logging.
- Runtime policy enforcement: "every tool invocation through MCP operates within defined enterprise policies"; **agent constraints** (governed action boundaries).
- Role-based access controls per agent → systems/data.

**Platform context (the real product)**
- Agent orchestration: multi-step workflows across departments, cross-system process execution.
- Runtime monitoring of every agent action; governance dashboard; compliance reporting; AI security-posture management.
- Integration Framework with OOTB connectors + MCP support; works with existing stack, no architecture rebuild.

**Pricing (published, self-serve — unusual)**
- Free: $0, 1 user, 100 agent executions/mo, 10 agents.
- Individual: $50/mo, 1 user, 1,000 executions.
- Team: $250/mo, unlimited users, 10,000 executions.
- Enterprise: custom — SSO/SAML, audit logs, dedicated support.
- Note: pricing is **per agent execution**, not per seat — an orchestration-platform pricing model applied to gateway access.

### Weaknesses
- MCP gateway is a feature of a much larger platform — buying it means buying Airia's orchestration worldview (lock-in; heavyweight for teams that just want a gateway).
- Not covered in independent gateway comparisons (obot.ai omits it) — weak mindshare in the gateway-qua-gateway market.
- No public API/SDK/deployment-model detail on the MCP pages; no performance numbers; security depth (threat detection) is asserted, not specified — no named MCP-attack detections like Runlayer/Operant.

### AX notes
- Agents are the platform's first-class citizens (it's an agent-builder), but gateway control is dashboard/governance-team-driven; no agent-operable config surface documented.

---

## 6. MCP Manager (by Usercentrics)

**Positioning:** "The safety net for AI agents" — MCP gateway with security, deployment, and observability, from the consent-management company Usercentrics (compliance DNA). Aggressive SEO content engine ("best MCP gateway for X" posts).
**Open source:** No.
**Implementation language:** Not public; PII detection built on **Microsoft Presidio** (OSS, Python) per third-party analysis.

### Feature surface

**Gateway & registry**
- Single place to provision, control, monitor every MCP across the org; ends manual Docker-container management and hardcoded credentials.
- Private MCP registry with approval workflows; per-team server registries with cross-team sharing; approval workflows for access requests.
- Tool/server assignment to specific teams.

**Security**
- Rug-pull attack prevention; **alerts when tool descriptions change** or behavior is abnormal (anti-mimicry); data-exfiltration and over-privileged-agent prevention.
- Content + **PII filtering (Presidio NLP-based, not regex)** before data reaches models.
- OAuth enforcement incl. **Dynamic Client Registration**.

**Identity:** RBAC for users **and agents**; SSO (Okta, Azure AD, Google Workspace) — auth once, gateway handles downstream server auth; SCIM provisioning; individual identity enforcement instead of shared credentials (though shared credentials are also supported — criticized).

**Observability:** every tool call logged per user/agent/server; audit logs exportable to **Splunk and Datadog**; token + usage charts; metadata-flow insight; real-time dashboards by team/user/server/tool; OTEL export (Business tier).

**Pricing:** **Free to start**; Business/Enterprise tiers add SSO, SCIM, advanced PII filtering, OTEL export, dedicated support (exact prices not public).

### Weaknesses (third-party, obot.ai)
- Closed-source detectors; can't pin versions or benchmark false positives.
- **Shared credentials supported as a first-class feature** — degrades audit clarity.
- **Policy configuration is UI-only — no GitOps/policy-as-code workflow documented.**
- No curated starting catalog — customers source and vet every server themselves.
- OTEL export gated behind Business tier.

### AX notes
- Built for IT admins via dashboard; nothing agent-operable documented. The "UI-only policy config" critique is the clearest statement in this cohort of the gap an agent-first CLI/MCP control plane would fill.

---

## Cross-Vendor Synthesis

### Table stakes (everyone has, buyers expect)
| Capability | Runlayer | MintMCP | Natoma | Operant | Airia | MCP Manager |
|---|---|---|---|---|---|---|
| Private registry / approved catalog | ✅ | ✅ | ✅ | ⚠️ discovery only | ✅ | ✅ (no seed catalog) |
| SSO / SCIM | ✅ | ✅ (Ent.) | ✅ | ⚠️ | ✅ (Ent.) | ✅ (Business) |
| Per-tool-call audit log | ✅ | ✅ | ✅ tamper-evident | ✅ | ✅ | ✅ |
| RBAC/ABAC to tool level | ✅ ABAC 6-dim | ✅ roles | ✅ Cedar ABAC | ✅ | ✅ | ✅ RBAC |
| MCP threat detection (poisoning/shadowing/rug-pull/injection) | ✅✅ | ⚠️ rules | ✅ DLP+block | ✅✅ | ⚠️ | ✅ |
| Credential injection / vaulting | ✅ +1Password | ✅ | ✅ +BYO | ⚠️ | ✅ | ✅ |
| SIEM/OTEL export | audit trails | OTEL (Ent.) | Splunk/Sentinel/Falcon | ✅ | ⚠️ | Splunk/Datadog/OTEL |
| Self-host/VPC option | ✅ | ⚠️ contact | ✅ +on-prem | K8s | ⚠️ | ⚠️ |
| Shadow-AI/MCP discovery | ⚠️ | ⚠️ | ✅✅ (225/org) | ✅ | ⚠️ | ⚠️ |
| STDIO→remote hosting | ✅ local CLI | ✅✅ signature | ✅ desktop mgmt | ⚠️ | ⚠️ | ✅ |

### Enterprise evaluation criteria they compete on (from obot.ai's 6-axis rubric + vendor messaging)
1. **Deployment model** — SaaS vs VPC vs on-prem vs air-gapped.
2. **Openness** — can security teams inspect the detector stack, policy engine, request path? (All six fail this.)
3. **Catalog & discovery** — curated self-service directory; shadow-AI inventory.
4. **Identity & policy** — IdP federation, per-user identity propagation through to downstream servers, agent identities.
5. **MCP-specific threats** — rug pull, tool poisoning, cross-server shadowing, fake servers, prompt injection.
6. **Operational maturity** — rate limiting, circuit breaking, retries, OTEL.

### Published performance numbers (sparse — an open lane)
- Natoma: 1.8M tool calls/day capacity; 99.9% uptime; 225 avg shadow-AI instances found per enterprise.
- Runlayer: "no noticeable performance impact" (unquantified); "10-minute deployment"; 18k+ catalog; 300+ clients.
- Airia: 1,000+ integrations (catalog size as the metric).
- Operant/MintMCP/MCP Manager: no hard perf numbers. **Nobody publishes latency overhead benchmarks.**

### Pricing landscape
- MintMCP: $1,250/mo (50 seats) Teams; Enterprise custom. Seat-based.
- Airia: Free / $50 / $250 / Enterprise. Execution-based.
- MCP Manager: free tier; paid tiers undisclosed.
- Runlayer, Natoma, Operant: sales-led, undisclosed.

### Features worth stealing for an OSS gateway
1. **MintMCP's STDIO→remote pipeline** (paste a STDIO config, get a hosted, OAuth'd, logged remote server) — the single highest-leverage enterprise wedge in the cohort.
2. **Virtual MCP servers / role-based tool bundles** with a curated tool manifest returned at connect-time (MintMCP VMCPs, Natoma Profiles, Runlayer registry).
3. **Cedar (or similar) policy-as-code ABAC** — Natoma proves it works; MCP Manager's UI-only policy is explicitly criticized. GitOps-able policy is an open gap.
4. **Tool-description change detection / rug-pull alerts** (MCP Manager) — cheap to build, high perceived value.
5. **Six-dimension ABAC context** (Runlayer): user, device, client, server, session, request.
6. **Per-request risk scoring + pre-approval scanning of servers** (Runlayer) with results shown in an approval workflow.
7. **Shadow-MCP discovery** (Natoma/Operant) — inventory unmanaged MCP configs on endpoints; quantify it ("we found N").
8. **IDE-hook enforcement** (Runlayer × Cursor Hooks): allow/deny tool calls inside the agent loop, not just at the network hop.
9. **Tamper-evident audit logs with SIEM forwarding** (Natoma) and **OTEL export not gated to a paid tier** (counter MCP Manager/MintMCP gating).
10. **Inline redaction/DLP on tool inputs AND outputs** (Operant, Natoma, MCP Manager/Presidio).
11. **Agent identities as first-class principals** distinct from users (MintMCP, MCP Manager, Operant NHI).
12. **Published catalog + one-click client install for 300+ clients** (Runlayer) — distribution mechanics matter.

### Collective weaknesses = OSS opportunity
- 100% closed-source; detector stacks unversioned and un-benchmarkable — "openness" is a stated buyer criterion nobody satisfies.
- Demo-gated evaluation; thin public docs (MintMCP partial exception).
- No unified LLM-gateway + MCP-gateway product; MCP-only governance leaves model-call cost/routing to a second vendor.
- No agent-operable control plane (no MCP server for the gateway itself, no CLI-first workflow except Runlayer's local tooling); policy config is dashboard-bound.
- No latency benchmarks published anywhere.
- Consolidation risk: Natoma→Snowflake shows customers these startups can vanish into hyperscaler stacks; an OSS single binary is the hedge.
- Pricing opacity (4 of 6) frustrates self-serve adoption; MintMCP's $15k/yr floor prices out small teams.

### Sources
- runlayer.com (/, /security, /about, /blog/cursor-hooks); TechCrunch 2025-11-17 (Runlayer $11M)
- mintmcp.com (/docs/intro, /docs/architecture, /pricing, /mcp-gateway, vs-blogs)
- natoma.ai (/, /platform); Snowflake press release + BusinessWire 2026-05-27 (acquisition)
- operant.ai (/solutions/mcp-gateway, /platform/ai-gatekeeper, Gartner blog); SiliconANGLE 2025-06-16
- airia.com (/airia-launches-mcp-gateway/, /ai-platform/mcp-capabilities/); GlobeNewswire 2026-02-26 (1,000 integrations); dupple.com pricing
- mcpmanager.ai (/, /pricing-plans/, /solutions/features-overview/); usercentrics.com
- obot.ai "The 13 Best MCP Gateways for Enterprise Teams in 2026" (third-party criticism + evaluation rubric)
