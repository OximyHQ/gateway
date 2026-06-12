# Gateway Dashboard Reskin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reskin the Gateway's embedded dashboard to the Oximy product dashboard's visual language (real pixel monogram, PPMondwest display + true-mono numbers, new-york radii, neutral greys), with zero IA/layout/JS-behavior change.

**Architecture:** One self-contained file, `crates/gateway-dash/ui/dist/index.html`. All changes are confined to `<head>` (favicon), the inline `<style>` block (lines 14–166), the `.brand` markup (lines 171–173), and one logo string inside the `openKeyGate()` helper. The `VIEWS` registry, router, `api()`, and polling are untouched. Fonts (Delight + PPMondwest) are already embedded as inline woff2 — only their CSS roles change.

**Tech Stack:** Vanilla HTML/CSS/JS embedded via `rust-embed`; Rust/axum server (`oximy-gateway` bin); `cargo` build + `cargo test`.

---

## Reference: the monogram SVG (used in Tasks 2)

Compact, visually identical to `primary/oximy/apps/dashboard/public/oximy-monogram.svg`
(full orange field + 13 white counter cells). Paste verbatim where instructed:

```html
<svg class="mono-mark" viewBox="0 0 91 91" aria-hidden="true" focusable="false"><rect width="90.3704" height="90.3704" fill="#FF4D00"/><g fill="#fff"><rect x="26.7773" y="13.3877" width="10.0412" height="10.0412"/><rect x="26.7773" y="66.9407" width="10.0412" height="10.0412"/><rect x="53.5508" y="13.3877" width="10.0412" height="10.0412"/><rect x="53.5508" y="66.9407" width="10.0412" height="10.0412"/><rect x="13.3906" y="26.7764" width="10.0412" height="10.0412"/><rect x="13.3906" y="40.1643" width="10.0412" height="10.0412"/><rect x="13.3906" y="53.553" width="10.0412" height="10.0412"/><rect x="40.168" y="13.3877" width="10.0412" height="10.0412"/><rect x="40.168" y="40.1643" width="10.0412" height="10.0412"/><rect x="40.168" y="66.9407" width="10.0412" height="10.0412"/><rect x="66.9414" y="26.7764" width="10.0412" height="10.0412"/><rect x="66.9414" y="40.1643" width="10.0412" height="10.0412"/><rect x="66.9414" y="53.553" width="10.0412" height="10.0412"/></g></svg>
```

## Reference: standing verification commands

Used at the end of every task. Run from the worktree root
(`.worktrees/dash-revamp`). `cargo` must be on PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p gateway-dash            # must stay green (guards shell strings)
cargo build --release --bin oximy-gateway   # must compile (re-embeds the HTML)
```

Visual check: open `crates/gateway-dash/ui/dist/index.html` directly in a
browser for pure-layout/chrome/typography/logo/color (data is key-gated and
will be empty — that is expected and fine). For data-bearing views, run the
built binary and load `localhost:8080`. Screenshot with the `browse` skill if
available; otherwise inspect manually.

> **Do not** alter the brand string `Oximy Gateway`, or the view labels
> `Overview`, `Models`, `Keys`, `Playground` — `crates/gateway-dash/src/router.rs`
> asserts their presence in the served shell.

---

## Task 1: Retune the token layer (`:root`)

**Files:**
- Modify: `crates/gateway-dash/ui/dist/index.html` (lines 17–26, the `:root` body)

- [ ] **Step 1: Replace the greys, status, shadow, font, and radius tokens; add viz palette + info + focus ring**

Replace these exact lines (17–26):

```css
  --line:#efe7e2; --line2:#e7ddd6; --line3:#d9cdc4;
  --tx:#1a1a1a; --dim:#57514e; --faint:#8a817c; --ghost:#b3a8a1;
  --acc:#FF4D00; --acc2:#E64500; --acc-dark:#3C1800; --acc-bg:rgba(255,77,0,.08); --acc-ln:rgba(255,77,0,.26); --acc-soft:#FBF4EB;
  --ok:#16a34a; --ok-bg:#f0fdf4; --warn:#d97706; --warn-bg:#fffbeb;
  --err:#dc2626; --err-bg:#fef2f2; --violet:#9d6f93; --violet-bg:#f6eef4;
  --sh-sm:0 1px 2px rgba(60,24,0,.05); --sh:0 2px 8px -2px rgba(60,24,0,.08),0 1px 2px rgba(60,24,0,.04);
  --sh-md:0 8px 24px -6px rgba(60,24,0,.12),0 2px 6px rgba(60,24,0,.05);
  --mono:'PPMondwest',ui-monospace,'SF Mono',Menlo,monospace;
  --sans:'Delight','Hanken Grotesk',system-ui,-apple-system,'Segoe UI',sans-serif;
  --ease:cubic-bezier(.16,1,.3,1); --r:8px; --r2:12px; --sb:248px;
