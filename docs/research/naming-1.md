# Naming Brainstorm — Open-Source AI Gateway (LLM + MCP)

Brief: unified, fastest, most comprehensive LLM + MCP gateway. One binary, dashboard included, agent-first (CLI + own MCP server). Must sound like serious infrastructure (Envoy, Caddy, Traefik, Vector, Temporal) yet be memorable. Style: Greek/Latin/mythology + infrastructure metaphors.

Date: 2026-06-10. Collision checks via WebSearch + direct registry probes (registry.npmjs.org, pypi.org/pypi, crates.io/api).

---

## Full candidate list (30)

| # | Name | Meaning / metaphor | Quick verdict |
|---|------|--------------------|---------------|
| 1 | **Diolkos** | The paved trackway across the Isthmus of Corinth — ships hauled overland between two seas. "Move payloads across the divide." | TOP 8 — cleanest of all |
| 2 | **Frontinus** | Sextus Julius Frontinus, Rome's *curator aquarum* — audited aqueduct flow, metered usage, caught illegal taps (literally shadow-usage governance) | TOP 8 — clean registries, perfect story |
| 3 | **Telamon** | Architectural load-bearing column figure (male Atlas); also Argonaut | TOP 8 — only a dormant academic GPU project |
| 4 | **Mansio** | Official waystation on Roman roads — the relay stop of the *cursus publicus* | TOP 8 — registries clean, tiny GmbH exists |
| 5 | **Euripus** | Strait of Chalkis whose current reverses direction several times a day — high-velocity bidirectional flow through a narrow channel; site of the world's first drawbridge | TOP 8 — essentially unclaimed |
| 6 | **Claviger** | Latin "key-bearer" — gatekeeper, holder of all the keys (API keys pun) | TOP 8 — light collisions, PyPI taken |
| 7 | **Cardea** | Roman goddess of the door hinge — "her power is to open what is shut, to shut what is open" (Ovid) | TOP 8 — best mythology fit, but npm/PyPI taken |
| 8 | **Agger** | Roman defensive embankment/rampart; happy echo of "aggregator" | TOP 8 — registries clean, minor collisions |
| 9 | Portunus | Roman god of keys, doors and harbors — thematically perfect | Rejected: 4+ active GitHub infra projects + npm package |
| 10 | Castellum | *Castellum divisorium* — the aqueduct distribution tank that splits flow to destinations (literally a router) | Rejected: OpenStack autoscaler (SAP), Max Planck product, more |
| 11 | Pharos | Lighthouse of Alexandria | Rejected: Workday's observability platform, pharos.ai, CMU SEI tool, npm crate |
| 12 | Sluice | Gate that controls flow | Rejected: existing credential-governance proxy/MCP gateway for AI agents (nnemirovsky/sluice) — direct collision |
| 13 | Keryx | Greek herald/messenger | Rejected: actionhero/keryx is "the TypeScript framework for MCP and APIs" + Keryx Labs (decentralized AI) — direct collisions |
| 14 | Propylon | Monumental temple gateway | Rejected: 25-year-old legal-tech company, acquired by RWS for $33M |
| 15 | Thyra | Greek "door" | Rejected: Trilinos package + massalabs blockchain gateway |
| 16 | Limen | Latin "threshold" | Rejected: 3+ existing AI-agent projects named Limen, one MCP-based |
| 17 | Postern | Secondary castle gate | Rejected: well-known Android VPN/proxy app |
| 18 | Cursus | The *cursus publicus*, Rome's imperial relay network | Rejected: means "course/training" in Dutch/French; 42-school "cursus" noise |
| 19 | Dromos | Greek "road"; entrance passage | Rejected: Prescott Data ships "Dromos" for enterprise AI automation/orchestration — direct collision |
| 20 | Portolan | Medieval harbor-routing charts | Rejected: Triton VXLAN daemon, geospatial SDI toolkit, several more |
| 21 | Quoin | Cornerstone of a wall | Rejected: Quoin Inc. consultancy; "Quoine" crypto exchange; coin homophone |
| 22 | Vallum | The rampart along Hadrian's Wall | Rejected: commercial macOS application firewall (vallumfirewall.com) |
| 23 | Ostia | Rome's harbor — the empire's gateway port | Rejected: 3scale-labs/ostia, an OpenShift-native API-management gateway — same category |
| 24 | Narthex | Church vestibule — the entrance you pass through | Borderline: pentest dictionary generator + 2 data tools; usable but crowded |
| 25 | Pylos | From Greek *pylē* (gate); Mycenaean palace city | Rejected: Gigamic board game dominates the name |
| 26 | Lintel | The beam over every doorway | Rejected: npm, PyPI AND crates.io all taken |
| 27 | Herma | Boundary-marker pillar of Hermes guarding doors/crossroads | Rejected: HERMA = €450M German labels company; Hermes namespace exhausted (Meta JS engine, HashiCorp, Nous models) |
| 28 | Pontifex | Latin "bridge-builder" | Rejected (pre-search): papal connotation dominates |
| 29 | Barbican | Fortified outer gateway of a castle | Rejected (known): OpenStack Barbican key manager + Barbican Centre |
| 30 | Stylobate | The platform on which all columns stand | Borderline: clean but 3 syllables of architecture jargon, hard to say |

