# Naming Round 2 — Direction: Gateway-Literal

Brief: name an open-source AI gateway — one Rust binary, dashboard included,
agent-first (CLI + its own MCP server), Apache-2.0. Must read as serious
infrastructure (envoy/caddy/vector/temporal) yet be memorable and ownable.
This direction leans INTO the gateway/door/passage concept in fresh modern
(near-)English. Founders rejected obscure mythological/ancient-place names.

## Full brainstorm (30+)

Port family: porta, portal, portus, portman, freeport, harbor, haven, wharf,
quay, jetty, pier, dock, berth, gangway, waypoint, wayport

Door / gate family: stile, postern, wicket, transom, lintel, jamb, mullion,
threshold, sill, casement, vestibule, foyer, atrium, alcove, archway, arcade,
narthex, vestry

Passage / way family: causeway, throughway, conduit, culvert, sluice, weir,
lock, lockgate, channel, strait, ford, span, viaduct, sallyport

## Taste filtering

Cut for collision-in-infra (verified, see below): wicket (Apache Wicket),
causeway (Apache Causeway), quay (Red Hat Project Quay), waypoint (HashiCorp).
Cut for being too plain / hard to own: portal, harbor, haven, channel, dock,
gateway-as-is. Cut for ugliness/unpronounceable-on-sight: jamb, mullion,
culvert, narthex-adjacent vestry.

## The 8 (best on taste) + collision findings

### 1. postern  — VERDICT: clean
A small secondary gate in a fortress wall — a real "gateway" word, short,
typeable (`postern serve`), pronounceable on sight. Quiet, infrastructural,
not cute. No hits in the AI-gateway space; no crates.io / npm / PyPI package;
no prominent GitHub repo. The cleanest name in this set. Top pick.

### 2. stile  — VERDICT: minor-friction
A set of steps that let you cross a wall/fence — a literal passage-through.
Five letters, instantly typable, clean. Only collision is a tiny dormant npm
package `stile` (a React inline-style helper, bloodyowl/stile). No infra/AI
overlap, no crate. Brandable and largely ownable.

### 3. narthex  — VERDICT: minor-friction
The entrance/threshold hall of a building — the literal "way in." Distinctive,
ownable, no AI-gateway collision. Several small unrelated GitHub repos exist
(delving/narthex metadata tool; a C dictionary generator; a MarkLogic
front-end) but none prominent and none in this space. Slightly less obvious to
pronounce than postern/stile.

### 4. transom  — VERDICT: minor-friction
The crossbar/window above a door — a gateway part, modern-sounding, good as a
command (`transom up`). Collision: TransomJS, a Node REST-API framework on npm
(@transomjs/transom-core) plus a minor static-site generator. Adjacent to the
API/backend space, so meaningful but not fatal friction.

### 5. gangway  — VERDICT: minor-friction
The boarding bridge onto a ship — a vivid passage word, very brandable. But
there is a recognizable (VMware-archived, deprecated 2021) Kubernetes OIDC auth
tool named gangway. Same infra/k8s neighborhood — the strongest friction of the
"minor" set despite being archived.

### 6. sallyport  — VERDICT: minor-friction
A secured gateway through a fortification — perfect "gateway" semantics, very
ownable. Collision: `sallyport` is a Rust crate in the Enarx confidential-
computing project (archived repo, still vendored in enarx/enarx). A crates.io
clash matters for a Rust binary, but it's a library not a product.

### 7. freeport  — VERDICT: collision
A port open to all trade — clean "port/gateway" meaning. But `freeport` is a
generic, well-known npm utility meaning "find a free TCP port," and the literal
reading collides with what an infra binary does. Confusing in a dev shell.

### 8. wicket  — VERDICT: collision
A small gate/door — perfect literal meaning and a lovely word. Disqualified:
Apache Wicket is a long-standing, well-known Java web framework (apache/wicket),
plus an npm `wicket` (WKT geometry lib). Exactly the "lives next to envoy"
shadow we must avoid.

## Recommendation

Lead with **postern** (clean, infra-toned, ownable, on-sight pronounceable),
backed by **stile** (shortest/cleanest, near-zero collision) and **narthex**
(most distinctive, fully ownable). transom / gangway / sallyport are viable but
each carries one adjacent-namespace clash. freeport and wicket are out.