```

with:

```css
  --line:#e5e5e5; --line2:#d4d4d4; --line3:#d4d4d4;
  --tx:#1a1a1a; --dim:#525252; --faint:#737373; --ghost:#a3a3a3;
  --acc:#FF4D00; --acc2:#E64500; --acc-dark:#3C1800; --acc-bg:rgba(255,77,0,.1); --acc-ln:rgba(255,77,0,.26); --acc-soft:#FBF4EB;
  --ok:#22c55e; --ok-bg:#f0fdf4; --warn:#f59e0b; --warn-bg:#fffbeb;
  --err:#ef4444; --err-bg:#fef2f2; --info:#3b82f6; --info-bg:#eff6ff;
  --violet:#a87c9f; --violet-bg:rgba(168,124,159,.12);
  --viz-1:#FF4D00; --viz-2:#5778a4; --viz-3:#a87c9f; --viz-4:#85b6b2; --viz-5:#e7ca60; --viz-6:#6a9f58; --viz-7:#967662;
  --sh-sm:0 1px 2px 0 rgb(0 0 0/.05); --sh:0 4px 6px -1px rgb(0 0 0/.1),0 2px 4px -2px rgb(0 0 0/.1);
  --sh-md:0 10px 15px -3px rgb(0 0 0/.1),0 4px 6px -4px rgb(0 0 0/.1);
  --ring:0 0 0 2px #FF4D00,0 0 0 4px rgba(255,77,0,.15);
  --mono:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,monospace;
  --display:'PPMondwest','Delight',system-ui,sans-serif;
  --sans:'Delight','Hanken Grotesk',system-ui,-apple-system,'Segoe UI',sans-serif;
  --ease:cubic-bezier(.16,1,.3,1); --r:6px; --r2:8px; --sb:248px;
```

Notes for the implementer:
- `--violet`/`--violet-bg` are kept (consumed by the cache-HIT `.chip.v`) but
  recolored to viz-3.
- `--viz-*` and `--info` are added for dashboard token parity. The viz palette
  has no single-metric consumer yet (bar lists are intentionally single-hue);
  do **not** wire per-row colors. Leave the tokens defined for future grouped
  charts.
- `--display` and the rewritten `--mono` are applied in Task 3.

- [ ] **Step 2: Verify build + tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p gateway-dash && cargo build --release --bin oximy-gateway
```
Expected: tests PASS (12), build succeeds.

- [ ] **Step 3: Visual sanity**

Open `index.html`. Borders/greys read slightly cooler/neutral; status greens/oranges unchanged in feel. No layout shift. Logo/fonts unchanged (those are later tasks).

- [ ] **Step 4: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html
git commit -s -m "feat(dash): align color/shadow/radius tokens to dashboard

