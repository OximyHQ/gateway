# Naming Round 3 — Plain-English Compounds + Single Evocative Words

Brief: name an open-source AI gateway — unified, fastest, most comprehensive LLM + MCP gateway. One binary, dashboard included, agent-first (CLI + own MCP server). Must sound like serious infrastructure (Envoy, Caddy, Traefik, Vector, Temporal) yet be memorable.

Style constraints for this round:
- Plain-English compounds (OpenRouter/LiteLLM style: descriptive + brandable)
- Single evocative English words (relay, switchboard, junction, overpass energy)

## Full candidate list (33)

### Single evocative words — connection/routing infrastructure

| # | Name | Metaphor / vibe |
|---|------|-----------------|
| 1 | **Trunkline** | Telecom: the high-capacity main line all calls ride. "Everything flows through the trunkline." |
| 2 | **Switchboard** | The operator panel that connects any caller to any line. Perfect semantic fit for LLM+MCP routing. |
| 3 | **Switchyard** | Rail yard where cars are routed between lines. Industrial, physical, serious. |
| 4 | **Junction** | Where lines meet and traffic chooses a path. Short, strong. |
| 5 | **Patchbay** | Audio engineering: one panel that wires any input to any output. Exactly what an MCP gateway does. |
| 6 | **Viaduct** | Elevated span carrying many lanes over obstacles. Envoy-class gravitas. |
| 7 | **Interchange** | Highway interchange: every direction reachable from one structure. |
| 8 | **Causeway** | Engineered raised road across water — reliable passage. |
| 9 | **Gantry** | The overhead structure spanning all lanes (toll gantries read every car = observability vibe). |
| 10 | **Overpass** | Traffic flows over, unimpeded. |
| 11 | **Relay** | The classic: receives and forwards. |
| 12 | **Conduit** | The protected channel everything runs through. |
| 13 | **Crossbar** | Crossbar switch — the original any-to-any telephone exchange fabric. |
| 14 | **Manifold** | One intake, many outlets (engine manifold). Great for fan-out routing. |
| 15 | **Drawbridge** | Controlled access point — gateway + security connotation. |
| 16 | **Aqueduct** | Roman infrastructure: moves the essential resource at scale. |
| 17 | **Mainline** | The primary track; also "mainline a change." |
| 18 | **Turnpike** | Managed high-speed roadway; tolls = metering/cost-tracking connotation. |
| 19 | **Spillway** | Engineered channel that handles overflow safely (failover vibes). |
| 20 | **Flume** | Engineered water channel; fast flow. |
| 21 | **Culvert** | Understated channel under the road. Humble-infra like Caddy. |
| 22 | **Transom** | The structural crossbeam over a doorway. |
| 23 | **Sluice** | Gate that controls flow — rate-limiting connotation. |
| 24 | **Breakwater** | Protective structure all traffic shelters behind (guardrails vibe). |
| 25 | **Wireway** | Electrical: the enclosed channel that carries every wire. |

### Plain-English compounds — descriptive + brandable

| # | Name | Logic |
|---|------|-------|
| 26 | **ModelPort** | Models + port (harbor where everything docks / network port). One word, instantly parseable. |
| 27 | **ModelRelay** | Says exactly what it does: relays model traffic. |
| 28 | **OmniRoute** | Routes everything — LLMs and MCP alike. |
| 29 | **AgentGate** | The gate agents pass through; agent-first positioning baked in. |
| 30 | **WireGate** | Wire-level gateway; WireGuard-adjacent toughness (maybe too adjacent). |
| 31 | **OneRelay** | One binary, one relay for all AI traffic. |
| 32 | **AnyModel** | OpenRouter-style promise: any model behind one endpoint. |
| 33 | **UniRoute** | Unified routing; terse. |

## Top 8 — collision checks (WebSearch, 2026-06-10)

### 1. Trunkline — CLEAR (best result of the round)
- Searched: `"Trunkline" software github npm project`. **No exact software collision found.** Results were all adjacent "trunk" tools: trunk.io (code quality), trunk-rs/trunk (Rust WASM bundler), trunk-based development — none named Trunkline.
- Rationale: telecom trunk line = the single high-capacity line that carries all calls between exchanges. Exactly the product: all AI traffic rides one line. Sounds like Vector/Temporal-class infra, verbs well ("point it at the trunkline"), `trunkline` likely free on npm/crates.
- Risk: low. "Trunk" neighborhood is busy but the full word is distinct.

### 2. ModelPort — CLEAR
- Searched: `"ModelPort" github npm pypi`. **Nothing found** under that name on GitHub/npm/PyPI.
- Rationale: most OpenRouter/LiteLLM-style candidate — descriptive (models through a port) and brandable. Double meaning: harbor port (everything docks) + network port (one binary listening).
- Risk: low-moderate. Slightly "product-y" rather than "infra-y"; check ONNX-adjacent "model porting" tools before committing.

