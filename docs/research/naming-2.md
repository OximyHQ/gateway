# Naming Round 2 — Open-Source AI Gateway (LLM + MCP, one binary, agent-first)

Brief: unified, fastest, most comprehensive LLM + MCP gateway. One binary, dashboard included, managed via CLI + its own MCP server. Must sound like serious infrastructure (Envoy, Caddy, Traefik, Vector, Temporal) yet be memorable. Style: short invented/coined words (4-7 letters) + non-English words meaning gate/bridge/junction.

Date: 2026-06-10. Collision checks via WebSearch + DNS availability check.

---

## Full candidate list (30)

### Gate words (non-English)
| Name | Origin / meaning | First take |
|---|---|---|
| **Brana** | Czech *brána* = gate | 5 letters, soft, easy to say; "the gate" literally |
| **Vrata** | Slavic (Czech/Serbian/Slovenian) = gates | Strong sound; collision risk (known) |
| **Torana** | Sanskrit — Indian free-standing gateway arch | Beautiful arch imagery; "Torii" cousin |
| **Torii** | Japanese shrine gate | Iconic, but heavily used in tech already |
| **Brama** | Polish = gate | Given as example in brief; bra-ma rolls fine |
| **Kapu** | Hungarian = gate (also Hawaiian *kapu* = sacred/forbidden — nice access-control double meaning) | 4 letters, very clean sound |
| **Kapija** | Serbian = gate | 6 letters, less obvious pronunciation |
| **Yumen** | 玉门 Jade Gate — the Han-dynasty frontier pass on the Silk Road | Gate + trade-route story is perfect for a gateway |
| **Dvara** | Sanskrit *dvāra* = door/gate | Great meaning; FB used it for a Mongo proxy |
| **Janua** | Latin = door/gate (root of Janus, January) | Classical, infra-serious |
| **Geata** | Irish = gate | Pronunciation ambiguity (GAY-ta/GEH-ta) |
| **Porten** | Danish/Norwegian = "the gate" | Nordic infra vibe (cf. Envoy/Vector tone) |

### Bridge / crossing words
| Name | Origin / meaning | First take |
|---|---|---|
| **Setu** | Sanskrit = bridge | 4 letters, gorgeous; known collision (Setu fintech API co, setu.co) |
| **Pons** | Latin = bridge | 4 letters; also brain structure that relays signals — apt |
| **Silta** | Finnish = bridge | Clean, Nordic, 5 letters |
| **Kopru** | Turkish *köprü* = bridge | Distinctive; diacritic-stripped |
| **Bifrost** | Norse rainbow bridge to Asgard | Memorable but very crowded namespace in OSS |
| **Trestle** | EN, framework bridge | Infra-real; CoreLogic "Trestle" API exists |
| **Bascule** | FR/EN, a counterweighted drawbridge (Tower Bridge) | A bridge that *decides* to open — gating + routing in one image |
| **Pontex** | coined from Latin *pons/pontifex* ("bridge-builder") | Invented but readable |

### Junction / confluence / threshold words
| Name | Origin / meaning | First take |
|---|---|---|
| **Sangam** | Sanskrit/Hindi = confluence of rivers | Many-streams-into-one = LLM + MCP unification |
| **Triveni** | Sanskrit = three-river confluence | 7 letters, melodic; LLM+MCP+dashboard = three streams |
| **Limen** | Latin = threshold | 5 letters; liminal = at the boundary — exactly what a gateway is |
| **Varco** | Italian = passage/opening/checkpoint | Sounds like Envoy-class infra; VARCO LLM collision (known) |
| **Cruce** | Spanish = crossing/junction | 5 letters; pronunciation drift risk (KROO-seh vs "cruise") |
| **Ostia** | Latin *ostium* = river mouth/door; Rome's harbor city | "The port of Rome" for your models — perfect story |

### Coined / structural-engineering words
| Name | Origin / meaning | First take |
|---|---|---|
| **Lintel** | EN, the load-bearing beam over a doorway | Everything passes under it; quietly structural |
| **Gantry** | EN, frame spanning over roads/launchpads | Rocket-launch + toll-gantry imagery; known startup collision |
| **Plenum** | Latin/EN, the air-routing space above ceilings; also "full assembly" | Infra-invisible-but-everywhere; Hyperledger collision (known) |
| **Penstock** | EN, the gate-channel feeding a turbine | All flow, under control, generating power; 8 letters (over budget) |
| **Junctor** | coined from junction + actor | Telephony-switching term originally; robotic feel |

---

## Top 8 — collision check results

### 1. Yumen — "the Jade Gate"
The Silk Road frontier pass: every caravan (request) between empires (models/tools) passed through it. Gate + trade + frontier in one word. 5 letters, YOO-men.
- **Collisions: CLEAN.** Only a dormant GitHub username (`github.com/yumen`, misc Python repos). No company, no product, no npm/pypi package surfaced.
- **Domains: yumen.dev AVAILABLE.** yumen.ai registered.
- Risk: also a city in Gansu, China (fine — cf. Aspen, Denver pattern).

### 2. Brana — Czech for "gate"
Literally "gate" in Czech; 5 letters, friendly to say (BRAH-na), sounds like a sibling of Caddy/Vector.
- **Collisions: NEAR-CLEAN.** Only an empty GitHub org (`github.com/Brana`); no software product, company, or package found.
- **Domains: brana.dev, brana.ai, brana.com all registered** (parked/small). Would need getbrana.com / branagateway.dev style fallback — the main weakness.
- Note: also a Czech surname (Kenneth Branagh adjacency is harmless).

