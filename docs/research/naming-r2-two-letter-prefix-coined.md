# Naming Round 2 — Direction: Two-Letter-Prefix-Coined

**Brief:** Name an open-source AI gateway — unified/fastest/most-comprehensive LLM + MCP gateway, one Rust binary, dashboard included, agent-first (CLI + its own MCP server), Apache-2.0. Must sound like serious infra next to envoy/caddy/vector/temporal, yet be memorable and ownable. Lean toward the gateway/conduit concept where possible.

**Direction definition:** Coined names built from a sharp consonant cluster or short prefix + an infra-sounding suffix (—ix, —os, —ly, —ar, —yx, —en, —ic; qu/zx/kr/vx onsets used tastefully). Target the register of envoy/caddy/vector/pulsar/quasar — but new and unclaimed.

---

## Full brainstorm (50)

Quorix, Voltic, Relix, Naptic, Plyx, Vantix, Quasix, Zentry, Volar, Karix, Vexar, Nyxos, Crylix, Velar, Aptix, Synix, Pylar, Vortx, Kryos, Velix, Axion, Quaxar, Relyx, Nodix, Voxen, Klyr, Plexar, Vantyx, Galix, Vellix, Quave, Voltar, Pulix, Queryx, Lytix, Veho, Convix, Portix, Quill, Glyf, Velum, Quorum, Onyx, Vyne, Klaviz, Pylix, Vellum, Gateryx, Quanta, Veyo

Cut for taste: cute-misspell or word-collision (Glyf, Quill, Vyne, Vellum, Onyx, Quorum, Quanta), too-compound (Gateryx, Plexar-ish), too-obscure/hard-to-say (Quaxar, Klaviz, Nyxos).

---

## Final 8 — collision analysis

Searched: AI/dev companies, npm, crates.io, PyPI, GitHub orgs/repos.

### 1. Queryx — VERDICT: clean (top pick)
- On-theme: literal "query" + x; reads instantly as a request/LLM gateway. `queryx serve`, `queryx route`.
- Collision: no notable software/AI company, npm, or crate surfaced. The namespace is effectively open in our category. Strongest available result of the set.
- Risk: "query" is a common stem — generic-sounding; some users may expect a DB tool. Minor.

### 2. Kryos — VERDICT: minor-friction
- kr onset + os; cold/hard, very infra. Typable, pronounceable.
- Collision: several small unrelated companies (Kryos cold-storage temp monitoring; Kryos Systems mobility consultancy in Calgary; Kryos SRL environmental simulation; "Kyros" variants). None in dev-infra / AI-gateway space. No dominant npm/crate.
- Risk: not a clean global namespace, but our category is clear. SEO will share with the temp-monitoring brand.

### 3. Relyx — VERDICT: minor-friction
- rel + yx; "relay" undertone is perfect for a proxy/gateway. `relyx up`.
- Collision: no software/AI company surfaced. One notable non-tech mark: "RelyX" is a 3M dental cement (registered, but different class entirely). No npm/crate of note.
- Risk: 3M trademark exists in dental; low confusion risk in software, but worth a formal TM check before committing.

### 4. Portix — VERDICT: minor-friction
- "port" = gateway literal + ix. Strong concept fit. `portix`.
- Collision: PORTIX LTD (UK), Portix property-management / logistics software, a GitHub org `portix` exists. None in AI/LLM infra.
- Risk: existing GitHub org `portix` is the main friction (org-name contention). Company-name reuse but in unrelated verticals.

### 5. Voltic — VERDICT: minor-friction
- volt + ic; energy/throughput connotation, clean infra register.
- Collision: Voltic = real e-mobility / EV-fleet software company (LinkedIn presence). Adjacent "Volt/Volta" namespace is busy (volta-cli/volta, @themesberg/volt). Not AI/LLM.
- Risk: live company with the exact name in another vertical; crowded "volt-" prefix family.

### 6. Voxen — VERDICT: collision
- vox (voice/LLM) + en; distinctive.
- Collision: multiple live AI companies — "Voxen AI" (AI for small biz), "VoxenAI" (voice/automation agents), Voxen Tech LLP, Voxen B2B studio. Direct AI-namespace overlap.
- Risk: too many AI brands already named Voxen. Avoid.

### 7. Aptix — VERDICT: collision
- apt + ix; short, crisp.
- Collision: Aptix iPaaS integration platform (Topcon, construction); historical Aptix Corporation (EDA); and — most damaging — `Aptix-Framework` on GitHub, an open-source CLI tool to build AI agents. Direct adjacency to our category.
- Risk: the AI-agent CLI framework collision is disqualifying for an agent-first dev tool.

### 8. Velar — VERDICT: collision
- vel + ar; clean two-syllable, soft-serious.
- Collision: Velar = funded Bitcoin DeFi brand ($3.5M raised, Velar Labs), velar.com taken; also Land Rover "Velar" trim. Strong, active marks.
- Risk: well-capitalized crypto company owns the name and domain. Avoid.

---

## Recommendation

**Queryx** is the only clean result and is also the most on-concept (query/gateway). **Kryos**, **Relyx**, **Portix** are viable minor-friction backups (different verticals, but each carries one specific snag: shared SEO, a dental TM, an existing GitHub org respectively). **Voltic** is borderline (live same-name company elsewhere). **Voxen / Aptix / Velar** should be dropped — each collides inside the AI/agent/infra namespace.