Neutral greys + dashboard status colors, neutral shadows, new-york
radius scale, viz palette + info + focus-ring tokens, --display role
and true-mono --mono. No visual role changes yet."
```

---

## Task 2: Swap the placeholder logo for the real monogram + add favicon

**Files:**
- Modify: `crates/gateway-dash/ui/dist/index.html` — `<head>` (line 6 area), `.logo` CSS (lines 51–56), `.gate .logo` (line 151), brand markup (line 172), `openKeyGate()` logo string (~line 436)

- [ ] **Step 1: Add the favicon link in `<head>`**

After line 6 (`<title>Oximy Gateway</title>`), insert a new line:

```html
<link rel="icon" type="image/svg+xml" href='data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 91 91"><rect width="91" height="91" fill="%23FF4D00"/><g fill="%23fff"><rect x="26.78" y="13.39" width="10.04" height="10.04"/><rect x="26.78" y="66.94" width="10.04" height="10.04"/><rect x="53.55" y="13.39" width="10.04" height="10.04"/><rect x="53.55" y="66.94" width="10.04" height="10.04"/><rect x="13.39" y="26.78" width="10.04" height="10.04"/><rect x="13.39" y="40.16" width="10.04" height="10.04"/><rect x="13.39" y="53.55" width="10.04" height="10.04"/><rect x="40.17" y="13.39" width="10.04" height="10.04"/><rect x="40.17" y="40.16" width="10.04" height="10.04"/><rect x="40.17" y="66.94" width="10.04" height="10.04"/><rect x="66.94" y="26.78" width="10.04" height="10.04"/><rect x="66.94" y="40.16" width="10.04" height="10.04"/><rect x="66.94" y="53.55" width="10.04" height="10.04"/></g></svg>'>
```
(`#`→`%23`; href wrapped in single quotes so the SVG's double quotes are valid.)

- [ ] **Step 2: Replace the `.logo` CSS rules (lines 51–56)**

Replace exactly:

```css
.logo{width:31px;height:31px;border-radius:9px;position:relative;flex:0 0 auto;
  background:linear-gradient(140deg,#FF6A2B,#FF4D00);box-shadow:0 0 0 1px rgba(255,77,0,.25),0 7px 16px -7px rgba(255,77,0,.7)}
.logo::before{content:"";position:absolute;inset:0;border-radius:9px;background:radial-gradient(circle at 32% 28%,rgba(255,255,255,.55),transparent 48%)}
.logo i{position:absolute;inset:0;display:grid;place-items:center;font-style:normal;font-family:var(--mono);font-size:16px;color:#fff}
.pulse{position:absolute;right:-3px;top:-3px;width:9px;height:9px;border-radius:50%;background:var(--ok);box-shadow:0 0 0 2px #fff;animation:pl 2.6s infinite}
@keyframes pl{0%{box-shadow:0 0 0 2px #fff,0 0 0 2px rgba(22,163,74,.4)}70%{box-shadow:0 0 0 2px #fff,0 0 0 8px rgba(22,163,74,0)}100%{box-shadow:0 0 0 2px #fff,0 0 0 8px rgba(22,163,74,0)}}
```

with:

```css
.logo{width:30px;height:30px;border-radius:3px;flex:0 0 auto;overflow:hidden;line-height:0}
.logo .mono-mark{width:100%;height:100%;display:block}
```

(Removes the gradient placeholder, the `<i>` glyph styling, and the pulse dot + its keyframe — health stays in the header pill + footer dot.)

- [ ] **Step 3: Update `.gate .logo` (line 151)**

Replace:

```css
.gate .logo{width:48px;height:48px;margin:0 auto 18px}.gate .logo i{font-size:24px}
```

with:

```css
.gate .logo{width:48px;height:48px;margin:0 auto 18px}
```

- [ ] **Step 4: Replace the brand markup (line 172)**

Replace:

```html
      <div class="logo"><i>O</i><span class="pulse"></span></div>
```

with (paste the monogram SVG from the Reference section, inside `.logo`):