### 3. Bascule — the drawbridge that decides
A counterweighted drawbridge (Tower Bridge is a bascule). A bridge with built-in gating/admission control — semantically the richest match for a gateway that routes AND polices.
- **Collisions: LOW.** A GitHub topic exists; (known from ecosystem: Comcast/xmidt `bascule` is a small Go auth middleware lib — adjacent but obscure). No company or major product surfaced in search.
- **Domains: bascule.dev AVAILABLE.** bascule.ai registered.
- Risk: 7 letters, French spelling — Americans may say "BASK-yool" vs "bas-KYOOL"; acceptable (cf. Traefik survives worse).

### 4. Limen — Latin for "threshold"
The exact point you cross to enter; root of "liminal," "subliminal," "eliminate." 5 letters, scholarly-infra tone like Temporal.
- **Collisions: LOW-MODERATE.** Several small GitHub projects: `solishq/limen` ("Cognitive OS" with an npm `limen-cli` — most concerning, AI-adjacent), `thecodearcher/limen` (Go auth lib, limenauth.dev), a stock tool. No funded company.
- **Domains: limen.dev and limen.ai registered.**
- Verdict: usable but the AI-adjacent solishq project + taken domains make it second-tier.

### 5. Sangam — confluence
Where rivers merge into one stream — the literal pitch of "unified LLM + MCP gateway." Warm, memorable, 6 letters.
- **Collisions: NOT individually searched this round** (budget went to the others). Known prior art: Sangam is a common Indian brand word (Sangam literature, IEEE Sangam conference); expect crowded but unfocused namespace, likely no infra-software incumbent.
- Action before use: run the npm/pypi/GH check.

### 6. Torana — the Indian gateway arch
Free-standing sacred gateway (Sanchi arches); the architectural ancestor of the torii. Distinctive, pan-Asian gate lineage.
- **Collisions: MODERATE.** Torana Inc. (Stamford, CT) — data-quality/ETL-testing software company (makers of iCEDQ), ~170 employees, founded 2005. Data-tooling adjacency is uncomfortably close for an infra product.
- **Domains: torana.dev registered.**

### 7. Ostia — the port of Rome
Latin *ostium* = door/river-mouth; Ostia was the harbor through which everything entered Rome. "All your AI traffic enters through Ostia."
- **Collisions: MODERATE.** `3scale-labs/ostia` — an OpenShift-native API-management experiment by Red Hat's 3scale labs. Archived/unofficial, but it's the SAME category (API gateway), so the ghost is on-topic.
- **Domains: ostia.dev AVAILABLE.**
- Verdict: great story, available .dev; the archived Red Hat lab is a tolerable ghost but would forever share search results.

### 8. Vrata — Slavic for "gates"
Hard, memorable, 5 letters, V-names index well in infra (Vector, Vault, Vite).
- **Collisions: DIRECT.** `PoweredLocal/vrata` — "API gateway implemented in PHP and Lumen", README literally opens "Vrata (Russian for 'gates') is a simple API gateway." Dormant (PHP7-era) with multiple forks, but it claimed the exact name-meaning-category triple.
- **Domains: vrata.dev shows available-ish (registryStatus ambiguous).**
- Verdict: only usable if you're comfortable steamrolling a dead project that had the identical idea.

### Checked and ELIMINATED
- **Varco** — VARCO is NCSOFT's LLM family (VARCO LLM / VARCO 2.0). A name collision with an actual LLM is disqualifying for an LLM gateway.
- **Gantry** — gantry.io, ML-observability startup (Josh Tobin/Vicki Cheung, $28.3M from Amplify/Coatue). Dead-center AI-tooling collision.
- **Plenum** — Hyperledger Indy's BFT consensus protocol is named Plenum; plus a physics collaboration and a design system. Crowded.
- **Lintel** — 4+ OSS projects (CSS framework, FFmpeg video decoder used in ML pipelines, VSCode ESLint extension, HP utility lib) and inevitable "lint tool?" confusion.
- **Dvara** — facebookarchive/dvara is a MongoDB *connection-pooling proxy* (proxy-category collision, archived), plus Dvara Holdings (Indian fintech).

---

## Recommendation

**Ship-list order: 1) Yumen, 2) Bascule, 3) Brana, 4) Ostia.**

- **Yumen** wins on the brief: invented-feeling but real, 5 letters, frontier-gate-of-the-Silk-Road story (gate + commerce + protocol crossing), zero product collisions, **yumen.dev available**. "Yumen Gateway" needs no explanation once the Jade Gate story is told.
- **Bascule** is the dark horse: the only candidate whose metaphor includes *admission control* (a bridge that opens and closes), **bascule.dev available**, near-zero collisions. Slightly harder to spell.
- **Brana** is the purest "Caddy-class" name but all three prime domains are squatted.
- **Ostia** has the best narrative and an open .dev, but shares its name with an archived Red Hat API-gateway experiment.

Pre-registration checklist for the finalist: npm `yumen`, pypi `yumen`, crates.io `yumen`, GitHub org `yumen`/`yumen-gateway`, Docker Hub, USPTO class 9/42 search.
