# Naming R2 — Direction: plain-english-punch

Single punchy real English words or tight compounds an engineer reads and instantly
gets — lineage of OpenRouter/Helicone but cleaner. One-syllable-leaning, strong
consonants, instantly typable as a shell command, no cute misspellings, no SaaS-y
CamelCase. Must read as serious infra (envoy/caddy/vector/temporal neighbors) and
not collide with existing AI/devtools.

Target product: unified, fastest, most comprehensive LLM + MCP gateway. One Rust
binary, dashboard included, agent-first (CLI + its own MCP server), Apache-2.0.

---

## Full brainstorm (40+)

relay, conduit, throughway, crossbar, manifold, lattice, current, hub, span, weld,
splice, fuse, mesh, drift, dispatch, beacon, ferry, portage, junction, switchyard,
throughput, channel, tunnel, bridge, gateway, transit, vector, pivot, router,
funnel, sluice, valve, port, dock, wharf, causeway, pylon, girder, truss, trunk,
trunkline, busbar, plexus, nexus, ridge, vane, throughline, spillway, header,
patchbay, tieline, feedline, mainline, conductor, bus.

Pruned immediately for collision/genericness/taste: gateway (literal category),
router (OpenRouter shadow + every network box), vector (Vector by Datadog — the
exact infra neighbor named in the brief), relay (LiveKit Relay, Resend Relay,
crowded), channel/tunnel/bridge (generic), nexus (Sonatype Nexus), mesh (service
mesh category term), dispatch (Twilio/PagerDuty-ish, also a crate), current
(RethinkDB Horizon "current"-ish + too soft), fuse (FUSE filesystem), hub (GitHub /
Docker Hub), span (OpenTelemetry span — semantic landmine for a tracing-adjacent
gateway), lattice (Cosmonic Lattice, wasmCloud).

---

## The 8 (researched for collisions)

### 1. Throughway — VERDICT: clean
The straight-road word for "the thing all traffic goes through." Reads instantly as
a gateway, one strong compound, types clean (`throughway ...`). No software, npm,
crate, PyPI, or AI-company collision surfaced — search returns only the generic
"AI gateway" category pages, nothing named Throughway. Best blend of meaning +
ownability in the set. Mild note: slightly longer to type than a one-syllable mark.

### 2. Busbar — VERDICT: clean
Electrical term: the single conductor bar every circuit taps into to distribute
power — a near-perfect metaphor for one binary every model/MCP call routes through.
Two hard syllables, very typable (`busbar`), feels like real infrastructure.
Collision search returns ONLY electrical-engineering CAD/design software (Easy
Busbar, FTZ-Panel, CENOS) — zero AI/devtool/npm/crate overlap. The EE meaning is an
asset, not a clash (it signals "wiring," not a competitor). Strongest "ownable +
evocative" candidate.

### 3. Spillway — VERDICT: clean
The engineered channel that controls overflow from a dam — connotes controlled,
high-volume throughput under pressure (rate limits, fallback, load). Clean compound,
typable, infra-serious. No AI/devtool/npm/crate/PyPI collision surfaced. Slightly
more "flow/water" than "gateway," so meaning is a half-step indirect, but memorable
and inevitable-sounding.

### 4. Trunkline — VERDICT: minor-friction
Telecom term for the high-capacity line carrying aggregated traffic between
exchanges — exactly the "everything funnels through one fat pipe" story. No product
named Trunkline in AI/devtools. Friction is proximity, not collision: Trunk.io (CI)
and Trunk Tools (construction AI) both own "Trunk" in dev-adjacent space, so the
shared root invites momentary confusion. The full compound is distinct and the
metaphor is excellent.

### 5. Switchyard — VERDICT: minor-friction
Rail/electrical: the yard where lines are switched and routed — vivid routing
metaphor for a gateway that dispatches across providers. Strong, characterful,
typable. Friction: a dormant JBoss "SwitchYard" SOA framework (effectively EOL) and
a Python educational networking framework named switchyard. Both are low-traffic and
out-of-segment, but the name is not pristine. Good fallback if a more distinctive
mark than the generic-gateway words is wanted.

### 6. Splice — VERDICT: collision
Great verb — to join two lines into one continuous run (cabling/film/genetics);
short, one syllable, superb shell ergonomics (`splice`). But heavily taken: a
`splice` crate on crates.io, the well-known Splice music-tech brand, and Splice NFT
tooling. The crates.io hit alone is disqualifying for a Rust binary that wants its
own crate name. Beautiful word, not ownable.

### 7. Crossbar — VERDICT: collision
Switching-fabric term (a crossbar switch routes any input to any output) — almost
purpose-built meaning for a multi-provider router. But Crossbar.io is an established
open-source WAMP application-router / microservices networking platform — same
infrastructure aisle, same "router" framing. Direct in-segment collision; avoid.

### 8. Conduit — VERDICT: collision
Clean "the pipe everything flows through" word, very brandable. But saturated in our
exact space: ConduitIO (open-source data-streaming pipelines, with an AI-pipelines
repo), Conduit AI (data-analyst/agent platform), plus other "Conduit" AI products.
Too many in-segment owners; would fight for SEO and identity from day one.

---

## Recommendation

Lead candidates, in order of conviction:

1. **Busbar** — most ownable + most evocative (one-bar-everything-taps-into);
   collision-clean in software; feels like real infrastructure.
2. **Throughway** — most literally "gets it" on read; fully clean; safest pick.
3. **Spillway** — clean, distinctive, controlled-throughput connotation.

Trunkline and Switchyard are strong-meaning fallbacks with minor proximity friction.
Splice/Crossbar/Conduit are out on collision despite excellent taste — Crossbar and
Conduit especially because the collisions sit in the same infrastructure aisle.