Also considered and dropped without search (known collisions): Bifrost (Maxim AI's LLM gateway), Janus (DeepSeek model), Hermes (many), Heimdall (DB proxy), Talos (Cisco + Talos Linux), Anubis (Techaro proxy), Argus, Cerberus, Keystone (OpenStack/JS), Meridian (Google OSS), Pylon (YC co), Temenos (banking giant), Apogee (too close to Apigee), Ballista (DataFusion), Xenia (emulator), Obol (crypto), Aqueduct (defunct ML co), Flume (Apache), Wicket (Apache), Turnstile (Cloudflare), Torii (SaaS mgmt), Mimir (Grafana).

---

## Top 8 — deep collision analysis

Registry probes: 404 = available, 200 = taken.

### 1. Diolkos ⭐ recommended
- **Rationale:** The Diolkos was a stone trackway that let ships cross the Isthmus of Corinth overland — heavy payloads hauled between two seas without sailing around the Peloponnese. A gateway that moves your traffic across the provider divide, faster than the long way around. Obscure enough to own completely, real enough to have a Wikipedia page and 2,600 years of history. Says "serious infrastructure" the way Envoy/Traefik do.
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. GitHub: one tiny inactive demo repo (trisberg/diolkos, Spring Cloud k8s samples) that itself cites the same etymology; a Greek desktop road-design CAD product "DIOLKOS" (diolkos3d.com) in a totally unrelated market. No AI/devtools/infra company. **Cleanest name checked.**
- **Risks:** pronunciation (dee-OL-kos) needs a one-line guide in the README; that's Kubernetes-tier friction, survivable.

### 2. Frontinus
- **Rationale:** Frontinus was Rome's water commissioner (*curator aquarum*, 97 AD) and wrote *De Aquaeductu* — he measured every aqueduct's flow, standardized the nozzles (rate limits), and exposed people illegally tapping the pipes (shadow AI). A gateway with a dashboard, metering, and governance built in could not ask for a better patron. Sounds like Temporal-grade gravitas.
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. GitHub: only a personal username (frontinus, Ada parser) and a BibliothecaDAO docs repo ("Frontinus House"). No company, no product.
- **Risks:** 3 syllables, slightly bookish; person-name rather than thing-name (like Caddy vs Envoy — works either way).

### 3. Euripus
- **Rationale:** The Euripus strait reverses its current several times a day — the canonical image of high-velocity, bidirectional flow through one narrow channel. Aristotle supposedly died frustrated trying to explain it. Also home to the world's first drawbridge (a literal movable gateway, 411 BC). Great for a streaming, duplex (LLM + MCP both directions) gateway.
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. Web: only geography, dictionaries, and one un-published academic hardware-checkpointing paper. Effectively unclaimed in software.
- **Risks:** spelling/pronunciation (yoo-RYE-pus) is the hardest of the top 8; "Euripos" variant spelling splits searches.

### 4. Mansio
- **Rationale:** A *mansio* was the official relay station of the Roman imperial post — where couriers stopped, swapped horses, and continued at full speed. The relay/waystation metaphor maps exactly onto a gateway that receives, re-routes, and forwards model traffic. Short, pronounceable in every language, ends in -io like a devtool should.
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. GitHub: Mansio GmbH, a 2-repo German org (goapiutils, navclient) — negligible. Adjacent-name noise: "Software Mansion" (React Native consultancy) and mansion-io could cause mild confusion.
- **Risks:** the Software Mansion adjacency; autocorrect to "mansion".

### 5. Telamon
- **Rationale:** A telamon is the colossal male figure carved to carry the weight of a building on its shoulders (the male caryatid/Atlas). "The thing that bears the load" — perfect for the binary that carries all of your org's AI traffic. Sounds like a Greek hero because he also was one (father of Ajax, Argonaut).
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. GitHub: ulysseB/telamon, an academic GPU-kernel-optimization framework (Rust, dormant since ~2019) — same broad "performance" aura but dead and niche; plus a personal username. No company.
- **Risks:** the dormant Rust research project shares ecosystem (Rust + performance); low but nonzero confusion.

### 6. Claviger
- **Rationale:** Latin for "key-bearer" (epithet of Janus, the two-faced god of gates, holding the keys). The gateway literally holds all your provider keys and decides which doors open. Distinctive, growls nicely, zero English-word baggage.
- **Collisions:** npm 404, crates.io 404; **PyPI 200 — taken** by bwesterb/claviger, an SSH authorized_keys synchronizer (small, maintained-ish, also "key" themed — awkward). Two other hobby repos (Android hwinfo client, systems-management tool).
- **Risks:** the PyPI squat by another key-management tool is a real annoyance for a Python SDK; would need `claviger-ai` style prefixing. Knocks it down the list.

### 7. Cardea
- **Rationale:** The single best mythological fit found: Roman goddess of the door hinge, whose power per Ovid is "to open what is shut; to shut what is open," honored alongside Forculus (doors) and Limentinus (threshold). The hinge is the part of the gate everything swings on.
- **Collisions:** **npm 200 and PyPI 200 — both taken.** Crunchbase shows several Cardea companies (Cardea Bio — acquired by Paragraf; Cardea Technology — medical imaging; Cardea Software — cardiac data). None in devtools/AI infra, but the namespace is busy and both key registries are squatted.
- **Risks:** registry squats force suffixed package names from day one; multiple existing companies dilute SEO. Beautiful story, mediocre availability.

### 8. Agger
- **Rationale:** The Roman *agger* — the rampart-and-embankment that defined a fortified perimeter (your single controlled entry point for AI traffic), with a free bonus echo of "aggregator" — which is literally what a unified gateway is. Five letters, sounds like it belongs next to Envoy and Vector.
- **Collisions:** npm 404, PyPI 404, crates.io 404 — all free. GitHub: "Agger Sistemas" (small Brazilian insurance-software org), a Danish developer's username. Real-world: Daniel Agger (famous Danish footballer) owns general SEO.
- **Risks:** footballer SEO; "agger" reads as nonsense to non-Latin readers; could be heard as "aggro."

---

## Recommendation

**Diolkos** first (own it outright; the haul-across-the-isthmus story is a marketer's gift), **Frontinus** second (the metering/audit/anti-shadow-usage story aligns eerily well with a gateway+dashboard product), **Euripus** third if you want pure flow-speed connotation. If you want maximum safety on registries with the least pronunciation risk, **Mansio**.

Suggested namespace grabs for whichever wins: GitHub org, npm, PyPI, crates.io, Homebrew formula name, .dev/.io domains, @handle on X.
