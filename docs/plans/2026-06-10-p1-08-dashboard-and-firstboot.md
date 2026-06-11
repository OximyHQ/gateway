# Phase 1.8 — Embedded Dashboard + Zero-Config First Boot — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the last mile that makes Oximy Gateway the thing a developer or agent boots in **one command**: an embedded single-page **dashboard** (`gateway-dash`) — keys (list/create/revoke), usage/spend, request logs (filter/search), model catalog, and a playground — compiled into the binary via `rust-embed` and served by the existing `gateway-control` HTTP server; plus the `oximy-gateway up` command that does **zero-config first boot** (creates the SQLite store + a default admin key on first run, prints it **once**), starts the server, and opens the dashboard in the browser; plus the `cargo-dist` release fan-out and a documented one-command quickstart.

**The dashboard is a strict thin client of the REST API (design §3 `gateway-dash` invariant: "no capability the API lacks").** Every screen calls an endpoint that already exists from P1.4 (keys/models/chat) and P1.7 (logs/spend). This milestone adds **zero** new server *capabilities* — only (a) a static-asset serving layer mounted under the API router, (b) the first-boot bootstrap that seeds the store + admin key, and (c) the CLI/release/docs around it. If a screen needs data the API can't supply, that is a P1.4/P1.7 bug, not a new endpoint here.

**Architecture:** The SPA is a small **Svelte** app (chosen for the smallest static bundle → respects the `<50MB binary` / `<100MB RSS` targets in design §9) built to plain static assets in `crates/gateway-dash/ui/dist/`, embedded at compile time with `rust-embed` (folder absent in a clean checkout → a committed placeholder keeps the crate compiling; CI builds the real bundle before `cargo build`). `gateway-dash` exposes one Rust function — `dash_router()` — returning an `axum::Router` that serves embedded assets with SPA history-fallback (any unknown non-`/v1`, non-`/admin` path returns `index.html`) and correct `Content-Type` + immutable cache headers for fingerprinted assets. The binary mounts it as the **lowest-priority** route so API routes always win. The dashboard talks to the server over the same origin; the admin token is held in the browser (entered once on a login screen, kept in `sessionStorage`) and sent as `Authorization: Bearer …` — **auth-by-default** (design §2): the dashboard is never an unauthenticated bypass of the admin API.

First boot lives in the `oximy-gateway` binary: on `up`, resolve the data dir (`$OXIMY_GATEWAY_DIR` or platform data dir), and if the SQLite file does not exist, call the P1.6 store initializer and mint **one** admin `VirtualKey` (full budget, no model allowlist), print its secret once to stderr with a copy-paste hint, then never again. Then start the P1.4 server (which mounts `dash_router()`), and best-effort open `http://127.0.0.1:<port>/` in the browser.

**Tech Stack:** Rust 2024; `axum` (the server framework P1.4 introduced) + `tower`/`tower-http` (ServeDir-style fallback, but here from embedded bytes); `rust-embed` (compile-time asset embedding); `mime_guess` (content types); `open` (cross-platform browser launch); `gateway-spine` (`VirtualKey`), `gateway-control` (the API router builder + key store), `gateway-config`/P1.6 store init. SPA toolchain: Node + `pnpm` + Vite + Svelte, invoked by a `build.rs`-free **explicit pre-build step** (`pnpm --dir crates/gateway-dash/ui build`) wired into CI and the release pipeline (never into `cargo build` itself — keeps `cargo build` Node-free for Rust-only contributors, who get the committed placeholder bundle). Release: `cargo-dist`.

**Interfaces this milestone CONSUMES (must exist from prior milestones — code against them verbatim):**

- From **P1.4** (`gateway-control`): `gateway_control::api_router(state: ControlState) -> axum::Router` — the admin + `/v1/*` REST router; and `ControlState` (the shared app state: key store, spine handles, registry). P1.4 also owns bearer auth middleware. *If P1.4 named these differently, adapt the import; the seam is "a function that returns the API `Router` and a cloneable state".*
- From **P1.6** (`gateway-config`/persistence): `gateway_config::open_or_init_store(path: &std::path::Path) -> anyhow::Result<Store>` — opens SQLite, runs migrations, returns the handle; `Store::is_freshly_created(&self) -> bool`; and a key-creation path `Store::insert_key(&self, key: &VirtualKey) -> anyhow::Result<()>`.
- From **P1.7** (`gateway-telemetry`): the request-log + spend **read** endpoints (`GET /admin/logs`, `GET /admin/spend`) are already mounted in `api_router`. The dashboard only calls them.

**Interfaces this milestone EXPOSES (later milestones / the binary depend on these):**

- `gateway_dash::dash_router() -> axum::Router` — the embedded-SPA serving router (history-fallback + asset content-types). Mounted **last** by the binary.
- `gateway_dash::INDEX_HTML_PRESENT: bool` and `gateway_dash::asset_count() -> usize` — introspection used by the binary's health/`--version` output and by tests to assert the bundle embedded.
- `oximy_gateway::firstboot::ensure_admin_key(store: &Store, clock: &dyn Clock) -> anyhow::Result<Option<MintedKey>>` — seeds the default admin key iff the store is fresh; returns `Some(MintedKey { secret, key_id })` exactly once (the only time the secret exists in plaintext), `None` if a key already exists. **P2/P3 reuse this for re-bootstrap and the admin-MCP "rotate root key" verb.**
- The `oximy-gateway up` contract (flags `--port`, `--host`, `--dir`, `--no-open`, `--print-key`) — the AXI CLI surface other tooling and the quickstart docs script against.

**Invariants this milestone enforces (design §2, §3):**
- **Auth-by-default** — the dashboard sends the admin bearer token on every API call; static assets are public (they contain no secrets) but every *data* path is the authenticated API. There is no unauthenticated admin surface. Proven by a test that the dash router serves `index.html` but exposes no data route.
- **Secret shown once** — the first-boot admin secret is printed exactly once and only its hash is persisted (reuses the P1.1 `VirtualKey::hash_secret` discipline). Proven by a test that a second `ensure_admin_key` on a non-fresh store returns `None` and never re-derives a secret.
- **Thin client** — the dash router serves only static assets + SPA fallback; it owns no business endpoint. Proven by a test that every non-asset path resolves to `index.html`, never to JSON.

**Explicitly DEFERRED (out of P1 scope, noted where the seam is):** MCP screens (P2), the in-dashboard config diff/apply editor and admin-MCP surface (P3), SSO/multi-user dashboard auth + RBAC (post-P1 commercial line, design §11), guardrail dry-run UI (P4). The login screen here is a single shared admin token, not user accounts.

---