```html
      <div class="logo"><svg class="mono-mark" viewBox="0 0 91 91" aria-hidden="true" focusable="false"><rect width="90.3704" height="90.3704" fill="#FF4D00"/><g fill="#fff"><rect x="26.7773" y="13.3877" width="10.0412" height="10.0412"/><rect x="26.7773" y="66.9407" width="10.0412" height="10.0412"/><rect x="53.5508" y="13.3877" width="10.0412" height="10.0412"/><rect x="53.5508" y="66.9407" width="10.0412" height="10.0412"/><rect x="13.3906" y="26.7764" width="10.0412" height="10.0412"/><rect x="13.3906" y="40.1643" width="10.0412" height="10.0412"/><rect x="13.3906" y="53.553" width="10.0412" height="10.0412"/><rect x="40.168" y="13.3877" width="10.0412" height="10.0412"/><rect x="40.168" y="40.1643" width="10.0412" height="10.0412"/><rect x="40.168" y="66.9407" width="10.0412" height="10.0412"/><rect x="66.9414" y="26.7764" width="10.0412" height="10.0412"/><rect x="66.9414" y="40.1643" width="10.0412" height="10.0412"/><rect x="66.9414" y="53.553" width="10.0412" height="10.0412"/></g></svg></div>
```

- [ ] **Step 5: Update the gate-modal logo in `openKeyGate()` (~line 436)**

In the `openModal(...)` template string, replace the fragment:

```html
<div class="gate" style="text-align:center"><div class="logo"><i>O</i></div></div>
```

with:

```html
<div class="gate" style="text-align:center"><div class="logo"><svg class="mono-mark" viewBox="0 0 91 91" aria-hidden="true"><rect width="90.3704" height="90.3704" fill="#FF4D00"/><g fill="#fff"><rect x="26.7773" y="13.3877" width="10.0412" height="10.0412"/><rect x="26.7773" y="66.9407" width="10.0412" height="10.0412"/><rect x="53.5508" y="13.3877" width="10.0412" height="10.0412"/><rect x="53.5508" y="66.9407" width="10.0412" height="10.0412"/><rect x="13.3906" y="26.7764" width="10.0412" height="10.0412"/><rect x="13.3906" y="40.1643" width="10.0412" height="10.0412"/><rect x="13.3906" y="53.553" width="10.0412" height="10.0412"/><rect x="40.168" y="13.3877" width="10.0412" height="10.0412"/><rect x="40.168" y="40.1643" width="10.0412" height="10.0412"/><rect x="40.168" y="66.9407" width="10.0412" height="10.0412"/><rect x="66.9414" y="26.7764" width="10.0412" height="10.0412"/><rect x="66.9414" y="40.1643" width="10.0412" height="10.0412"/><rect x="66.9414" y="53.553" width="10.0412" height="10.0412"/></g></svg></div></div>
```

