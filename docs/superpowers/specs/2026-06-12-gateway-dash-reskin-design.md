# Gateway Dashboard Reskin — align to Oximy dashboard design language

**Date:** 2026-06-12
**Branch:** `feat/dash-revamp` (worktree `.worktrees/dash-revamp`)
**Status:** Approved design → ready for implementation plan
**Scope owner file:** `crates/gateway-dash/ui/dist/index.html` (single hand-authored file)

## Summary

Port the Oximy product dashboard's visual language (logo, font roles, color
tokens, shape/radii, component styling) into the Gateway's self-contained
vanilla-JS dashboard. This is a **faithful reskin**: same information
architecture, same 9 surfaces, same layouts, same JS behavior and endpoints.
Only the `<style>` block, the brand/logo markup, and class-level presentation
change.

Source of truth for the target language:
`primary/oximy/apps/dashboard/src/app/globals.css` and
`primary/oximy/apps/dashboard/public/oximy-monogram.svg`.

## Key finding (why this is narrow)

The Gateway is already partially aligned and needs no asset plumbing:

- **Fonts already embedded** in `index.html` as inline base64 woff2:
  Delight (weights 400/500/600/700) + PPMondwest (1 weight). No font files to
  add. Work is **role remapping only**.
- **Palette already matches** on backgrounds: `--bg #FFFAF8`, `--bg1 #FFF5F1`,
  `--bg2 #FFEFEA`, `--card #FFFFFF` are identical to the dashboard's
  `--color-bg-*`. Accent `#FF4D00` matches.

The real gaps: the **logo** (placeholder CSS `<i>O</i>` vs. the real pixel
monogram), **font roles** (PPMondwest mis-wired as the mono face for all
numbers/labels), **shape** (soft 8–12px radii vs. shadcn new-york 2–6px),
and **token deltas** (neutral greys, status colors, a missing viz palette).

## Decisions (locked with user)

1. **Depth:** Faithful reskin. Keep all 9 surfaces and their current layouts.
   No IA/navigation/layout restructuring.
2. **Theme:** Light only. Do **not** port the dashboard's dark theme.
3. **Typography blend:** Match the dashboard's type system, **except keep the
   characterful PPMondwest treatment on prominent stat/headline numbers.**
   Dense tabular numbers use clean monospace for legibility.
4. **Assets:** Keep the single-file approach. Inline the monogram as SVG (and
   as a data-URI favicon). No new runtime files in `ui/dist/`.
5. **Approach A:** Token-layer swap + targeted component retune in place. No JS
   component layer, no `<style>` rewrite from scratch.

## Non-goals

- Dark mode / theme toggle.
- Any change to `VIEWS`, routing, polling, `api()`, or endpoints.
- New API surfaces or write actions (Providers/MCP/Guardrails stay read-only;
  they currently have **no** add-actions, so nothing to hide or label).
- Embedding additional font weights (current 400/500/600/700 are sufficient).
- Restructuring layouts or interactions of any surface.

## Design specification

### 1. Color tokens (`:root`)

Backgrounds and accent stay. Remap greys/status to the dashboard's values and
add the viz palette. Current → target:

| Token (current) | Current value | Target value | Source |
|---|---|---|---|
| `--dim` (text secondary) | `#57514e` | `#525252` | `--color-text-secondary` |
| `--faint` (text tertiary) | `#8a817c` | `#737373` | `--color-text-tertiary` |
| `--ghost` (text muted) | `#b3a8a1` | `#a3a3a3` | `--color-text-muted` |
| `--line` (default border, hairlines) | `#efe7e2` | `#e5e5e5` | `--color-border-default` |
| `--line2` (stronger border) | `#e7ddd6` | `#d4d4d4` | `--color-border-strong` |
| `--line3` (strongest) | `#d9cdc4` | `#d4d4d4` | `--color-border-strong` |

> Note: the gateway's `--line → --line2 → --line3` increase in strength, so both
> stronger tokens map to the dashboard's `--color-border-strong #d4d4d4`. The
> dashboard's lighter `--color-border-subtle #f0f0f0` has no current consumer;
> add it as `--line-subtle` only if a divider needs it.
| `--ok` | `#16a34a` | `#22c55e` (+ `--ok-bg #f0fdf4`) | `--color-success` |
| `--warn` | `#d97706` | `#f59e0b` (+ `--warn-bg #fffbeb`) | `--color-warning` |
| `--err` | `#dc2626` | `#ef4444` (+ `--err-bg #fef2f2`) | `--color-error` |
| `--violet`/`--violet-bg` (cache-HIT chip) | `#9d6f93` | `#a87c9f` (viz-3) | viz palette |

Keep: `--acc #FF4D00`, `--acc2 #E64500`, `--acc-dark #3C1800`,
`--acc-soft #FBF4EB`. Add `--info #3b82f6` / `--info-bg #eff6ff`.

**Add the viz palette** (used by sparkline / barlist / top-models so multi-row
series read as brand, not monochrome orange):

```
--viz-1:#FF4D00; --viz-2:#5778a4; --viz-3:#a87c9f; --viz-4:#85b6b2;
--viz-5:#e7ca60; --viz-6:#6a9f58; --viz-7:#967662;
```

**Shadows:** retune the warm-tinted shadows (`rgba(60,24,0,…)`) to the
dashboard's neutral set (`rgba(0,0,0,…)`): `--sh-sm 0 1px 2px 0 rgb(0 0 0/.05)`,
`--sh 0 4px 6px -1px rgb(0 0 0/.1),0 2px 4px -2px rgb(0 0 0/.1)`. Low-stakes.