### 3. Switchyard — MODERATE (usable, juniors only)
- Found: JBoss SwitchYard (SOA framework, **deprecated ~2017**, absorbed into Camel); jsommers/switchyard (educational Python networking framework); barakmich/switchyard (small Go vhost router); veighnsche/switchyard (Rust fs library).
- Rationale: rail-yard routing metaphor is dead-on; industrial and serious. The only sizable collision is a dead JBoss project.
- Risk: moderate. Red Hat history means stale SEO; no AI-space conflict.

### 4. Switchboard — CROWDED
- Found: George5562/Switchboard — **an MCP gateway on npm** (`@george5562/switchboard`, lazy-loads child MCPs); TentacleOpera/switchboard (VS Code AI agent teams); igrigorik/AgentBoard markets itself as "a switchboard for AI"; `SwitchBoard` + `switchboard-automation` on npm; switchboard.xyz (Solana oracle).
- Rationale: semantically the single best word for this product — which is exactly why it's already taken in the exact MCP-gateway niche.
- Risk: high. Direct category collision.

### 5. Junction — CROWDED
- Found: FMotalleb/junction (reverse proxy with SNI/header autorouting — same category); github.com/junction org (Junction Networks); Junction Labs (junctionlabs.io, service-discovery/routing library).
- Rationale: short, strong, infra-correct.
- Risk: high. Multiple routing/proxy projects share it; generic word, weak trademark.

### 6. Patchbay — TAKEN on npm
- Found: `patchbay` npm name taken (Secure Scuttlebutt client, ssbc/patchbay); patchbay.tools / @patchbayhq (audio UI components); patchbay.js (DOM rope library).
- Rationale: the audio-engineering metaphor (any input wired to any output) is the most precise metaphor in the whole list for an LLM+MCP fabric.
- Risk: high for the bare name; would need a prefix/suffix, which kills the elegance.

### 7. Viaduct — TAKEN (big-co collision)
- Found: **airbnb/viaduct** (GraphQL unified data-access layer — active, well-known); jace-ys/viaduct (Go API gateway — same category); apl-cornell/viaduct (crypto compiler); WilliamVenner/viaduct (Rust IPC).
- Risk: high. Airbnb owns the mindshare; a small Go API gateway already uses it for the same job.

### 8. Interchange — TAKEN
- Found: interchange/interchange — Perl ecommerce application server, 25+ years old, still maintained (interchangecommerce.org).
- Rationale: highway-interchange metaphor is great; word is too generic and already a long-lived OSS server name.
- Risk: moderate-high. Old collision + near-zero searchability.

### Also checked (knocked out before top-8)
- **Causeway** — BLOCKED: Apache Causeway (TLP Java framework, renamed from Isis 2022). Apache trademark; do not use.
- **Gantry** — BLOCKED-ish: gantry.io (ML observability startup, $28M raised, Greg Brockman investor — directly in the AI-tooling space) + Gantry5 theme framework.

## Recommendation

1. **Trunkline** — only candidate that is simultaneously evocative, infra-serious, category-accurate, and collision-clear. Tagline writes itself: "The trunkline for AI traffic."
2. **ModelPort** — strongest descriptive compound, clear on all registries; the OpenRouter-style "safe" pick.
3. **Switchyard** — best fallback if a single evocative word is required and Trunkline doesn't land; only collision is a dead JBoss project.

Avoid Switchboard / Junction / Patchbay / Viaduct despite their semantic fit — each has a live collision in or near the gateway/proxy category.

Sources consulted:
- https://github.com/George5562/Switchboard, https://www.npmjs.com/package/SwitchBoard, https://github.com/igrigorik/AgentBoard
- https://github.com/FMotalleb/junction, https://github.com/junction
- https://www.npmjs.com/package/patchbay/v/8.0.0, https://github.com/ssbc/patchbay, https://ui.patchbay.tools/
- https://github.com/airbnb/viaduct, https://github.com/jace-ys/viaduct
- https://github.com/apache/causeway
- https://www.gantry.io/, https://techcrunch.com/2022/06/07/crunch/, https://github.com/gantry/gantry5
- https://github.com/jboss-switchyard/switchyard, https://github.com/jsommers/switchyard, https://github.com/barakmich/switchyard
- https://github.com/interchange/interchange, https://www.interchangecommerce.org/i/dev
- https://trunkbaseddevelopment.com/, https://www.npmjs.com/package/trunk (adjacent-only; no Trunkline hit)