(The template string uses backticks; the SVG's double quotes are safe inside.)

- [ ] **Step 6: Verify build + tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p gateway-dash && cargo build --release --bin oximy-gateway
```
Expected: PASS + build OK.

- [ ] **Step 7: Visual check**

Open `index.html`: sidebar shows the orange pixel "O" monogram (near-square, 3px corners), "Oximy" + "GATEWAY" eyebrow intact, no green pulse dot. Browser tab shows the monogram favicon. The key-gate modal (shown because no key) shows the 48px monogram.

- [ ] **Step 8: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html
git commit -s -m "feat(dash): use real Oximy pixel monogram + favicon

Replace the placeholder gradient O (brand + key-gate) with the
oximy-monogram mark; add an inline-SVG favicon. Drop the logo pulse dot."
```

---

## Task 3: Apply display + mono typography roles

**Files:**
- Modify: `crates/gateway-dash/ui/dist/index.html` — `.num` (line 45), `header h1` (74), `.pagehd h2` (87), `.stat .v` (98), `h3.sec` (105), `.modal h3` (148)

Rationale: after Task 1, `var(--mono)` resolves to true monospace (clean tabular) and `var(--display)` to PPMondwest (characterful). Wire display onto headings + headline stat numbers; wire true-mono onto tabular `.num` cells.

- [ ] **Step 1: Tabular numbers → true mono.** Replace line 45:

```css
.num{font-variant-numeric:tabular-nums;letter-spacing:-.01em}
```
with:
```css
.num{font-family:var(--mono);font-variant-numeric:tabular-nums;letter-spacing:-.01em}
```

- [ ] **Step 2: Header title → display.** Replace line 74:

```css
header h1{font-size:17px;font-weight:600;letter-spacing:-.01em}
```
with:
```css
header h1{font-family:var(--display);font-size:18px;font-weight:600;letter-spacing:0}
```

- [ ] **Step 3: Page heading → display.** Replace line 87:

```css
.pagehd h2{font-size:23px;font-weight:600;letter-spacing:-.02em}
```
with:
```css
.pagehd h2{font-family:var(--display);font-size:24px;font-weight:600;letter-spacing:0}
```

- [ ] **Step 4: Headline stat numbers → display ("keep numbers").** Replace line 98:

```css
.stat .v{font-size:clamp(15px,1.5vw,23px);font-weight:600;letter-spacing:-.02em;margin-top:11px;line-height:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums}
```
with:
```css
.stat .v{font-family:var(--display);font-size:clamp(16px,1.5vw,24px);font-weight:600;letter-spacing:0;margin-top:11px;line-height:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums}
```

- [ ] **Step 5: Section header → display.** Replace line 105:

```css
h3.sec{font-size:13px;font-weight:600;color:var(--tx);margin-bottom:14px;display:flex;align-items:center;gap:10px}
```
with:
```css
h3.sec{font-family:var(--display);font-size:14px;font-weight:600;color:var(--tx);margin-bottom:14px;display:flex;align-items:center;gap:10px}
```

- [ ] **Step 6: Modal title → display.** Replace line 148:

```css
.modal h3{font-size:18px;font-weight:600;margin-bottom:5px}
```
with:
```css
.modal h3{font-family:var(--display);font-size:18px;font-weight:600;margin-bottom:5px}
```

- [ ] **Step 7: Verify build + tests** (standing commands). Expected PASS + build OK.

- [ ] **Step 8: Visual check.** Run the binary (or open file): page titles, section headers, and the big Overview stat numbers render in PPMondwest (distinctive); table/cell numbers (cost, latency, tokens, budgets) render in clean monospace; uppercase eyebrow labels stay mono; body/nav stay Delight.

- [ ] **Step 9: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html
git commit -s -m "feat(dash): PPMondwest display headings + headline numbers, mono tabular

Display role on page/section/modal titles and headline stat numbers;
true-mono on .num tabular cells."
```

---

## Task 4: New-york shape — squared chips + tightened radii

**Files:**
- Modify: `crates/gateway-dash/ui/dist/index.html` — `.nav` (61), `.nav .ct` (67), `.chip` (100), `.cap b` (163), `input/select/textarea` (127), `.btn` (133), `.secretbox` (152), `.modal` (147), `.tst` (156), `.skeleton` (161)

(`.card`, `.tblwrap` already use `var(--r2)` → auto-tightened to 8px by Task 1.)

- [ ] **Step 1: Nav item radius 8→5.** Line 61, change `border-radius:8px` → `border-radius:5px`:

```css
.nav{display:flex;align-items:center;gap:11px;padding:8px 11px;border-radius:5px;color:var(--dim);
```

- [ ] **Step 2: Nav count badge square.** Line 67, change `border-radius:20px` → `border-radius:4px`:

```css
.nav .ct{margin-left:auto;font-family:var(--mono);font-size:12px;color:var(--faint);background:var(--bg2);padding:0 7px;border-radius:4px;min-width:20px;text-align:center}
```

- [ ] **Step 3: Badges squared.** Line 100, change `border-radius:20px` → `border-radius:4px`:

```css
.chip{display:inline-flex;align-items:center;gap:6px;font-family:var(--mono);font-size:12px;padding:2px 9px;border-radius:4px;border:1px solid var(--line2);color:var(--dim);background:var(--bg1);white-space:nowrap}
```

- [ ] **Step 4: Capability badges.** Line 163, change `border-radius:5px` → `border-radius:4px`:

```css
.cap{display:inline-flex;gap:4px}.cap b{font-family:var(--mono);font-size:11px;font-weight:400;padding:0 6px;border-radius:4px;background:var(--bg2);color:var(--faint);border:1px solid var(--line)}
```

- [ ] **Step 5: Inputs 8→4.** Line 127, change `border-radius:8px` → `border-radius:4px`:

```css
input,select,textarea{font-family:var(--sans);font-size:13px;color:var(--tx);background:var(--card);border:1px solid var(--line2);border-radius:4px;padding:9px 12px;width:100%;outline:none;transition:.14s}
```

- [ ] **Step 6: Buttons 8→4.** Line 133, change `border-radius:8px` → `border-radius:4px`:

```css
.btn{display:inline-flex;align-items:center;gap:8px;font-family:var(--sans);font-size:13px;font-weight:600;cursor:pointer;
  padding:9px 16px;border-radius:4px;border:1px solid var(--line2);background:var(--card);color:var(--tx);transition:.14s var(--ease);white-space:nowrap;box-shadow:var(--sh-sm)}
```

- [ ] **Step 7: Secretbox 10→6.** Line 152, change `border-radius:10px` → `border-radius:6px`:

```css
.secretbox{font-family:var(--mono);font-size:14px;background:var(--acc-soft);border:1px solid var(--acc-ln);color:var(--acc-dark);
  padding:13px;border-radius:6px;word-break:break-all;margin:8px 0;display:flex;gap:10px;align-items:center}
```

- [ ] **Step 8: Modal 18→10.** Line 147, change `border-radius:18px` → `border-radius:10px`:

```css
.modal{width:100%;max-width:440px;background:var(--card);border:1px solid var(--line2);border-radius:10px;padding:26px;box-shadow:var(--sh-md);animation:rise .3s both var(--ease)}
```

- [ ] **Step 9: Toast 10→6.** Line 156, change `border-radius:10px` → `border-radius:6px`:

```css
.tst{background:var(--card);border:1px solid var(--line2);border-left:3px solid var(--ok);border-radius:6px;padding:11px 15px;font-size:13px;box-shadow:var(--sh-md);animation:rise .25s both var(--ease);max-width:340px}
```

- [ ] **Step 10: Skeleton 8→6.** Line 161, change `border-radius:8px` → `border-radius:6px`:

```css
.skeleton{background:linear-gradient(90deg,var(--bg1),var(--bg2),var(--bg1));background-size:200% 100%;animation:sk 1.3s infinite;border-radius:6px;color:transparent}
```

- [ ] **Step 11: Verify build + tests** (standing commands). Expected PASS + build OK.

- [ ] **Step 12: Visual check.** Cards/tables/modal corners are tighter (8–10px); table status badges (`active`/`revoked`/`HIT`), nav count badges, and capability badges are squared (4px). Header `.hstat` pills intentionally stay rounded (control chips, not data badges).

- [ ] **Step 13: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html
git commit -s -m "feat(dash): new-york shape — squared badges, tighter radii"
```

---

## Task 5: Component finish — flatter buttons, status/viz chips, focus ring, neutral scrim

**Files:**
- Modify: `crates/gateway-dash/ui/dist/index.html` — `.chip.v` (104), input focus (128), `.btn.pri` (135), `.tbl tr:hover td` (112), `.scrim` (145)

- [ ] **Step 1: Cache-HIT chip → viz-3.** Line 104, replace:

```css
.chip.v{color:var(--violet);border-color:rgba(157,111,147,.28);background:var(--violet-bg)}
```
with:
```css
.chip.v{color:var(--violet);border-color:rgba(168,124,159,.28);background:var(--violet-bg)}
```

- [ ] **Step 2: Input focus ring → dashboard `--ring`.** Line 128, replace:

```css
input:focus,select:focus,textarea:focus{border-color:var(--acc);box-shadow:0 0 0 3px var(--acc-bg)}
```
with:
```css
input:focus,select:focus,textarea:focus{border-color:var(--acc);box-shadow:var(--ring)}
```

- [ ] **Step 3: Flatten primary button shadow.** Line 135, replace:

```css
.btn.pri{background:var(--acc);color:#fff;border-color:transparent;box-shadow:0 6px 16px -7px rgba(255,77,0,.7)}
```
with:
```css
.btn.pri{background:var(--acc);color:#fff;border-color:transparent;box-shadow:var(--sh-sm)}
```

- [ ] **Step 4: Neutralize table-row hover.** Line 112, replace:

```css
.tbl tr:hover td{background:#FFF8F3}
```
with:
```css
.tbl tr:hover td{background:var(--bg1)}
```

- [ ] **Step 5: Neutralize the scrim tint.** Line 145, replace:

```css
.scrim{position:fixed;inset:0;background:rgba(60,24,0,.28);backdrop-filter:blur(5px);z-index:50;display:none;align-items:center;justify-content:center;padding:24px}
```
with:
```css
.scrim{position:fixed;inset:0;background:rgba(0,0,0,.28);backdrop-filter:blur(5px);z-index:50;display:none;align-items:center;justify-content:center;padding:24px}
```

- [ ] **Step 6: Verify build + tests** (standing commands). Expected PASS + build OK.

- [ ] **Step 7: Visual check.** Primary buttons read flatter (no heavy orange glow); input/select/textarea focus shows the two-tone orange ring; modal scrim is neutral; cache-HIT chip is mauve (viz-3); table hover is warm-neutral.

- [ ] **Step 8: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html
git commit -s -m "feat(dash): flatter buttons, dashboard focus ring, viz-3 cache chip, neutral scrim"
```

---

## Task 6: Full-surface verification + changelog

**Files:**
- Modify: `crates/gateway-dash/CHANGELOG.md` if present, else `CHANGELOG.md` at repo root (append an entry)

- [ ] **Step 1: Run the gateway and walk all 9 surfaces.**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo run --release --bin oximy-gateway   # then open localhost:8080, paste admin key
```
Confirm against `docs/images/dash-*.png` + the product dashboard, per surface:
Overview, Usage, Keys, Logs, Models, Providers, MCP, Guardrails, Playground.
Checklist: monogram + favicon present; PPMondwest on titles/section heads/headline stat numbers; clean mono on tabular cells; squared status badges; tight card/table/modal corners; focus ring on Playground inputs + key modal; neutral greys/shadows; no layout regressions; no console errors.

- [ ] **Step 2: Confirm the regression guard.**

```bash
cargo test -p gateway-dash
```
Expected: 12 passing (shell still contains `Oximy Gateway`, `Overview`, `Models`, `Keys`, `Playground`).

- [ ] **Step 3: Add a CHANGELOG entry.** Append under the top/unreleased section:

```markdown
- Dashboard reskin: real Oximy pixel monogram + favicon, PPMondwest display
  headings/headline numbers with monospace tabular figures, shadcn new-york
  shape (squared badges, tighter radii), dashboard-aligned greys/status/shadows
  and focus ring. Light-only; no API/behavior changes.
```

- [ ] **Step 4: Commit.**

```bash
git add -A
git commit -s -m "docs(dash): changelog for dashboard reskin"
```

---

## Self-review (completed by plan author)

- **Spec coverage:** tokens→T1; logo+favicon→T2; typography roles→T3; shape/squared chips→T4; component finish (buttons/chips/focus/scrim)→T5; per-surface verification→T6. Dark mode / IA / new font weights are explicit non-goals (not tasked). Viz palette defined (T1) with a documented no-consumer note — matches the spec's "tokens added for parity."
- **Placeholders:** none — every CSS step shows exact old→new; the monogram SVG and favicon strings are provided verbatim.
- **Naming consistency:** `--display`, `--mono`, `--ring`, `--viz-*`, `--info`, `.mono-mark` are introduced in T1/T2 and referenced consistently in T3/T5. `.logo .mono-mark` selector (T2 CSS) matches the `class="mono-mark"` on the inlined SVGs (T2 markup + gate string).