**Body background glow:** keep the existing subtle orange radial-gradient mesh
(lines 33–36). It is tasteful and on-brand; the dashboard body is flat but this
does not conflict with the language. (Flip to flat if it reads as too much.)

### 2. Shape / radii (shadcn new-york)

Adopt the tighter radius scale. The new-york look = small corners + squared
badges.

| Element | Current | Target |
|---|---|---|
| `--r` (base) | `8px` | `6px` |
| `--r2` (cards/panels) | `12px` | `8px` |
| nav item | `8px` | `5px` |
| **chips / badges** | pill `20px` | **squared `4px`** (the visible new-york tell) |
| buttons | (varies) | `4px` |
| inputs / select / textarea | (varies) | `4px`, focus ring `0 0 0 2px #FF4D00, 0 0 0 4px rgba(255,77,0,.15)` |

Status "dot" indicators stay circular. Progress tracks/bars keep small radii.

### 3. Typography roles

Fonts already embedded; change the role mapping only.

- `--sans: 'Delight', system-ui, …` → body, nav, buttons. **Unchanged.**
- **NEW `--display: 'PPMondwest', 'Delight', system-ui, sans-serif`** → page
  titles (`#title`/`h2` in `.pagehd`), `h3.sec` section headers, **and the big
  stat numbers** (`.stat .v`, overview/headline figures). This is the "keep
  numbers" treatment.
- **`--mono: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, monospace`**
  (remove `'PPMondwest'` from the mono stack) → uppercase eyebrow labels
  (`.stat .k`, `.navlbl`, table `th`), table data cells, key prefixes/secrets,
  latency/cost/token cells, version string.

Net effect: headline numbers stay characterful (PPMondwest); dense tabular
numbers become clean and legible (true mono) — matching the dashboard while
honoring decision #3.

### 4. Logo / brand

Replace the `.logo` placeholder (`<i>O</i>` + radial gradient + green `.pulse`
dot) with the real **`oximy-monogram.svg`** — a 7×7 pixel grid of `#FF4D00` /
white squares (source:
`primary/oximy/apps/dashboard/public/oximy-monogram.svg`).

- Inline the SVG at ~28px, near-square corners (≤2px).
- Keep the lockup: monogram + "Oximy" (Delight 600) + "GATEWAY" eyebrow (mono,
  letter-spaced `.26em`, accent color) — already present in `.brand`.
- Remove the green pulse dot from the logo; health already shows in the header
  `#health` pill and the `#footstat` footer dot.
- Add the monogram as the favicon via an inline `data:image/svg+xml` `<link>`
  in `<head>` (currently none).

### 5. Component retune (CSS classes, no markup/JS change)

- **Buttons** (`.btn`, `.btn.pri`, `.btn.sm`, `.btn.danger`): primary =
  `#FF4D00` bg / white text, hover `#E64500`; secondary = `--card`/`--bg2` bg +
  `--line` border, hover border `--acc-ln`; radius 4px. Mirrors the dashboard's
  `.cl-formButtonPrimary` / `.cl-button--secondary`.
- **Cards** (`.card`, `.card.stat`): 1px `--line` border, `--sh-sm`, radius 8px.
- **Chips/badges** (`.chip`, `.chip.ok/.warn/.bad/.v`): squared 4px, mono,
  status-subtle bg — `.ok`→`--ok-bg`, `.warn`→`--warn-bg`, `.bad`→`--err-bg`,
  `.v` (cache HIT)→viz-3 text `#a87c9f` on `rgba(168,124,159,.12)`.
- **Tables** (`.tbl`): mono uppercase `th` in `--faint`, sticky header on
  `--bg1`, 1px `--line` row borders. Mostly already conformant — align
  colors/radii only.
- **Inputs / textarea / select / `.searchbox`:** 1px `--line`, radius 4px,
  accent focus ring (above).
- **Modal / scrim** (`.modal`, `.scrim`, `.secretbox`): radius 8px, `--sh-md`.
- **Nav** (`.nav`, `.nav.on`): keep the left accent bar + accent-subtle active
  bg; radius 5px.
- **Dataviz** (`spark()`, `barlist()`): sparkline keeps accent stroke/gradient;
  barlist track fill uses `--acc`; top-models / multi-series rows cycle the viz
  palette.

### 6. Per-surface

All 9 surfaces (`overview, usage, keys, logs, models, providers, mcp,
guardrails, playground`) keep their current `render()` output and layout; they
inherit the retuned tokens and component CSS automatically. No per-surface
markup changes are required beyond optional viz-palette wiring in
`overview` (top-models) and `usage` (barlist).

## Verification

- Build: `cargo build --release --bin oximy-gateway` (shared target cache),
  restart gateway, reload `localhost:8080`.
- Keep test-asserted strings intact: `gateway-dash` tests assert the shell
  contains `"Oximy Gateway"`, `Overview`, `Models`, `Keys`, `Playground`
  (`crates/gateway-dash/src/router.rs`). Run `cargo test -p gateway-dash`.
- Visual check each of the 9 surfaces against the reference screenshots
  (`docs/images/dash-*.png`) and against the product dashboard for language
  parity (logo, squared chips, mono tabular numbers, PPMondwest headlines).

## Risks

- **Grey/shadow cooling:** matching the dashboard shifts warm greys to neutral
  on the cream background. If it reads cold, keep the warm greys (cheap revert,
  isolated to `:root`).
- **PPMondwest legibility:** confirmed limited to headline numbers + headings;
  not used for dense tables.
- **Single-file diff size:** changes are concentrated in `<style>` + `.brand`
  markup; JS untouched, keeping the diff reviewable and low-regression.