### Task 1: Add serving + embedding + browser-launch dependencies

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/gateway-dash/Cargo.toml`
- Modify: `crates/oximy-gateway/Cargo.toml`

- [ ] **Step 1: Add the shared dep versions to the workspace**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `wiremock = "0.6"` line introduced in P1.2 — if P1.4 already added `axum`/`tower`/`tower-http`, do not duplicate them, just confirm the versions match):

```toml
axum = { version = "0.7", features = ["macros"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }
rust-embed = { version = "8", features = ["mime-guess"] }
mime_guess = "2"
open = "5"
```

- [ ] **Step 2: Reference them from `gateway-dash/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-dash/Cargo.toml` with, and add a `[dev-dependencies]` section:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
axum = { workspace = true }
rust-embed = { workspace = true }
mime_guess = { workspace = true }
gateway-control = { workspace = true }

[dev-dependencies]
tower = { workspace = true }
tokio = { workspace = true }
http-body-util = "0.1"
```

- [ ] **Step 3: Reference `open` from `oximy-gateway/Cargo.toml`**

In `crates/oximy-gateway/Cargo.toml`, add to the `[dependencies]` section (after the existing `gateway-dash = { workspace = true }` line):

```toml
axum = { workspace = true }
open = { workspace = true }
clap = { version = "4", features = ["derive", "env"] }
directories = "5"
```

Also add `clap`/`directories` versions to the root workspace (after the `open = "5"` line):

```toml
clap = { version = "4", features = ["derive", "env"] }
directories = "5"
http-body-util = "0.1"
```

- [ ] **Step 4: Verify it resolves**

Run: `cargo build -p gateway-dash -p oximy-gateway`
Expected: builds (still the scaffold `lib.rs` with the `CRATE` placeholder; binary still prints the scaffold `up` message).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/gateway-dash/Cargo.toml crates/oximy-gateway/Cargo.toml Cargo.lock
git commit -s -m "build(dash): add axum, rust-embed, mime_guess, open, clap deps"
```

---

### Task 2: The committed placeholder UI bundle (cargo build stays Node-free)

The real SPA is built by Node tooling (Task 9). So that `cargo build` works in a clean checkout **without Node**, commit a minimal placeholder `dist/` that `rust-embed` can embed. CI/release overwrite it with the real build before compiling the binary.

**Files:**
- Create: `crates/gateway-dash/ui/dist/index.html`
- Create: `crates/gateway-dash/ui/dist/.gitkeep`
- Create: `crates/gateway-dash/.gitignore`

- [ ] **Step 1: Create the placeholder index**

Create `crates/gateway-dash/ui/dist/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Oximy Gateway</title>
  </head>
  <body>
    <main id="app">
      <h1>Oximy Gateway</h1>
      <p>
        Placeholder dashboard bundle. Build the real SPA with
        <code>pnpm --dir crates/gateway-dash/ui install &amp;&amp; pnpm --dir crates/gateway-dash/ui build</code>.
      </p>
    </main>
  </body>
</html>
```

- [ ] **Step 2: Keep the directory tracked + ignore the real build output's churn**

Create `crates/gateway-dash/ui/dist/.gitkeep` (empty file).

Create `crates/gateway-dash/.gitignore`:

```gitignore
# Node toolchain artifacts; the built dist/ is committed (placeholder) and
# overwritten by CI/release before the Rust build — but never commit node_modules.
ui/node_modules/
ui/.vite/
```

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-dash/ui/dist/index.html crates/gateway-dash/ui/dist/.gitkeep crates/gateway-dash/.gitignore
git commit -s -m "feat(dash): committed placeholder UI bundle so cargo build is Node-free"
```

---

### Task 3: Embed + serve static assets with SPA history-fallback

**Files:**
- Create: `crates/gateway-dash/src/embed.rs`
- Modify: `crates/gateway-dash/src/lib.rs`

- [ ] **Step 1: Write the embedding + asset-resolution core**

Create `crates/gateway-dash/src/embed.rs`:

```rust
//! Compile-time-embedded SPA assets + the rules for serving them. The folder
//! `ui/dist/` is embedded by `rust-embed`; a clean checkout has the committed
//! placeholder bundle, CI/release overwrite it with the real Svelte build. The
//! serving rules (history-fallback, content-type, cache headers) live here so
//! they are unit-testable without spinning up the full server.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

/// `true` iff the embedded bundle contains `index.html` (the SPA shell). The
/// binary surfaces this in `--version`/health so a release built without the UI
/// step fails loudly instead of shipping a blank dashboard.
pub fn index_present() -> bool {
    Assets::get("index.html").is_some()
}

/// Number of embedded files. Used by tests to assert the bundle embedded and by
/// the binary's diagnostics.
pub fn asset_count() -> usize {
    Assets::iter().count()
}

/// A resolved asset ready to serve: its bytes and content type.
pub struct ResolvedAsset {
    pub bytes: std::borrow::Cow<'static, [u8]>,
    pub content_type: String,
    /// `true` for the SPA shell (`index.html`) — served `no-cache` so a new
    /// deploy is picked up; fingerprinted assets are immutable.
    pub is_index: bool,
}

/// Resolve a request path to an asset using SPA rules:
/// - exact asset hit → serve it;
/// - root or any path WITHOUT a file extension → serve `index.html` (client-side
///   routing owns it);
/// - a path WITH an extension that misses → `None` (real 404 for a missing asset).
pub fn resolve(path: &str) -> Option<ResolvedAsset> {
    let trimmed = path.trim_start_matches('/');

    if let Some(file) = Assets::get(trimmed)
        && !trimmed.is_empty()
    {
        let ct = mime_guess::from_path(trimmed)
            .first_or_octet_stream()
            .to_string();
        return Some(ResolvedAsset {
            bytes: file.data,
            content_type: ct,
            is_index: trimmed == "index.html",
        });
    }

    // History fallback: only for "page" paths (no file extension). A missing
    // `.js`/`.css`/`.png` is a genuine 404, not the SPA shell.
    let looks_like_file = trimmed.rsplit('/').next().is_some_and(|seg| seg.contains('.'));
    if looks_like_file {
        return None;
    }

    let index = Assets::get("index.html")?;
    Some(ResolvedAsset {
        bytes: index.data,
        content_type: "text/html".to_string(),
        is_index: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_bundle_is_embedded() {
        assert!(index_present(), "ui/dist/index.html must be embedded");
        assert!(asset_count() >= 1);
    }

    #[test]
    fn root_resolves_to_index() {
        let a = resolve("/").expect("root serves the SPA shell");
        assert!(a.is_index);
        assert_eq!(a.content_type, "text/html");
    }

    #[test]
    fn extensionless_page_path_falls_back_to_index() {
        // Client-side routes like /keys, /usage, /logs are owned by the SPA.
        let a = resolve("/keys").expect("page path serves the shell");
        assert!(a.is_index);
    }

    #[test]
    fn missing_asset_with_extension_is_404() {
        // A fingerprinted asset that isn't in the bundle is a real miss.
        assert!(resolve("/assets/nope-12345.js").is_none());
    }

    #[test]
    fn index_html_is_marked_as_index() {
        let a = resolve("/index.html").expect("index.html serves directly");
        assert!(a.is_index);
        assert_eq!(a.content_type, "text/html");
    }
}
```

- [ ] **Step 2: Wire the module into `lib.rs` (replacing the `CRATE` placeholder)**

Replace the body of `crates/gateway-dash/src/lib.rs` below the doc comment with:

```rust
#![forbid(unsafe_code)]

pub mod embed;
pub mod router;

pub use embed::{asset_count, index_present};
pub use router::dash_router;

/// `true` iff the embedded bundle contains the SPA shell.
pub const fn _assert_links() {}
```

> Note: `router` is created in Task 4; this `lib.rs` will not compile until then. That is expected — run the embed unit tests in isolation in Step 3, then the router in Task 4.

- [ ] **Step 3: Run the embed unit tests (temporarily comment the `router` lines if needed)**

Temporarily, to run only the embed tests before the router exists, set `lib.rs` to:

```rust
#![forbid(unsafe_code)]

pub mod embed;

pub use embed::{asset_count, index_present};
```

Run: `cargo test -p gateway-dash embed::`
Expected: 5 tests PASS (the placeholder bundle is present, fallback rules hold).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-dash --all-targets -- -D warnings
git add crates/gateway-dash/src/embed.rs crates/gateway-dash/src/lib.rs
git commit -s -m "feat(dash): embed SPA assets with history-fallback resolution"
```

---

### Task 4: The `dash_router()` axum router

**Files:**
- Create: `crates/gateway-dash/src/router.rs`
- Modify: `crates/gateway-dash/src/lib.rs`

- [ ] **Step 1: Write the failing router test**

Create `crates/gateway-dash/src/router.rs`:

```rust
//! The embedded-dashboard router: serves static assets and the SPA shell. It is
//! mounted LAST by the binary, under the API router, so `/v1/*` and `/admin/*`
//! always win; only paths the API doesn't claim reach here. This router owns NO
//! data endpoint — that is the `gateway-dash` thin-client invariant (design §3).

use axum::{
    body::Body,
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::embed::{resolve, ResolvedAsset};

/// Build the dashboard router. Two routes: `/` (the shell) and `/{*path}` (any
/// asset or SPA page). Both go through the same `resolve` rules.
pub fn dash_router() -> Router {
    Router::new()
        .route("/", get(serve_root))
        .route("/{*path}", get(serve_path))
}

async fn serve_root() -> Response {
    serve("/")
}

async fn serve_path(Path(path): Path<String>) -> Response {
    serve(&path)
}

fn serve(path: &str) -> Response {
    match resolve(path) {
        Some(asset) => asset_response(asset),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn asset_response(asset: ResolvedAsset) -> Response {
    let cache = if asset.is_index {
        // The shell must never be cached hard, or a new deploy is invisible.
        "no-cache"
    } else {
        // Fingerprinted assets are content-addressed → immutable forever.
        "public, max-age=31536000, immutable"
    };
    let ct = HeaderValue::from_str(&asset.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let body = Body::from(asset.bytes.into_owned());
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, ct),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt; // for `oneshot`

    async fn get_path(path: &str) -> Response {
        dash_router()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn root_serves_html_shell() {
        let res = get_path("/").await;
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
        let cache = res.headers().get(header::CACHE_CONTROL).unwrap();
        assert_eq!(cache, "no-cache");
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Oximy Gateway"));
    }

    #[tokio::test]
    async fn spa_page_path_serves_shell_not_404() {
        // /keys, /usage, /logs are client-side routes → the shell, 200.
        let res = get_path("/keys").await;
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
    }

    #[tokio::test]
    async fn missing_fingerprinted_asset_is_404() {
        let res = get_path("/assets/missing-deadbeef.js").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn router_exposes_no_json_data_route() {
        // Thin-client invariant: even an API-looking path resolves to the shell
        // (the binary mounts the real API ABOVE this router; in isolation the
        // dash router must NEVER answer with data — only the shell or 404).
        let res = get_path("/admin/keys").await;
        // No extension → SPA fallback (shell), never a JSON body from this crate.
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
    }
}
```

- [ ] **Step 2: Restore the full `lib.rs`**

Set `crates/gateway-dash/src/lib.rs` to exactly:

```rust
//! # gateway-dash
//!
//! A thin client of the REST API with no capability the API lacks. Compiled into
//! the binary so a single command boots the gateway and opens the dashboard.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway) — the unified,
//! Apache-2.0 LLM + MCP gateway. See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

pub mod embed;
pub mod router;

pub use embed::{asset_count, index_present};
pub use router::dash_router;
```

- [ ] **Step 3: Run the whole crate's tests**

Run: `cargo test -p gateway-dash`
Expected: 5 embed + 4 router = 9 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-dash --all-targets -- -D warnings
git add crates/gateway-dash/src/router.rs crates/gateway-dash/src/lib.rs
git commit -s -m "feat(dash): dash_router() serving embedded SPA with cache discipline"
```

---

### Task 5: First-boot admin-key bootstrap (seed-once, secret-shown-once)

**Files:**
- Create: `crates/oximy-gateway/src/firstboot.rs`
- Modify: `crates/oximy-gateway/src/main.rs` (add `mod firstboot;`)

This is the **secret-shown-once** invariant. It reuses `gateway_spine::VirtualKey::hash_secret` (P1.1) so only the hash is ever persisted, and the P1.6 store's `is_freshly_created`/`insert_key` (consumed verbatim). The unit test uses an **in-memory fake store** implementing the same trait so this milestone does not depend on SQLite being wired — the real store from P1.6 implements the same `KeyStore` seam.

- [ ] **Step 1: Write the failing test**

Create `crates/oximy-gateway/src/firstboot.rs`:

```rust
//! Zero-config first boot: on a fresh data dir, seed exactly one admin
//! `VirtualKey` (full budget, no model allowlist, no expiry) and return its
//! plaintext secret ONCE — the only moment the secret exists outside the user's
//! clipboard. On a non-fresh store, return `None` and never re-derive a secret.
//!
//! The store is abstracted behind `KeyStore` so this logic is testable without
//! SQLite; the P1.6 persistence store implements the same trait.

use gateway_spine::{Clock, RateLimits, VirtualKey};

/// The minimal store seam first-boot needs. The P1.6 SQLite `Store` implements
/// this (it already has key insert + a freshness flag); tests use a fake.
pub trait KeyStore {
    /// `true` iff the store was created by THIS process (no prior keys).
    fn is_fresh(&self) -> bool;
    /// Persist a key (only the hash + metadata; never the secret).
    fn insert_key(&self, key: &VirtualKey) -> anyhow::Result<()>;
    /// Number of keys currently stored (used to detect an existing root key).
    fn key_count(&self) -> anyhow::Result<usize>;
}

/// The one-time output of a successful seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedKey {
    /// The full plaintext admin secret — shown once, never stored.
    pub secret: String,
    pub key_id: String,
}

/// A cryptographically random admin secret with the `sk-oximy-` prefix.
fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    // 32 bytes of base62-ish entropy.
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let body: String = (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("sk-oximy-{body}")
}

/// Seed the default admin key iff the store has no keys. Idempotent: a second
/// call on a populated store returns `Ok(None)`.
pub fn ensure_admin_key(
    store: &dyn KeyStore,
    clock: &dyn Clock,
) -> anyhow::Result<Option<MintedKey>> {
    if store.key_count()? > 0 {
        return Ok(None);
    }
    let secret = generate_secret();
    let key_id = format!("key_admin_{}", clock.now_ms());
    let token_prefix: String = secret.chars().take(12).collect();
    let key = VirtualKey {
        id: key_id.clone(),
        token_hash: VirtualKey::hash_secret(&secret),
        token_prefix,
        max_budget: None, // admin: unlimited
        limits: RateLimits::default(),
        model_allowlist: None, // admin: all models
        expires_at: None,
        revoked: false,
        parent_id: None,
    };
    store.insert_key(&key)?;
    Ok(Some(MintedKey { secret, key_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::MockClock;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeStore {
        keys: Mutex<Vec<VirtualKey>>,
    }
    impl KeyStore for FakeStore {
        fn is_fresh(&self) -> bool {
            self.keys.lock().unwrap().is_empty()
        }
        fn insert_key(&self, key: &VirtualKey) -> anyhow::Result<()> {
            self.keys.lock().unwrap().push(key.clone());
            Ok(())
        }
        fn key_count(&self) -> anyhow::Result<usize> {
            Ok(self.keys.lock().unwrap().len())
        }
    }

    #[test]
    fn seeds_one_admin_key_on_fresh_store() {
        let store = FakeStore::default();
        let clock = MockClock::new(1_700_000_000_000);
        let minted = ensure_admin_key(&store, &clock).unwrap().expect("fresh store seeds a key");
        assert!(minted.secret.starts_with("sk-oximy-"));
        assert_eq!(store.key_count().unwrap(), 1);

        // The persisted key stores ONLY the hash, never the secret.
        let stored = &store.keys.lock().unwrap()[0];
        assert_ne!(stored.token_hash, minted.secret);
        assert!(stored.verify(&minted.secret), "the minted secret verifies against the hash");
        assert!(stored.max_budget.is_none(), "admin key is unlimited budget");
        assert!(stored.model_allowlist.is_none(), "admin key allows all models");
    }

    #[test]
    fn second_boot_does_not_reseed_or_reveal_secret() {
        let store = FakeStore::default();
        let clock = MockClock::new(1_700_000_000_000);
        let _first = ensure_admin_key(&store, &clock).unwrap().unwrap();
        // Second boot: store already has a key → no new secret.
        let second = ensure_admin_key(&store, &clock).unwrap();
        assert_eq!(second, None, "never re-seeds or re-derives a secret");
        assert_eq!(store.key_count().unwrap(), 1, "no duplicate admin key");
    }

    #[test]
    fn generated_secrets_are_unique() {
        let a = generate_secret();
        let b = generate_secret();
        assert_ne!(a, b);
        assert!(a.len() > 40);
    }
}
```

- [ ] **Step 2: Declare the module in `main.rs`**

In `crates/oximy-gateway/src/main.rs`, add directly under the `#![forbid(unsafe_code)]` line:

```rust
mod firstboot;
```

Also add `rand` to `crates/oximy-gateway/Cargo.toml` `[dependencies]` (after the `directories = "5"` line added in Task 1):

```toml
rand = { workspace = true }
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p oximy-gateway firstboot::`
Expected: 3 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p oximy-gateway --all-targets -- -D warnings
git add crates/oximy-gateway/src/firstboot.rs crates/oximy-gateway/src/main.rs crates/oximy-gateway/Cargo.toml Cargo.lock
git commit -s -m "feat(gateway): first-boot admin-key bootstrap (seed-once, secret-shown-once)"
```

---

### Task 6: Data-dir resolution + the `up` CLI surface (clap)

**Files:**
- Create: `crates/oximy-gateway/src/cli.rs`
- Modify: `crates/oximy-gateway/src/main.rs`

Replace the hand-rolled arg parsing with a `clap` derive surface giving `up` its flags (`--port`, `--host`, `--dir`, `--no-open`, `--print-key`) and a stable data-dir resolution. AXI discipline (design §7): `--json` reserved, semantic exit codes, definitive next-step hints.

- [ ] **Step 1: Write the failing test**

Create `crates/oximy-gateway/src/cli.rs`:

```rust
//! The CLI surface. `up` is the zero-config command. Data-dir resolution:
//! `--dir` flag > `$OXIMY_GATEWAY_DIR` > the platform data dir
//! (`~/.local/share/oximy-gateway` on Linux, `~/Library/Application Support/...`
//! on macOS). The SQLite file lives at `<dir>/gateway.db`.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "oximy-gateway", version, about = "Unified LLM + MCP gateway")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Boot the gateway and open the dashboard (zero-config on first run).
    Up(UpArgs),
    /// Print the version.
    Version,
}

#[derive(Debug, clap::Args, Clone)]
pub struct UpArgs {
    /// Port to bind the server (default 4141).
    #[arg(long, env = "OXIMY_GATEWAY_PORT", default_value_t = 4141)]
    pub port: u16,
    /// Host/interface to bind (default 127.0.0.1 — local only by default).
    #[arg(long, env = "OXIMY_GATEWAY_HOST", default_value = "127.0.0.1")]
    pub host: String,
    /// Data directory (overrides $OXIMY_GATEWAY_DIR and the platform default).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// Do not open the dashboard in a browser.
    #[arg(long)]
    pub no_open: bool,
    /// Re-print the admin key prefix info even when not freshly seeded (never
    /// re-reveals the secret — only confirms a key exists).
    #[arg(long)]
    pub print_key: bool,
}

/// Resolve the data directory: `--dir` > `$OXIMY_GATEWAY_DIR` > platform default.
pub fn resolve_data_dir(flag: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("OXIMY_GATEWAY_DIR")
        && !env.is_empty()
    {
        return Ok(PathBuf::from(env));
    }
    let proj = directories::ProjectDirs::from("com", "oximy", "oximy-gateway")
        .ok_or_else(|| anyhow::anyhow!("could not resolve a platform data directory"))?;
    Ok(proj.data_dir().to_path_buf())
}

/// The SQLite file path inside a data dir.
pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_dir_flag_wins() {
        let p = PathBuf::from("/tmp/explicit");
        let resolved = resolve_data_dir(Some(&p)).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn db_path_is_under_data_dir() {
        let dir = PathBuf::from("/var/lib/oximy");
        assert_eq!(db_path(&dir), PathBuf::from("/var/lib/oximy/gateway.db"));
    }

    #[test]
    fn up_args_default_port_and_host() {
        let cli = Cli::parse_from(["oximy-gateway", "up"]);
        match cli.command {
            Command::Up(args) => {
                assert_eq!(args.port, 4141);
                assert_eq!(args.host, "127.0.0.1");
                assert!(!args.no_open);
            }
            _ => panic!("expected up"),
        }
    }

    #[test]
    fn up_flags_parse() {
        let cli = Cli::parse_from([
            "oximy-gateway", "up", "--port", "8080", "--host", "0.0.0.0", "--no-open",
        ]);
        match cli.command {
            Command::Up(args) => {
                assert_eq!(args.port, 8080);
                assert_eq!(args.host, "0.0.0.0");
                assert!(args.no_open);
            }
            _ => panic!("expected up"),
        }
    }
}
```

- [ ] **Step 2: Declare the module in `main.rs`**

In `crates/oximy-gateway/src/main.rs`, add under the existing `mod firstboot;` line:

```rust
mod cli;
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p oximy-gateway cli::`
Expected: 4 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p oximy-gateway --all-targets -- -D warnings
git add crates/oximy-gateway/src/cli.rs crates/oximy-gateway/src/main.rs
git commit -s -m "feat(gateway): clap up CLI surface + data-dir resolution"
```

---

### Task 7: Wire `up` — boot store, seed key, start server, mount dashboard, open browser

**Files:**
- Modify: `crates/oximy-gateway/src/main.rs`

This is the integration point. It assembles: P1.6 store init → first-boot seed (Task 5) → P1.4 `api_router` (the API) → mount `gateway_dash::dash_router()` **last** (fallback) → bind + serve → open the browser. The P1.4/P1.6 calls are behind the seams documented in the header; if a name differs, adapt the call, not the structure.

- [ ] **Step 1: Replace `main.rs` with the wired `up` path**

Set `crates/oximy-gateway/src/main.rs` to:

```rust
//! # Oximy Gateway
//!
//! The unified, fastest, open-source LLM + MCP gateway. Single static binary,
//! embedded dashboard, agent-first control plane (CLI + admin-MCP + config-as-code).
//!
//! `oximy-gateway up` boots the gateway and opens the dashboard.
//!
//! See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

mod firstboot;
mod cli;

use std::process::ExitCode;

use axum::Router;
use clap::Parser;
use gateway_spine::SystemClock;

use cli::{Cli, Command, UpArgs};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("oximy-gateway {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Up(args) => match run_up(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("oximy-gateway up failed: {e:#}");
                // Semantic exit code: runtime failure (not a usage error).
                ExitCode::from(70)
            }
        },
    }
}

/// Boot the gateway: open/init the store, seed the admin key on first run, start
/// the HTTP server with the dashboard mounted, and open the browser.
fn run_up(args: UpArgs) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_up_async(args))
}

async fn run_up_async(args: UpArgs) -> anyhow::Result<()> {
    let data_dir = cli::resolve_data_dir(args.dir.as_deref())?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = cli::db_path(&data_dir);
    tracing::info!(dir = %data_dir.display(), "data directory");

    // --- P1.6 seam: open or initialize the persistent store. ---
    // `open_or_init_store` runs migrations and returns a handle implementing the
    // control-plane `KeyStore` (and the first-boot `firstboot::KeyStore`) traits.
    let store = gateway_config::open_or_init_store(&db_path)?;

    // --- First boot: seed the admin key once. ---
    let clock = SystemClock;
    if let Some(minted) = firstboot::ensure_admin_key(&store, &clock)? {
        print_minted_key(&minted);
    } else if args.print_key {
        eprintln!(
            "An admin key already exists for this data dir ({}). The secret is \
             never recoverable; rotate it from the dashboard or `keys` CLI if lost.",
            data_dir.display()
        );
    }

    // --- P1.4 seam: build the admin + /v1 API router over the shared state. ---
    let state = gateway_control::ControlState::from_store(store);
    let api: Router = gateway_control::api_router(state);

    // Mount the dashboard LAST so /v1/* and /admin/* always win; only paths the
    // API doesn't claim fall through to the embedded SPA.
    let app = api.merge(gateway_dash::dash_router());

    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let url = format!("http://{}:{}/", display_host(&args.host), args.port);
    tracing::info!(%url, assets = gateway_dash::asset_count(), "dashboard ready");
    eprintln!("\n  Oximy Gateway is running.\n  Dashboard:  {url}\n  API base:   {url}v1\n");

    if !args.no_open && gateway_dash::index_present() {
        // Best-effort; a headless box simply won't open anything.
        let _ = open::that(&url);
    }

    axum::serve(listener, app).await?;
    Ok(())
}

/// Print the freshly minted admin secret exactly once.
fn print_minted_key(minted: &firstboot::MintedKey) {
    eprintln!(
        "\n  ┌─ First boot ─────────────────────────────────────────────\n\
         \x20 │  A default admin key was created. It is shown ONCE:\n\
         \x20 │\n\
         \x20 │     {}\n\
         \x20 │\n\
         \x20 │  Use it as your Bearer token for the API and dashboard.\n\
         \x20 │  Store it now — it cannot be recovered.\n\
         \x20 └──────────────────────────────────────────────────────────\n",
        minted.secret
    );
}

/// For display, show 127.0.0.1 even when bound to 0.0.0.0 (the reachable local URL).
fn display_host(host: &str) -> String {
    if host == "0.0.0.0" { "127.0.0.1".to_string() } else { host.to_string() }
}
```

- [ ] **Step 2: Build (compilation is the gate; live serve is exercised in Task 8's smoke test)**

Run: `cargo build -p oximy-gateway`
Expected: builds, **provided P1.4/P1.6 supplied `gateway_control::api_router`/`ControlState::from_store` and `gateway_config::open_or_init_store`**. If those exact names differ in the landed P1.4/P1.6, adjust the three call sites (store open, state construct, router build) — the wiring shape is fixed, the symbol names follow the landed crates.

> If P1.4/P1.6 are not yet merged when this task runs, stub the three seam calls behind a `#[cfg(feature = "firstboot-stub")]` returning an empty `Router::new()` and a fake store, land Tasks 1–6 + 8–11, and revisit this wiring in the integration pass. Do **not** invent new API endpoints to make it compile — that violates the thin-client invariant.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p oximy-gateway --all-targets -- -D warnings
git add crates/oximy-gateway/src/main.rs
git commit -s -m "feat(gateway): wire `up` — store init, key seed, server + dashboard, open browser"
```

---

### Task 8: End-to-end serve smoke test (dashboard reachable under the API)

**Files:**
- Create: `crates/gateway-dash/tests/serve_smoke.rs`

Prove the merged router serves the dashboard at `/` and a SPA page at `/keys`, and that a co-mounted API route wins over the fallback — the **mount-order invariant** the binary relies on. This lives in `gateway-dash` so it has no P1.4 dependency: it builds a stand-in API router with a single `/admin/ping` route and merges the dash router exactly as the binary does.

- [ ] **Step 1: Write the test**

Create `crates/gateway-dash/tests/serve_smoke.rs`:

```rust
//! Integration: the dashboard router, merged under a stand-in API router exactly
//! as `oximy-gateway up` does, serves the SPA shell on page paths and yields to
//! API routes mounted above it (mount-order invariant).

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

fn app() -> Router {
    // Stand-in for the P1.4 api_router: one authenticated-ish admin route.
    let api = Router::new().route("/admin/ping", get(|| async { "pong" }));
    // Same merge order the binary uses: API first, dashboard last.
    api.merge(gateway_dash::dash_router())
}

#[tokio::test]
async fn dashboard_shell_is_served_at_root() {
    let res = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Oximy Gateway"));
}

#[tokio::test]
async fn spa_page_path_serves_shell() {
    let res = app()
        .oneshot(Request::builder().uri("/usage").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_route_wins_over_dashboard_fallback() {
    let res = app()
        .oneshot(Request::builder().uri("/admin/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    assert_eq!(&body[..], b"pong", "the API route must win, not the SPA shell");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p gateway-dash --test serve_smoke`
Expected: 3 tests PASS — the dashboard serves, and the API route wins over the SPA fallback.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-dash --all-targets -- -D warnings
git add crates/gateway-dash/tests/serve_smoke.rs
git commit -s -m "test(dash): merged-router serve smoke + mount-order invariant"
```

---

### Task 9: The Svelte SPA (thin client of the REST API)

**Files:**
- Create: `crates/gateway-dash/ui/package.json`
- Create: `crates/gateway-dash/ui/vite.config.ts`
- Create: `crates/gateway-dash/ui/index.html`
- Create: `crates/gateway-dash/ui/src/main.ts`
- Create: `crates/gateway-dash/ui/src/App.svelte`
- Create: `crates/gateway-dash/ui/src/lib/api.ts`
- Create: `crates/gateway-dash/ui/src/lib/auth.ts`
- Create: `crates/gateway-dash/ui/src/routes/{Keys,Usage,Logs,Models,Playground}.svelte`

The SPA is the only non-Rust code in this milestone. It is a thin client: every screen is a `fetch` against an endpoint that exists from P1.4/P1.7, with the admin bearer token from `sessionStorage`. Keep it small (design §9 binary-size budget).

> **Build contract:** `pnpm --dir crates/gateway-dash/ui install && pnpm --dir crates/gateway-dash/ui build` emits fingerprinted assets into `crates/gateway-dash/ui/dist/`, overwriting the placeholder. `cargo build` then embeds them. CI runs the pnpm build **before** `cargo build`; Rust-only contributors get the placeholder.

- [ ] **Step 1: `package.json` + Vite + Svelte config**

Create `crates/gateway-dash/ui/package.json`:

```json
{
  "name": "oximy-gateway-dash",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "check": "svelte-check --tsconfig ./tsconfig.json"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^4",
    "svelte": "^5",
    "svelte-check": "^4",
    "typescript": "^5",
    "vite": "^6"
  }
}
```

Create `crates/gateway-dash/ui/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Build to ../dist so rust-embed picks it up. Fingerprint assets for immutable
// caching; keep the bundle lean (the binary-size budget is real, design §9).
export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    assetsDir: "assets",
    target: "es2022",
  },
  // In dev, proxy API calls to a locally running `oximy-gateway up`.
  server: {
    proxy: {
      "/v1": "http://127.0.0.1:4141",
      "/admin": "http://127.0.0.1:4141",
    },
  },
});
```

Create `crates/gateway-dash/ui/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Oximy Gateway</title>
  </head>
  <body>
    <main id="app"></main>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 2: Auth + API client (the thin-client core)**

Create `crates/gateway-dash/ui/src/lib/auth.ts`:

```ts
// The admin bearer token, entered once on the login screen and kept in
// sessionStorage. Auth-by-default: every API call carries it; there is no
// unauthenticated admin path.
const KEY = "oximy_admin_token";

export function getToken(): string | null {
  return sessionStorage.getItem(KEY);
}

export function setToken(token: string): void {
  sessionStorage.setItem(KEY, token);
}

export function clearToken(): void {
  sessionStorage.removeItem(KEY);
}
```

Create `crates/gateway-dash/ui/src/lib/api.ts`:

```ts
import { getToken, clearToken } from "./auth";

// Thin client: one fetch wrapper, every screen calls an endpoint that already
// exists from P1.4 (keys/models) and P1.7 (logs/spend). No screen invents data.
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const token = getToken();
  const res = await fetch(path, {
    method,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (res.status === 401) {
    clearToken();
    throw new Error("unauthorized");
  }
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status}`);
  return (await res.json()) as T;
}

export interface KeySummary {
  id: string;
  token_prefix: string;
  max_budget_usd: number | null;
  revoked: boolean;
}

// Endpoint contract (owned by P1.4 / P1.7 — this client must NOT exceed it):
export const api = {
  // Keys (P1.4 admin API)
  listKeys: () => call<KeySummary[]>("GET", "/admin/keys"),
  createKey: (b: { name?: string; max_budget_usd?: number; models?: string[] }) =>
    call<{ id: string; secret: string }>("POST", "/admin/keys", b),
  revokeKey: (id: string) => call<void>("POST", `/admin/keys/${id}/revoke`),
  // Spend / usage (P1.7)
  spend: () => call<{ key_id: string; spent_usd: number }[]>("GET", "/admin/spend"),
  // Request logs (P1.7) — filter/search params passed as query string
  logs: (q: string) => call<unknown[]>("GET", `/admin/logs${q}`),
  // Model catalog (P1.4 — the machine-readable /v1/models)
  models: () => call<{ id: string; provider: string }[]>("GET", "/v1/models"),
  // Playground proxies the user's own request through the gateway itself.
  chat: (b: unknown) => call<unknown>("POST", "/v1/chat/completions", b),
};
```

- [ ] **Step 3: Shell + screens**

Create `crates/gateway-dash/ui/src/main.ts`:

```ts
import App from "./App.svelte";
import { mount } from "svelte";

const app = mount(App, { target: document.getElementById("app")! });
export default app;
```

Create `crates/gateway-dash/ui/src/App.svelte`:

```svelte
<script lang="ts">
  import { getToken, setToken } from "./lib/auth";
  import Keys from "./routes/Keys.svelte";
  import Usage from "./routes/Usage.svelte";
  import Logs from "./routes/Logs.svelte";
  import Models from "./routes/Models.svelte";
  import Playground from "./routes/Playground.svelte";

  // Hash-based routing keeps history-fallback trivial (no server route map).
  let route = $state(location.hash.slice(1) || "keys");
  window.addEventListener("hashchange", () => (route = location.hash.slice(1) || "keys"));

  let token = $state(getToken());
  let tokenInput = $state("");
  function login() {
    setToken(tokenInput.trim());
    token = getToken();
  }

  const tabs = ["keys", "usage", "logs", "models", "playground"];
</script>

{#if !token}
  <section class="login">
    <h1>Oximy Gateway</h1>
    <p>Paste your admin key (shown once on first boot).</p>
    <input type="password" bind:value={tokenInput} placeholder="sk-oximy-…" />
    <button onclick={login}>Continue</button>
  </section>
{:else}
  <nav>
    {#each tabs as t}
      <a href={`#${t}`} class:active={route === t}>{t}</a>
    {/each}
  </nav>
  <main>
    {#if route === "keys"}<Keys />
    {:else if route === "usage"}<Usage />
    {:else if route === "logs"}<Logs />
    {:else if route === "models"}<Models />
    {:else if route === "playground"}<Playground />
    {/if}
  </main>
{/if}
```

Create `crates/gateway-dash/ui/src/routes/Keys.svelte`:

```svelte
<script lang="ts">
  import { api, type KeySummary } from "../lib/api";

  let keys = $state<KeySummary[]>([]);
  let error = $state("");
  let newSecret = $state("");

  async function load() {
    try {
      keys = await api.listKeys();
    } catch (e) {
      error = String(e);
    }
  }
  async function create() {
    const res = await api.createKey({ name: "dashboard-created" });
    newSecret = res.secret; // shown once
    await load();
  }
  async function revoke(id: string) {
    await api.revokeKey(id);
    await load();
  }
  load();
</script>

<h2>Keys</h2>
{#if error}<p class="error">{error}</p>{/if}
<button onclick={create}>Create key</button>
{#if newSecret}
  <p class="secret">New key (copy now, shown once): <code>{newSecret}</code></p>
{/if}
<ul>
  {#each keys as k}
    <li>
      <code>{k.token_prefix}…</code>
      {k.max_budget_usd === null ? "unlimited" : `$${k.max_budget_usd}`}
      {#if k.revoked}<span class="revoked">revoked</span>
      {:else}<button onclick={() => revoke(k.id)}>Revoke</button>{/if}
    </li>
  {/each}
</ul>
```

Create `crates/gateway-dash/ui/src/routes/Usage.svelte`:

```svelte
<script lang="ts">
  import { api } from "../lib/api";
  let rows = $state<{ key_id: string; spent_usd: number }[]>([]);
  api.spend().then((r) => (rows = r)).catch(() => (rows = []));
</script>

<h2>Usage &amp; spend</h2>
<table>
  <thead><tr><th>Key</th><th>Spent (USD)</th></tr></thead>
  <tbody>
    {#each rows as r}<tr><td>{r.key_id}</td><td>${r.spent_usd.toFixed(4)}</td></tr>{/each}
  </tbody>
</table>
```

Create `crates/gateway-dash/ui/src/routes/Logs.svelte`:

```svelte
<script lang="ts">
  import { api } from "../lib/api";
  let q = $state("");
  let rows = $state<unknown[]>([]);
  async function search() {
    const qs = q ? `?q=${encodeURIComponent(q)}` : "";
    rows = await api.logs(qs);
  }
  search();
</script>

<h2>Request logs</h2>
<input bind:value={q} placeholder="filter (model, key, status)…" />
<button onclick={search}>Search</button>
<pre>{JSON.stringify(rows, null, 2)}</pre>
```

Create `crates/gateway-dash/ui/src/routes/Models.svelte`:

```svelte
<script lang="ts">
  import { api } from "../lib/api";
  let models = $state<{ id: string; provider: string }[]>([]);
  api.models().then((m) => (models = m)).catch(() => (models = []));
</script>

<h2>Model catalog</h2>
<ul>{#each models as m}<li><code>{m.id}</code> — {m.provider}</li>{/each}</ul>
```

Create `crates/gateway-dash/ui/src/routes/Playground.svelte`:

```svelte
<script lang="ts">
  import { api } from "../lib/api";
  let model = $state("gpt-4o");
  let prompt = $state("Say hello in one word.");
  let out = $state("");
  async function run() {
    out = "…";
    const res = await api.chat({
      model,
      messages: [{ role: "user", content: prompt }],
    });
    out = JSON.stringify(res, null, 2);
  }
</script>

<h2>Playground</h2>
<input bind:value={model} />
<textarea bind:value={prompt}></textarea>
<button onclick={run}>Run</button>
<pre>{out}</pre>
```

- [ ] **Step 2: Build the bundle (this is a manual/CI step, NOT cargo)**

Run:
```bash
pnpm --dir crates/gateway-dash/ui install
pnpm --dir crates/gateway-dash/ui build
```
Expected: `crates/gateway-dash/ui/dist/` now holds `index.html` + fingerprinted `assets/*.js`/`*.css`, replacing the placeholder.

- [ ] **Step 3: Re-run the Rust serve test against the real bundle**

Run: `cargo test -p gateway-dash`
Expected: still green — `asset_count()` is now > 1 (fingerprinted assets), the shell still contains "Oximy Gateway", page paths still fall back to it.

- [ ] **Step 4: Commit the SPA source (and the rebuilt dist)**

```bash
git add crates/gateway-dash/ui/package.json crates/gateway-dash/ui/vite.config.ts \
        crates/gateway-dash/ui/index.html crates/gateway-dash/ui/src crates/gateway-dash/ui/dist
git commit -s -m "feat(dash): Svelte SPA — keys/usage/logs/models/playground thin client"
```

---

### Task 10: cargo-dist release config (one-tag fan-out)

**Files:**
- Create: `dist-workspace.toml` (or `[workspace.metadata.dist]` in root `Cargo.toml`)
- Create: `.github/workflows/release.yml` (generated by `dist init`, with the UI pre-build wired in)

design §11 calls for **cargo-dist** one-tag fan-out (brew/deb/rpm/Docker/installer) and §9 the `<50MB binary` budget. The crucial twist: the release must run the **pnpm UI build before the cargo build** so the real dashboard is embedded, not the placeholder.

- [ ] **Step 1: Initialize cargo-dist**

Run (this scaffolds config + the release workflow):
```bash
cargo install cargo-dist --locked
dist init --yes
```
Accept: installers = `shell` + `homebrew`; targets = `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`. Only the `oximy-gateway` binary is published.

- [ ] **Step 2: Pin the published binary + add the UI pre-build hook**

In the generated `dist-workspace.toml` (or `[workspace.metadata.dist]`), ensure:

```toml
[dist]
cargo-dist-version = "0.22.0"
installers = ["shell", "homebrew"]
targets = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]
install-path = "CARGO_HOME"
# Build the dashboard bundle before cargo builds the binary that embeds it.
github-build-setup = "./.github/build-setup.yml"
```

Create `.github/build-setup.yml` (steps injected into every release build job, before `cargo build`):

```yaml
- name: Install pnpm
  uses: pnpm/action-setup@v4
  with:
    version: 9
- name: Set up Node
  uses: actions/setup-node@v4
  with:
    node-version: 22
    cache: pnpm
    cache-dependency-path: crates/gateway-dash/ui/pnpm-lock.yaml
- name: Build dashboard bundle
  run: |
    pnpm --dir crates/gateway-dash/ui install --frozen-lockfile
    pnpm --dir crates/gateway-dash/ui build
- name: Verify dashboard embedded
  run: test -f crates/gateway-dash/ui/dist/index.html
```

- [ ] **Step 3: Generate the release workflow and verify the plan**

Run: `dist plan`
Expected: prints the artifact matrix (5 targets × archives + shell/homebrew installers) with no errors.

- [ ] **Step 4: Commit**

```bash
git add dist-workspace.toml .github/workflows/release.yml .github/build-setup.yml Cargo.toml
git commit -s -m "build(release): cargo-dist fan-out with dashboard pre-build step"
```

---

### Task 11: One-command quickstart docs

**Files:**
- Create: `docs/quickstart.md`
- Modify: `README.md` (add the quickstart section + a top-of-file pointer)

design §10/§11: "the thing a developer or agent boots in one command." Document it so the README delivers the promise.

- [ ] **Step 1: Write the quickstart**

Create `docs/quickstart.md`:

```markdown
# Oximy Gateway — Quickstart

Run the unified LLM + MCP gateway in one command. Zero config: the first boot
creates a local store and a single admin key, prints it once, and opens the
dashboard.

## Install

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/oximyhq/gateway/releases/latest/download/oximy-gateway-installer.sh | sh
# or: brew install oximyhq/tap/oximy-gateway
```

## Boot

```bash
oximy-gateway up
```

On first run you'll see:

```
  ┌─ First boot ─────────────────────────────────────────────
  │  A default admin key was created. It is shown ONCE:
  │
  │     sk-oximy-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
  │
  │  Use it as your Bearer token for the API and dashboard.
  │  Store it now — it cannot be recovered.
  └──────────────────────────────────────────────────────────

  Oximy Gateway is running.
  Dashboard:  http://127.0.0.1:4141/
  API base:   http://127.0.0.1:4141/v1
```

The dashboard opens automatically. Paste the admin key to sign in.

## Use it from any OpenAI client

```bash
curl http://127.0.0.1:4141/v1/chat/completions \
  -H "Authorization: Bearer sk-oximy-…" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}]}'
```

Point any OpenAI-compatible SDK at `http://127.0.0.1:4141/v1` with the admin key.

## Flags

| Flag | Default | Meaning |
|---|---|---|
| `--port` | `4141` | Bind port (env `OXIMY_GATEWAY_PORT`) |
| `--host` | `127.0.0.1` | Bind host (env `OXIMY_GATEWAY_HOST`) |
| `--dir` | platform data dir | Data directory (env `OXIMY_GATEWAY_DIR`); store at `<dir>/gateway.db` |
| `--no-open` | off | Don't open the browser (servers/CI) |
| `--print-key` | off | Confirm an admin key exists (never re-reveals the secret) |

## Lost the admin key?

The secret is never recoverable (only its hash is stored). Rotate it from the
dashboard or `oximy-gateway keys` (CLI lands with the control plane).
```

- [ ] **Step 2: Add the quickstart to the README**

Add to `README.md` (a `## Quickstart` section near the top, linking to `docs/quickstart.md`):

```markdown
## Quickstart

```bash
oximy-gateway up
```

Zero-config: first boot creates a local store + a one-time admin key and opens
the dashboard. Full walkthrough → [docs/quickstart.md](docs/quickstart.md).
```

- [ ] **Step 3: Commit**

```bash
git add docs/quickstart.md README.md
git commit -s -m "docs: one-command quickstart for `oximy-gateway up`"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-dash` is fully green (embed unit tests, router tests, `serve_smoke` integration) against both the placeholder and a real `pnpm build` bundle.
- [ ] `cargo test -p oximy-gateway` is green (`firstboot::` seed-once + secret-shown-once, `cli::` flag/data-dir parsing).
- [ ] `cargo clippy --all-targets -- -D warnings` clean across `gateway-dash` + `oximy-gateway`; `cargo fmt --all --check` clean. (let-chains used in `embed::resolve` and `cli::resolve_data_dir`, no `unsafe`.)
- [ ] The three invariants this milestone owns are each proven by a test: thin-client (`router_exposes_no_json_data_route` + `api_route_wins_over_dashboard_fallback`), secret-shown-once (`second_boot_does_not_reseed_or_reveal_secret`), auth-by-default (the SPA's `api.ts` sends `Authorization` on every call; static assets carry no secret).
- [ ] `oximy-gateway up` on a fresh data dir: creates `<dir>/gateway.db`, prints the admin key exactly once, serves the dashboard at `http://127.0.0.1:4141/`, and opens the browser (manual verification: run it, confirm key prints once, reload after a second `up` shows no new key).
- [ ] `dist plan` produces the 5-target artifact matrix with the UI pre-build step wired in; a release tag would embed the **real** dashboard (the `Verify dashboard embedded` step fails the build otherwise).
- [ ] `docs/quickstart.md` documents the one-command boot, flags, OpenAI-client usage, and the lost-key behavior.

> **Integration note:** Tasks 1–6 and 8–11 are self-contained within `gateway-dash`/`oximy-gateway`. Task 7 (wiring `up` to the live server) consumes the P1.4 `api_router`/`ControlState` and P1.6 `open_or_init_store` seams; if those land after this milestone starts, build everything else, stub Task 7's three seam calls, and close the wiring in the integration pass — **without** adding any server capability the API lacks (the thin-client invariant).

**This is the final P1 milestone.** With it, P1's phase exit criterion is met: `oximy-gateway up` boots and serves the dashboard with no config file. **Next:** Phase 2 — the MCP plane on the same spine (federation, transport bridging, inbound OAuth 2.1, per-key tool ACL, tool-call dollar metering) — which adds MCP screens to this dashboard (reusing `dash_router` + the thin-client `api.ts` pattern) and reuses `firstboot::ensure_admin_key` for re-bootstrap.
