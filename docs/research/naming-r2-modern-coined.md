# Naming Round 2 — Direction: Modern-Coined

Brief: Open-source AI gateway — unified, fastest, most comprehensive LLM + MCP gateway. One Rust binary, dashboard included, agent-first (CLI + its own MCP server), Apache-2.0. Must read as serious infra (envoy/caddy/vector/temporal neighbor) yet be memorable, ownable, typable as a CLI command and GitHub org. Short INVENTED/coined words, 4-6 letters, no meaning required. NOT mythological, NOT a place name, NOT a metaphor needing explanation.

## Brainstorm (30+ raw candidates)

Volt, Vail, Praxa, Quell, Klyn, Brix, Stax, Velo, Volo, Rive, Surl, Loken, Vora, Synd, Klave, Naxa, Orin, Lume, Riven, Vex, Skell, Bramo, Vane, Velt, Korl, Pylo, Maro, Nexo, Verra, Caro, Velar, Vesp, Klyro, Portyl, Tyne, Korv, Pyra, Volk, Vael, Skyl, Drix

Quick taste cuts:
- Real English words (violate "no meaning"): Volt, Vail, Brix(-ish), Rive, Surl, Vane, Vex, Drift, Glide, Trove, Veil, Quell (real word — "to suppress")
- Too close to existing famous infra/AI: Nexo (crypto), Plex, Onyx, Velox
- Weak phonotactics / hard to say once: Klyn, Synd, Skell, Portyl, Bramo

## Narrowed to 8 best (with collision findings)

### 1. Tyne — VERDICT: clean
Coined-feeling, 4 letters, one syllable, types beautifully as `tyne` / `github.com/tyne`. No AI/dev/infra collision surfaced across web, npm, crates.io, PyPI, GitHub orgs. (It is a UK river name, so some generic geographic results exist, but ZERO conflict in our space.) Strongest clean option.

### 2. Korv — VERDICT: clean
4 letters, hard consonant edges read as serious infra (cf. Kong, Caddy). No software/AI/dev-tool or package collision found. Nearest neighbor is "Korbit AI" (code review) — distinct name, distinct space. Slight phonetic note: "korv" is Swedish for sausage — unlikely to matter to an EN dev audience but worth flagging.

### 3. Naxa — VERDICT: minor-friction
4 letters, clean CVCV, easy to say. No npm/crates/PyPI/GitHub-org collision in our space. BUT several existing companies share the name: NAXA (Nepal geo-IT), NAXA Inc (Japan broadcast), Naxa Electronics (consumer AV), Nax Solutions (AI API, different spelling). None are LLM/AI-gateway/dev-infra, so SEO/trademark friction is moderate, not fatal.

### 4. Quell — VERDICT: minor-friction
Reads clean and confident, types well as `quell`. Only a low-footprint "Quell" LinkedIn page in AI surfaced; no prominent product, package, or repo collision. Caveat: it is a real English word ("to suppress/quiet"), which softly bends the "no meaning" taste bar — but the meaning is neutral-to-pleasant for a control-plane/gateway and is not a cute misspelling.

### 5. Klyro — VERDICT: minor-friction
5 letters, modern coined feel, distinctive. Collisions exist but are small and scattered: an AI assistant widget shipping `@klyro/widget` on npm, a Web3 developer-credentialing project "Klyro", and a "Klyro.digital" agency. None are AI gateways or core infra, but the npm scope `@klyro` being taken is real friction for package naming.

### 6. Vesp — VERDICT: minor-friction
4 letters, sharp and infra-credible. Risk is proximity to "Vespa" (vespa-engine — a prominent AI search platform) and existing `@vesp/*` npm packages + a "vesp" GitHub repo + the "Vesper" Node framework. Distinct enough as `vesp`, but lives in a crowded V-e-s phonetic neighborhood — expect confusion with Vespa.

### 7. Velo — VERDICT: collision
Excellent sound and typability, but the space is crowded: multiple "Velo AI" products (legacy modernization, test generation, marketing agent), a YC company "Velos", and GitHub repos. Strong name, but materially contested in the AI/dev space. Documented because the founders may still love the sound — but it is not ownable as-is.

### 8. Klave — VERDICT: collision
Looks and types like premium infra, but it is a DIRECT collision: Klave (klave.com / klave-network GitHub org / Secretarium) is an existing confidential-compute developer PaaS for WebAssembly/Rust apps — same audience, same "serious infra" register, and they even market "Klave AI." Avoid.

## Recommendation order for the founders

1. Tyne — cleanest, most ownable, best CLI/org fit. Top pick.
2. Korv — clean and infra-toned; check the Swedish connotation is acceptable.
3. Quell — clean footprint; accept that it's a soft real word.
4. Naxa — usable if comfortable competing with non-overlapping NAXA companies on SEO/TM.

Hard avoids from this set: Klave (direct infra collision), Velo (crowded AI space), Riven/Vora/Orin/Korl/Pyra/Velt/Volk/Verra (all checked, all collide — see notes below).

## Also-checked, rejected for collision (do not pursue)
- Riven — Riven AI is literally an AI-native developer platform with MCP/agents. Hard collision.
- Vora — multiple AI companies (Vora IQ, Vora.software, Solana Vora). Crowded.
- Orin — NVIDIA Jetson Orin is a flagship AI-compute brand. Hard collision.
- Verra / Vera — "Vera, The AI Gateway" exists (askvera.io). Direct space collision + Verra carbon registry.
- Korl — recent AI startup (korl.co, customer presentations). Collision.
- Pyra — several AI companies/platforms (pyraai, pyrabuilds, pyra crypto). Crowded.
- Volk — gnuradio/volk and zeux/volk are well-known libraries; Volk AI exists. Collision.
- Velt — YC company, collaboration SDK, github.com/velt-js. Hard dev-tool collision.
- Pylo — existing tech company (MCreator) + github.com/Pylo org. Collision.
