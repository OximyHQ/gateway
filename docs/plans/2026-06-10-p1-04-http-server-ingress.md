# Phase 1.4 — HTTP Server + Request Lifecycle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `gateway-control` Axum HTTP server — the ingress that turns a real bearer-authenticated HTTP request into a fully-governed LLM call. It exposes `/v1/chat/completions` (JSON + SSE stream), `/v1/responses`, `/v1/messages`, `/v1/embeddings`, and `/v1/models`, and wires the **complete per-request lifecycle** over the P1.1 spine and P1.2 `Provider` egress: resolve the bearer key → `ensure_usable` → `allows_model` → rate-limit `acquire` → budget `reserve` → guard pre-hook (stub seam) → provider egress (streaming, idempotency key) → `commit` actual cost → `release_parallel` → emit response with `usage.cost` + `x-overhead-duration-ms` headers. The spine's `SpineError` taxonomy maps to HTTP status codes, and **fallback fires only before the first token**.

**This is where the spine invariants become observable over the wire.** Every governance decision the spine encodes (fail-closed budgets, auth-by-default, no-double-billing, commit-once) is now enforced on a real `axum::Router`. The shapes here — `AppState`, the `Gateway` lifecycle struct, the `ProviderRegistry`, the auth extractor — are imported by P1.5 (cache), P1.6 (persistence/config), P1.7 (telemetry), and P1.8 (dashboard/first-boot). Define them with care; later milestones wire real I/O behind the seams this milestone stubs.

**Architecture:** A thin Axum HTTP layer over a pure-orchestration `Gateway` core. The handlers do only HTTP concerns (deserialize body, extract bearer token, set headers, serialize); they delegate the entire governance lifecycle to `Gateway::run` / `Gateway::run_stream`, which is unit-testable with no socket. State (`ModelRegistry`, `BudgetLedger`, `RateLimiter`, key store, `ProviderRegistry`, `AuditSink`, `Clock`) lives in an `AppState` behind `Arc`, injected via Axum `State`. The `Provider` egress (P1.2) is selected by the model's `provider` field from the registry. A `KeyStore` trait seam resolves a bearer secret → `VirtualKey` (single-tenant static-key store here; full key CRUD is P1.6). A `GuardHook` trait seam is a no-op pass in P1.4 (real PII/injection/moderation is P4). The streaming path threads the same `idempotency_key` as non-streaming and **commits usage from the terminal `StreamDelta`** — never dropping it on an aborted stream.

**Tech Stack:** Rust 2024; `axum` 0.8 (router, extractors, SSE) + `tokio`; `gateway-spine` (keys, budget, rate limit, registry, audit, error) and `gateway-llm` (`ChatRequest`/`ChatResponse`/`StreamDelta`/`Provider`/`Credentials`/`ProviderError`); `futures`/`tokio-stream` for stream adaptation; `serde`/`serde_json`; `thiserror`; `uuid` for idempotency keys. Tests: `tokio::test` + `axum::body`/`tower::ServiceExt::oneshot` (in-process router, no socket), a `MockProvider` implementing `gateway_llm::Provider`, and the spine's `MockClock`.

**Invariants this milestone enforces (design §2, §6):**
- **Auth-by-default** — every `/v1/*` data route requires a valid bearer key; a missing/unknown/revoked/expired key is rejected **before** any provider egress. Proven by tests that no `MockProvider` call occurs on auth failure.
- **Fail-closed budgets** — `reserve` happens **before** egress; a `BudgetExceeded` short-circuits to HTTP 429 with the provider never called. Commit uses the provider-reported usage (true-up), never the estimate.
- **No double-billing** — one `idempotency_key` (a UUID minted once per logical request) is threaded to `Provider::chat`/`stream`; it is the same value passed to the egress regardless of internal retries. Cost is committed exactly once from `usage`.
- **Never lose usage on aborted streams** — the streaming lifecycle commits cost from the terminal delta's `usage`; if the client disconnects mid-stream, the spawned drain still commits whatever usage arrived (no leak of the reservation, no missed bill).
- **Cost-correctness** — `usage.cost` and the committed amount both come from `ModelRegistry::cost`; an unknown model is a 400 (`UnknownModel`), never a guessed price.

**Explicitly DEFERRED** (kept out of scope; the seam is noted inline where it lives):
- **Exact-match cache** lookup/replay → **P1.5** (a `CacheHook` seam returns `None` here).
- **Routing/fallback array, provider prefs, hedging, mid-stream resume** → **P1.5/P4** (P1.4 implements *single-deployment* egress + the "fallback only before first token" guarantee structurally, but ships exactly one upstream per model).
- **Persistent key store + full key CRUD + Postgres** → **P1.6** (P1.4 ships a `StaticKeyStore` bootstrapped from one env/`AppState` secret).
- **Real guardrail stages (PII/injection/moderation)** → **P4** (`GuardHook` is a no-op `Allow` here).
- **Embedded telemetry write + Prometheus** → **P1.7** (`x-overhead-duration-ms` IS shipped here; the async columnar write is P1.7 — P1.4 records to the in-memory `AuditSink`).
- **Real embeddings transport** → the `/v1/embeddings` route is wired through auth+budget but returns `501`-style `Unsupported` until an embeddings `Provider` method exists (P5); the route exists so clients fail with a typed error, not a 404.

---

### Task 1: Add HTTP / server dependencies to `gateway-control`

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/gateway-control/Cargo.toml`

- [ ] **Step 1: Add shared dep versions to the workspace**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `wiremock = "0.6"` line added by P1.2):

```toml
axum = { version = "0.8", features = ["json", "tokio", "http2"] }
tower = { version = "0.5", features = ["util"] }
http = "1"
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 2: Reference them from `gateway-control/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-control/Cargo.toml` with, and add a `[dev-dependencies]` section:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
gateway-spine = { workspace = true }
gateway-llm = { workspace = true }
gateway-config = { workspace = true }
gateway-telemetry = { workspace = true }
axum = { workspace = true }
http = { workspace = true }
uuid = { workspace = true }
futures = { workspace = true }
async-trait = { workspace = true }

[dev-dependencies]
tower = { workspace = true }
http = { workspace = true }
serde_json = { workspace = true }
```

> `gateway-llm` is a new dependency of `gateway-control` (it was not in the scaffold manifest); it brings `futures`/`async-trait`/`reqwest` transitively, but we depend on `futures` + `async-trait` directly because the `Provider` seam and stream adaptation use them in this crate's own code. `gateway-config`/`gateway-telemetry` stay (scaffold deps) for P1.6/P1.7 wiring; unused for now is fine — they don't break the build.

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p gateway-control`
Expected: builds (still the scaffold `lib.rs` with the `CRATE` placeholder).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/gateway-control/Cargo.toml Cargo.lock
git commit -s -m "build(control): add axum, tower, http, uuid deps"
```

---

### Task 2: `GatewayError` — the HTTP-facing error with `SpineError`/`ProviderError` mapping

**Files:**
- Create: `crates/gateway-control/src/error.rs`
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/error.rs`:

```rust
//! The HTTP-facing error. Every lifecycle failure becomes a `GatewayError`,
//! which carries the HTTP status and an OpenAI-shaped JSON error body so SDK
//! clients parse it. `SpineError` and `ProviderError` map in here — this is the
//! single place the governance taxonomy meets HTTP status codes (design §6).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use gateway_llm::ProviderError;
use gateway_spine::SpineError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum GatewayError {
    #[error("missing or malformed Authorization header")]
    MissingAuth,
    #[error("invalid api key")]
    InvalidKey,
    #[error("{0}")]
    Spine(#[from] SpineError),
    #[error("{0}")]
    Provider(#[from] ProviderError),
    #[error("invalid request: {0}")]
    BadRequest(String),
    #[error("feature not supported: {0}")]
    Unsupported(String),
}

impl GatewayError {
    /// The HTTP status this error maps to (design §6).
    pub fn status(&self) -> StatusCode {
        match self {
            GatewayError::MissingAuth | GatewayError::InvalidKey => StatusCode::UNAUTHORIZED,
            GatewayError::Spine(e) => match e {
                SpineError::BudgetExceeded { .. } | SpineError::RateLimited { .. } => {
                    StatusCode::TOO_MANY_REQUESTS
                }
                SpineError::KeyRevoked { .. } | SpineError::KeyExpired { .. } => {
                    StatusCode::UNAUTHORIZED
                }
                SpineError::ModelNotAllowed { .. } => StatusCode::FORBIDDEN,
                SpineError::UnknownModel { .. } => StatusCode::BAD_REQUEST,
                SpineError::NoSuchKey { .. } => StatusCode::UNAUTHORIZED,
                SpineError::NoSuchReservation => StatusCode::INTERNAL_SERVER_ERROR,
            },
            GatewayError::Provider(e) => match e {
                ProviderError::Auth => StatusCode::BAD_GATEWAY,
                ProviderError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
                ProviderError::Upstream { status, .. } => {
                    StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
                }
                ProviderError::Unsupported { .. } => StatusCode::NOT_IMPLEMENTED,
                ProviderError::Transport(_) | ProviderError::Decode(_) => StatusCode::BAD_GATEWAY,
            },
            GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
            GatewayError::Unsupported(_) => StatusCode::NOT_IMPLEMENTED,
        }
    }

    /// OpenAI-shaped error "type" string clients switch on.
    pub fn error_type(&self) -> &'static str {
        match self {
            GatewayError::MissingAuth | GatewayError::InvalidKey => "authentication_error",
            GatewayError::Spine(SpineError::BudgetExceeded { .. }) => "insufficient_quota",
            GatewayError::Spine(SpineError::RateLimited { .. }) => "rate_limit_error",
            GatewayError::Spine(SpineError::KeyRevoked { .. } | SpineError::KeyExpired { .. }) => {
                "authentication_error"
            }
            GatewayError::Spine(SpineError::ModelNotAllowed { .. }) => "permission_error",
            GatewayError::Spine(SpineError::UnknownModel { .. }) | GatewayError::BadRequest(_) => {
                "invalid_request_error"
            }
            GatewayError::Provider(_) => "upstream_error",
            _ => "api_error",
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": {
                "message": self.to_string(),
                "type": self.error_type(),
            }
        });
        (self.status(), Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::RateDimension;

    #[test]
    fn budget_exceeded_is_429_insufficient_quota() {
        let e = GatewayError::Spine(SpineError::budget_exceeded(
            "k",
            gateway_spine::Usd::from_micros(2),
            gateway_spine::Usd::from_micros(1),
        ));
        assert_eq!(e.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(e.error_type(), "insufficient_quota");
    }

    #[test]
    fn rate_limited_is_429() {
        let e = GatewayError::Spine(SpineError::RateLimited {
            key_id: "k".into(),
            dimension: RateDimension::Requests,
        });
        assert_eq!(e.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn revoked_and_missing_auth_are_401() {
        assert_eq!(GatewayError::MissingAuth.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(GatewayError::InvalidKey.status(), StatusCode::UNAUTHORIZED);
        let e = GatewayError::Spine(SpineError::KeyRevoked { key_id: "k".into() });
        assert_eq!(e.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn model_not_allowed_is_403_unknown_is_400() {
        let na = GatewayError::Spine(SpineError::ModelNotAllowed {
            key_id: "k".into(),
            model: "x".into(),
        });
        assert_eq!(na.status(), StatusCode::FORBIDDEN);
        let unk = GatewayError::Spine(SpineError::UnknownModel { model: "x".into() });
        assert_eq!(unk.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn provider_unsupported_is_501() {
        let e = GatewayError::Provider(ProviderError::Unsupported { feature: "audio".into() });
        assert_eq!(e.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
```

Replace the body of `crates/gateway-control/src/lib.rs` (drop the `CRATE` placeholder) with:

```rust
//! # gateway-control
//!
//! The HTTP ingress + per-request governance lifecycle over the spine. Three
//! thin clients (REST API, admin-MCP, CLI) share one core; P1.4 ships the REST
//! data-plane (`/v1/*`) and the lifecycle. See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

pub mod error;

pub use error::GatewayError;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control error::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/error.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): GatewayError with SpineError/ProviderError -> HTTP mapping"
```

---

### Task 3: `KeyStore` trait + `StaticKeyStore` (single-tenant bootstrap)

**Files:**
- Create: `crates/gateway-control/src/keystore.rs`
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/keystore.rs`:

```rust
//! Resolves a bearer secret → the governing `VirtualKey`. The trait is the seam
//! that P1.6 fills with a persistent, CRUD-backed store; P1.4 ships a static,
//! in-memory store bootstrapped from one secret (single-tenant first boot,
//! design §10 "single-tenant static-key bootstrap"). Lookup is by SHA-256 hash
//! so the raw secret is never stored.

use std::collections::HashMap;

use gateway_spine::VirtualKey;

/// Resolve an incoming bearer secret to its `VirtualKey`, or `None` if unknown.
pub trait KeyStore: Send + Sync {
    fn resolve(&self, secret: &str) -> Option<VirtualKey>;
}

/// In-memory store keyed by the secret's SHA-256 hash. Seeded at boot.
#[derive(Debug, Default, Clone)]
pub struct StaticKeyStore {
    by_hash: HashMap<String, VirtualKey>,
}

impl StaticKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key whose `token_hash` already matches the secret it should
    /// resolve. (The key carries its own hash; we index by it.)
    pub fn insert(&mut self, key: VirtualKey) {
        self.by_hash.insert(key.token_hash.clone(), key);
    }

    /// Convenience for single-tenant bootstrap: build a budget-only key from a
    /// raw secret and register it. Returns the key's id.
    pub fn bootstrap(&mut self, secret: &str, max_budget: Option<gateway_spine::Usd>) -> String {
        let hash = VirtualKey::hash_secret(secret);
        let prefix: String = secret.chars().take(8).collect();
        let key = VirtualKey {
            id: "key_bootstrap".into(),
            token_hash: hash,
            token_prefix: prefix,
            max_budget,
            limits: gateway_spine::RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        };
        let id = key.id.clone();
        self.insert(key);
        id
    }
}

impl KeyStore for StaticKeyStore {
    fn resolve(&self, secret: &str) -> Option<VirtualKey> {
        let hash = VirtualKey::hash_secret(secret);
        self.by_hash.get(&hash).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::Usd;

    #[test]
    fn bootstrap_resolves_only_the_right_secret() {
        let mut store = StaticKeyStore::new();
        let id = store.bootstrap("sk-live-abcdefgh", Some(Usd::from_dollars_f64(10.0)));
        assert_eq!(id, "key_bootstrap");

        let resolved = store.resolve("sk-live-abcdefgh").expect("known secret resolves");
        assert_eq!(resolved.id, "key_bootstrap");
        assert_eq!(resolved.max_budget, Some(Usd::from_dollars_f64(10.0)));
        assert_eq!(resolved.token_prefix, "sk-live-");

        assert!(store.resolve("sk-wrong").is_none());
    }

    #[test]
    fn never_stores_the_raw_secret() {
        let mut store = StaticKeyStore::new();
        store.bootstrap("super-secret", None);
        // The stored key's hash is not the plaintext.
        let k = store.resolve("super-secret").unwrap();
        assert_ne!(k.token_hash, "super-secret");
        assert_eq!(k.token_hash.len(), 64);
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod keystore;

pub use keystore::{KeyStore, StaticKeyStore};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control keystore::`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/keystore.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): KeyStore trait + StaticKeyStore single-tenant bootstrap"
```

---

### Task 4: `ProviderRegistry` + `GuardHook` seam

**Files:**
- Create: `crates/gateway-control/src/providers.rs`
- Create: `crates/gateway-control/src/guard.rs`
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Write the failing test (provider registry)**

Create `crates/gateway-control/src/providers.rs`:

```rust
//! Maps a provider id (the `provider` field on a `ModelEntry`) to a concrete
//! egress `Provider` (P1.2) plus its `Credentials`. The lifecycle selects the
//! transport for a model by looking up its provider here. P1.5 grows this into
//! the multi-deployment / fallback array; P1.4 ships exactly one deployment per
//! provider id.

use std::collections::HashMap;
use std::sync::Arc;

use gateway_llm::{Credentials, Provider};

/// One configured egress deployment: a transport + the credentials to call it.
#[derive(Clone)]
pub struct Deployment {
    pub provider: Arc<dyn Provider>,
    pub credentials: Arc<Credentials>,
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    by_id: HashMap<String, Deployment>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, provider_id: impl Into<String>, deployment: Deployment) {
        self.by_id.insert(provider_id.into(), deployment);
    }

    pub fn get(&self, provider_id: &str) -> Option<&Deployment> {
        self.by_id.get(provider_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gateway_llm::{
        ChatRequest, ChatResponse, DeltaStream, ProviderCapabilities, ProviderError,
    };

    struct Dummy;

    #[async_trait]
    impl Provider for Dummy {
        fn id(&self) -> &str {
            "dummy"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
                supports_tools: true,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            _req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            unreachable!()
        }
        async fn stream(
            &self,
            _req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            unreachable!()
        }
    }

    #[test]
    fn lookup_by_provider_id() {
        let mut r = ProviderRegistry::new();
        r.insert(
            "openai",
            Deployment {
                provider: Arc::new(Dummy),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        assert!(r.get("openai").is_some());
        assert_eq!(r.get("openai").unwrap().provider.id(), "dummy");
        assert!(r.get("anthropic").is_none());
    }
}
```

- [ ] **Step 2: Write the failing test (guard hook)**

Create `crates/gateway-control/src/guard.rs`:

```rust
//! The guardrail seam. P4 fills this with PII/injection/moderation stages; P1.4
//! ships a no-op `AllowAll` so the lifecycle has the call site wired in the
//! right place (pre-egress) without doing work. A `Deny` short-circuits the
//! request before any provider egress, exactly like a budget/rate denial.

use gateway_llm::ChatRequest;

/// The verdict of a guard stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    Allow,
    Deny { reason: String },
}

/// Pre-egress hook. Returns `Allow` to proceed or `Deny` to short-circuit.
pub trait GuardHook: Send + Sync {
    fn pre(&self, req: &ChatRequest) -> GuardVerdict;
}

/// The P1.4 default: never blocks.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAll;

impl GuardHook for AllowAll {
    fn pre(&self, _req: &ChatRequest) -> GuardVerdict {
        GuardVerdict::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::{ChatRequest, Message, Role};

    #[test]
    fn allow_all_always_allows() {
        let req = ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi")]);
        assert_eq!(AllowAll.pre(&req), GuardVerdict::Allow);
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod guard;
pub mod providers;

pub use guard::{AllowAll, GuardHook, GuardVerdict};
pub use providers::{Deployment, ProviderRegistry};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p gateway-control providers:: guard::`
Expected: 1 provider + 1 guard = 2 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/providers.rs crates/gateway-control/src/guard.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): ProviderRegistry + no-op GuardHook seam"
```

---

### Task 5: `AppState` — the shared, `Arc`-injected service container

**Files:**
- Create: `crates/gateway-control/src/state.rs`
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/state.rs`:

```rust
//! The shared service container injected into every handler via Axum `State`.
//! It owns the spine governance components (registry, ledger, limiter, key
//! store, audit, clock) and the egress providers + guard seam. Everything is
//! behind `Arc`/interior-mutability so the whole state is cheap to clone per
//! request and safe to share across the Tokio pool. P1.6 swaps the in-memory
//! stores for persistent ones behind the same field types (trait objects).

use std::sync::{Arc, RwLock};

use gateway_spine::{
    AuditSink, BudgetLedger, Clock, MemoryAudit, ModelRegistry, RateLimiter, SystemClock,
};

use crate::guard::{AllowAll, GuardHook};
use crate::keystore::{KeyStore, StaticKeyStore};
use crate::providers::ProviderRegistry;

/// Concrete clock type the rate limiter is parameterized on for the server.
/// Boxed clocks aren't object-safe-friendly with `RateLimiter<C>`, so the
/// server fixes `SystemClock`; tests construct `AppState` with a `MockClock`
/// via [`AppState::with_clock`].
pub struct AppState<C: Clock = SystemClock> {
    pub registry: RwLock<ModelRegistry>,
    pub ledger: Arc<BudgetLedger>,
    pub limiter: Arc<RateLimiter<C>>,
    pub keys: Arc<dyn KeyStore>,
    pub providers: ProviderRegistry,
    pub guard: Arc<dyn GuardHook>,
    pub audit: Arc<dyn AuditSink>,
    pub clock: Arc<C>,
}

impl AppState<SystemClock> {
    /// Production constructor: a system clock, empty registry/providers to be
    /// populated by the binary (P1.8) or a config load (P1.6).
    pub fn new(keys: Arc<dyn KeyStore>) -> Self {
        Self::with_parts(
            keys,
            Arc::new(SystemClock),
            ProviderRegistry::new(),
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        )
    }
}

impl<C: Clock> AppState<C> {
    /// Full constructor used by tests (injects a `MockClock` + a `MockProvider`
    /// registry + a seeded key store).
    pub fn with_parts(
        keys: Arc<dyn KeyStore>,
        clock: Arc<C>,
        providers: ProviderRegistry,
        guard: Arc<dyn GuardHook>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        let limiter = Arc::new(RateLimiter::new_arc_clock(clock.clone()));
        Self {
            registry: RwLock::new(ModelRegistry::new()),
            ledger: Arc::new(BudgetLedger::new()),
            limiter,
            keys,
            providers,
            guard,
            audit,
            clock,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::MockClock;

    #[test]
    fn builds_with_a_static_keystore() {
        let mut ks = StaticKeyStore::new();
        ks.bootstrap("sk-x", None);
        let clock = Arc::new(MockClock::new(0));
        let state = AppState::with_parts(
            Arc::new(ks),
            clock,
            ProviderRegistry::new(),
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        );
        assert!(state.keys.resolve("sk-x").is_some());
        assert!(state.registry.read().unwrap().is_empty());
    }
}
```

> **`RateLimiter::new_arc_clock` does not exist yet.** The P1.1 `RateLimiter::new(clock)` takes the clock *by value* (`RateLimiter<C: Clock>`), but `AppState` needs to share the same clock instance with `ensure_usable`/expiry checks. Add a tiny `new_arc_clock` constructor to the spine in the next step rather than re-deriving time — this keeps one clock per server.

- [ ] **Step 2: Add `RateLimiter::new_arc_clock` to the spine (the one spine touch this milestone needs)**

The spine's `RateLimiter<C: Clock>` owns its clock by value. The server shares an `Arc<C>` so handlers can read `now_ms()` for `ensure_usable`. Add a constructor that accepts an `Arc<C>` — but `RateLimiter` stores `C`, not `Arc<C>`. The clean fix: implement `Clock` for `Arc<C>` in the spine (so `RateLimiter<Arc<C>>` works), then `new_arc_clock` is just `new`.

Edit `crates/gateway-spine/src/clock.rs` — add the blanket impl after the `MockClock` `impl Clock` block (before `#[cfg(test)]`):

```rust
impl<C: Clock> Clock for std::sync::Arc<C> {
    fn now_ms(&self) -> i64 {
        (**self).now_ms()
    }
}
```

Then in `crates/gateway-spine/src/ratelimit.rs`, add inside `impl<C: Clock> RateLimiter<C>` (after `new`):

```rust
    /// Construct from a shared clock. Equivalent to `new(clock)` where `C` is an
    /// `Arc<_>` — provided so callers that share one clock across components read
    /// `now_ms()` from the same source as rate-limit windows.
    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }
```

And change `AppState::with_parts` to construct the limiter without the missing helper — the limiter takes the `Arc<C>` *by value* because `Arc<C>: Clock` now:

Replace this line in `crates/gateway-control/src/state.rs`:

```rust
        let limiter = Arc::new(RateLimiter::new_arc_clock(clock.clone()));
```

with:

```rust
        let limiter = Arc::new(RateLimiter::new(clock.clone()));
```

and change the field type so `RateLimiter` is parameterized on the shared clock:

```rust
    pub limiter: Arc<RateLimiter<Arc<C>>>,
```

> Note: `RateLimiter::new(clock.clone())` now builds a `RateLimiter<Arc<C>>` because `Arc<C>: Clock`. The handler reads current time via `state.clock.now_ms()` (the `Arc<C>` deref), so windows and expiry share one clock. Re-run the spine's tests after the clock edit to confirm nothing regressed:

Run: `cargo test -p gateway-spine clock:: ratelimit::`
Expected: existing clock + ratelimit tests still PASS, plus the new `Arc<C>: Clock` impl compiles.

- [ ] **Step 3: Run the control test**

Run: `cargo test -p gateway-control state::`
Expected: 1 test PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-spine/src/clock.rs crates/gateway-spine/src/ratelimit.rs crates/gateway-control/src/state.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): AppState container + Arc<Clock> sharing in the spine"
```

Add to `crates/gateway-control/src/lib.rs` (do this in Step 2 before running, alongside the edits):

```rust
pub mod state;

pub use state::AppState;
```

---

### Task 6: Bearer auth extractor — resolve key + `ensure_usable`

**Files:**
- Create: `crates/gateway-control/src/auth.rs`
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/auth.rs`:

```rust
//! Bearer-token authentication: the auth-by-default chokepoint (design §2).
//! Every data route resolves the `Authorization: Bearer <secret>` header to a
//! `VirtualKey` and runs `ensure_usable(now)` BEFORE any governance or egress.
//! This is a plain function (not an Axum `FromRequestParts` extractor) so the
//! lifecycle can call it directly and tests don't need a full request — the
//! handler calls it first thing.

use gateway_spine::{Clock, VirtualKey};

use crate::error::GatewayError;
use crate::keystore::KeyStore;

/// Parse a bearer header value into its raw secret. `None` if missing/malformed.
pub fn parse_bearer(header: Option<&str>) -> Option<&str> {
    let value = header?;
    let rest = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))?;
    let secret = rest.trim();
    if secret.is_empty() { None } else { Some(secret) }
}

/// Resolve + validate a bearer secret into a usable `VirtualKey`. Fails closed:
/// missing header → 401 MissingAuth; unknown secret → 401 InvalidKey;
/// revoked/expired → 401 via `SpineError` mapping.
pub fn authenticate(
    keys: &dyn KeyStore,
    clock: &dyn Clock,
    auth_header: Option<&str>,
) -> Result<VirtualKey, GatewayError> {
    let secret = parse_bearer(auth_header).ok_or(GatewayError::MissingAuth)?;
    let key = keys.resolve(secret).ok_or(GatewayError::InvalidKey)?;
    key.ensure_usable(clock.now_ms())?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use gateway_spine::MockClock;

    fn store() -> StaticKeyStore {
        let mut s = StaticKeyStore::new();
        s.bootstrap("sk-good", None);
        s
    }

    #[test]
    fn parse_bearer_variants() {
        assert_eq!(parse_bearer(Some("Bearer sk-1")), Some("sk-1"));
        assert_eq!(parse_bearer(Some("bearer sk-2")), Some("sk-2"));
        assert_eq!(parse_bearer(Some("Bearer   sk-3  ")), Some("sk-3"));
        assert_eq!(parse_bearer(Some("Token sk-4")), None);
        assert_eq!(parse_bearer(Some("Bearer ")), None);
        assert_eq!(parse_bearer(None), None);
    }

    #[test]
    fn missing_header_is_missing_auth() {
        let s = store();
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, None).unwrap_err();
        assert!(matches!(err, GatewayError::MissingAuth));
    }

    #[test]
    fn unknown_secret_is_invalid_key() {
        let s = store();
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, Some("Bearer sk-nope")).unwrap_err();
        assert!(matches!(err, GatewayError::InvalidKey));
    }

    #[test]
    fn good_secret_resolves() {
        let s = store();
        let c = MockClock::new(0);
        let key = authenticate(&s, &c, Some("Bearer sk-good")).unwrap();
        assert_eq!(key.id, "key_bootstrap");
    }

    #[test]
    fn revoked_key_fails_closed() {
        let mut s = StaticKeyStore::new();
        let mut k = gateway_spine::VirtualKey {
            id: "k".into(),
            token_hash: gateway_spine::VirtualKey::hash_secret("sk-rev"),
            token_prefix: "sk-rev".into(),
            max_budget: None,
            limits: gateway_spine::RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: true,
            parent_id: None,
        };
        k.revoked = true;
        s.insert(k);
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, Some("Bearer sk-rev")).unwrap_err();
        // revoked maps through SpineError -> 401
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod auth;

pub use auth::{authenticate, parse_bearer};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control auth::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/auth.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): bearer auth — resolve key + ensure_usable, fail-closed"
```

---

### Task 7: The `Gateway` non-streaming lifecycle (`run`) — the governance heart

**Files:**
- Create: `crates/gateway-control/src/gateway.rs`
- Modify: `crates/gateway-control/src/lib.rs`

This is the milestone's centerpiece: a single, socket-free function that runs the **entire** admission → egress → commit path for a non-streaming chat call, in the exact order of design §6. It is unit-tested with a `MockProvider`.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/gateway.rs`:

```rust
//! The per-request lifecycle, free of HTTP. `Gateway::run` executes the design
//! §6 order exactly: authenticate (caller passes the resolved key) → allowlist
//! → rate-limit acquire → budget reserve → guard pre-hook → provider egress
//! (with the idempotency key) → commit ACTUAL cost from provider usage →
//! release the parallel slot → return the response + computed `usage.cost`.
//!
//! Failure handling is fail-closed and leak-free: a denial before egress
//! releases nothing (nothing was acquired past the failing step); a denial or
//! error AFTER reserve releases the reservation and the parallel slot so a
//! failed request never strands budget or a concurrency slot. The idempotency
//! key is minted once here and is the SAME value the provider sees on any retry
//! (retries are P1.5; the seam is correct now).

use std::sync::Arc;

use gateway_llm::{ChatRequest, ChatResponse};
use gateway_spine::{AuditEvent, Usd, VirtualKey};

use crate::error::GatewayError;
use crate::guard::GuardVerdict;
use crate::state::AppState;

/// A completed non-streaming call: the provider response plus the authoritative
/// cost committed to the ledger (`usage.cost`).
pub struct Completed {
    pub response: ChatResponse,
    pub cost: Usd,
    pub idempotency_key: String,
}

/// A conservative per-request token estimate used for the pre-call budget
/// reservation and TPM check. True-up happens at commit from real usage.
fn estimate_tokens(req: &ChatRequest) -> i64 {
    // ~4 chars/token over all text parts, plus the max_tokens ceiling for output.
    let input_chars: usize = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|p| match p {
            gateway_llm::ContentPart::Text { text } => Some(text.len()),
            _ => None,
        })
        .sum();
    let input_est = (input_chars / 4).max(1) as i64;
    let output_est = req.max_tokens.unwrap_or(1024);
    input_est + output_est
}

impl<C: gateway_spine::Clock> AppState<C> {
    /// Estimate the worst-case USD for a request, for the fail-closed reserve.
    /// Uses the model's price if known; an unknown model is a hard error here
    /// (cost-correctness: we never reserve against a guessed price).
    fn estimate_cost(&self, model: &str, est_tokens: i64) -> Result<Usd, GatewayError> {
        let reg = self.registry.read().unwrap();
        let entry = reg
            .get(model)
            .ok_or_else(|| GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                model: model.to_string(),
            }))?;
        // Treat the whole estimate as output tokens (the most expensive bucket).
        let usage = gateway_spine::TokenUsage {
            output_tokens: est_tokens,
            ..Default::default()
        };
        Ok(entry.price.cost(&usage))
    }
}

/// The lifecycle. Generic over the clock so tests inject `MockClock`.
pub struct Gateway;

impl Gateway {
    /// Run one non-streaming chat call end-to-end. `key` is the already
    /// authenticated `VirtualKey` (the handler resolves it via `auth`).
    pub async fn run<C: gateway_spine::Clock>(
        state: &AppState<C>,
        key: &VirtualKey,
        req: &ChatRequest,
    ) -> Result<Completed, GatewayError> {
        let model = req.model.as_str();

        // 1. model allowlist
        if !key.allows_model(model) {
            Self::audit(state, key, "request.denied", model, "denied", "model_not_allowed");
            return Err(GatewayError::Spine(gateway_spine::SpineError::ModelNotAllowed {
                key_id: key.id.clone(),
                model: model.to_string(),
            }));
        }

        // 2. resolve the egress deployment for this model's provider (unknown
        //    model → 400 here, before any acquisition).
        let provider_id = {
            let reg = state.registry.read().unwrap();
            reg.get(model)
                .map(|e| e.provider.clone())
                .ok_or_else(|| GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                    model: model.to_string(),
                }))?
        };
        let deployment = state.providers.get(&provider_id).cloned().ok_or_else(|| {
            GatewayError::BadRequest(format!("no egress configured for provider {provider_id}"))
        })?;

        let est_tokens = estimate_tokens(req);

        // 3. rate-limit acquire (RPM/TPM/parallel). On failure nothing else has
        //    been acquired, so just propagate.
        state
            .limiter
            .acquire(&key.id, &key.limits, est_tokens)
            .map_err(|e| {
                Self::audit(state, key, "request.denied", model, "denied", "rate_limited");
                GatewayError::Spine(e)
            })?;

        // 4. budget reserve (fail-closed, BEFORE egress). On failure, release the
        //    parallel slot we just acquired, then propagate.
        let est_cost = match state.estimate_cost(model, est_tokens) {
            Ok(c) => c,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(e);
            }
        };
        let reservation = match state.ledger.reserve(&key.id, est_cost) {
            Ok(r) => r,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                Self::audit(state, key, "request.denied", model, "denied", "budget_exceeded");
                return Err(GatewayError::Spine(e));
            }
        };

        // 5. guard pre-hook (stub Allow in P1.4). A deny releases reservation +
        //    parallel slot before any egress.
        if let GuardVerdict::Deny { reason } = state.guard.pre(req) {
            let _ = state.ledger.release(reservation);
            state.limiter.release_parallel(&key.id);
            Self::audit(state, key, "request.denied", model, "denied", &reason);
            return Err(GatewayError::BadRequest(format!("guard denied: {reason}")));
        }

        // 6. egress — mint ONE idempotency key (no-double-billing) and call the
        //    provider. Any error releases the reservation + parallel slot.
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let result = deployment
            .provider
            .chat(req, &deployment.credentials, &idempotency_key)
            .await;
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                Self::audit(state, key, "request.error", model, "error", &e.to_string());
                return Err(GatewayError::Provider(e));
            }
        };

        // 7. commit ACTUAL cost from provider usage (true-up). Cost is computed
        //    from the registry — never guessed. Then release the parallel slot.
        let actual_cost = {
            let reg = state.registry.read().unwrap();
            reg.cost(model, &response.usage).unwrap_or(Usd::ZERO)
        };
        state
            .ledger
            .commit(reservation, actual_cost)
            .map_err(GatewayError::Spine)?;
        state.limiter.release_parallel(&key.id);

        Self::audit(
            state,
            key,
            "request.complete",
            model,
            "ok",
            &format!("{} µUSD", actual_cost.micros()),
        );

        Ok(Completed {
            response,
            cost: actual_cost,
            idempotency_key,
        })
    }

    fn audit<C: gateway_spine::Clock>(
        state: &AppState<C>,
        key: &VirtualKey,
        action: &str,
        target: &str,
        outcome: &str,
        detail: &str,
    ) {
        state.audit.record(AuditEvent {
            ts_ms: state.clock.now_ms(),
            actor: key.id.clone(),
            action: action.to_string(),
            target: target.to_string(),
            outcome: outcome.to_string(),
            detail: Some(detail.to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::AllowAll;
    use crate::keystore::StaticKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use gateway_llm::{
        ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Message, Provider,
        ProviderCapabilities, ProviderError, Role,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, ModelRegistry, RateLimits, TokenUsage, Usd,
        VirtualKey,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A provider that records how many times it was called + the last
    /// idempotency key, and returns a fixed usage.
    struct MockProvider {
        calls: AtomicUsize,
        last_idem: std::sync::Mutex<Option<String>>,
        usage: TokenUsage,
    }

    impl MockProvider {
        fn new(usage: TokenUsage) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                last_idem: std::sync::Mutex::new(None),
                usage,
            }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
                supports_tools: true,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_idem.lock().unwrap() = Some(idempotency_key.to_string());
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![ContentPart::text("hello")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: self.usage,
                provider_response_id: Some("resp_1".into()),
            })
        }
        async fn stream(
            &self,
            _req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            unreachable!("non-streaming test")
        }
    }

    fn gpt4o() -> ModelEntry {
        ModelEntry {
            id: "gpt-4o".into(),
            provider: "openai".into(),
            price: ModelPrice {
                input_per_mtok: 2_500_000,
                output_per_mtok: 10_000_000,
                cache_read_per_mtok: 1_250_000,
                cache_write_per_mtok: 0,
            },
            context_window: Some(128_000),
            max_output_tokens: Some(16_384),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        }
    }

    fn key(budget: Option<Usd>, allow: Option<Vec<String>>, limits: RateLimits) -> VirtualKey {
        VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-test"),
            token_prefix: "sk-test".into(),
            max_budget: budget,
            limits,
            model_allowlist: allow,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    /// Build an AppState wired to a shared MockProvider so tests can inspect it.
    fn state_with(provider: Arc<MockProvider>, budget: Option<Usd>) -> AppState<MockClock> {
        let mut ks = StaticKeyStore::new();
        ks.insert(key(budget, None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: provider.clone(),
                credentials: Arc::new(Credentials::new("sk-up")),
            },
        );
        let state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(1_000)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", budget, Usd::ZERO);
        state
    }

    fn chat_req() -> ChatRequest {
        ChatRequest::new("gpt-4o", vec![Message::text(Role::User, "hi there")])
    }

    #[tokio::test]
    async fn happy_path_commits_actual_cost() {
        // 1000 in + 500 out → $0.0025 + $0.005 = $0.0075 = 7_500 µUSD
        let usage = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
        let provider = Arc::new(MockProvider::new(usage));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default());

        let done = Gateway::run(&state, &k, &chat_req()).await.unwrap();

        assert_eq!(done.cost, Usd::from_micros(7_500));
        assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO, "reservation trued up");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        // idempotency key was minted + passed
        assert!(provider.last_idem.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn budget_exceeded_never_calls_provider() {
        let usage = TokenUsage { output_tokens: 500, ..Default::default() };
        let provider = Arc::new(MockProvider::new(usage));
        // tiny budget that the worst-case estimate ($... for 1024 output) blows
        let state = state_with(provider.clone(), Some(Usd::from_micros(1)));
        let k = key(Some(Usd::from_micros(1)), None, RateLimits::default());

        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0, "fail-closed: no egress");
        // no stranded reservation or parallel slot
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO);
    }

    #[tokio::test]
    async fn disallowed_model_is_403_no_egress() {
        let provider = Arc::new(MockProvider::new(TokenUsage::default()));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(
            Some(Usd::from_dollars_f64(1.0)),
            Some(vec!["claude-3-5-sonnet".into()]),
            RateLimits::default(),
        );
        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unknown_model_is_400() {
        let provider = Arc::new(MockProvider::new(TokenUsage::default()));
        let state = state_with(provider.clone(), Some(Usd::from_dollars_f64(1.0)));
        let k = key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default());
        let req = ChatRequest::new("mystery", vec![Message::text(Role::User, "hi")]);
        let err = Gateway::run(&state, &k, &req).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn provider_error_releases_reservation() {
        struct Failing;
        #[async_trait]
        impl Provider for Failing {
            fn id(&self) -> &str { "fail" }
            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_streaming: false,
                    supports_tools: false,
                    supports_vision: false,
                    supports_idempotency: false,
                }
            }
            async fn chat(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                Err(ProviderError::Upstream { status: 500, body: "boom".into() })
            }
            async fn stream(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<DeltaStream, ProviderError> {
                unreachable!()
            }
        }

        let mut ks = StaticKeyStore::new();
        ks.insert(key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment { provider: Arc::new(Failing), credentials: Arc::new(Credentials::new("x")) },
        );
        let state = AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        );
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default());
        let err = Gateway::run(&state, &k, &chat_req()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO, "released on egress error");
        assert_eq!(state.ledger.spent("key_1"), Usd::ZERO, "nothing billed");
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod gateway;

pub use gateway::{Completed, Gateway};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control gateway::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/gateway.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): non-streaming request lifecycle (auth->budget->egress->commit)"
```

---

### Task 8: The `Gateway` streaming lifecycle (`run_stream`) — commit-from-terminal-delta

**Files:**
- Modify: `crates/gateway-control/src/gateway.rs`

The streaming path runs the identical admission steps, but egress returns a `DeltaStream`. We wrap that stream so that as it drains, content deltas pass through to the client, and the **terminal delta's `usage`** drives the commit + parallel release — guaranteeing the no-lost-usage-on-abort invariant even if the client disconnects (the commit closure runs when the wrapped stream ends or is dropped).

- [ ] **Step 1: Add the streaming lifecycle and its test to `gateway.rs`**

Add these imports at the top of `crates/gateway-control/src/gateway.rs` (merge into the existing `use` block):

```rust
use futures::stream::{Stream, StreamExt};
use gateway_llm::{ProviderError, StreamDelta};
use gateway_spine::ReservationId;
```

Add a streaming result type after `Completed`:

```rust
/// The output of a streaming run: the wrapped delta stream (commit-on-terminal)
/// plus the minted idempotency key for response headers. The stream yields the
/// SAME `StreamDelta`s the provider produced; the cost commit is a side effect
/// that fires on the terminal (usage-carrying) delta.
pub struct CompletedStream {
    pub stream: std::pin::Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>> + Send>>,
    pub idempotency_key: String,
}
```

Add the `run_stream` method inside `impl Gateway` (after `run`):

```rust
    /// Streaming variant. Same admission order as `run`; egress returns a
    /// delta stream that we wrap to commit the actual cost from the terminal
    /// delta's usage and release the parallel slot when the stream ends. If the
    /// stream is dropped before completion (client abort), the wrapper commits
    /// whatever usage arrived and releases the slot — usage is never lost and a
    /// reservation is never stranded.
    pub async fn run_stream<C: gateway_spine::Clock + 'static>(
        state: Arc<AppState<C>>,
        key: &VirtualKey,
        req: &ChatRequest,
    ) -> Result<CompletedStream, GatewayError> {
        let model = req.model.clone();

        if !key.allows_model(&model) {
            Self::audit(&state, key, "request.denied", &model, "denied", "model_not_allowed");
            return Err(GatewayError::Spine(gateway_spine::SpineError::ModelNotAllowed {
                key_id: key.id.clone(),
                model: model.clone(),
            }));
        }

        let provider_id = {
            let reg = state.registry.read().unwrap();
            reg.get(&model)
                .map(|e| e.provider.clone())
                .ok_or_else(|| GatewayError::Spine(gateway_spine::SpineError::UnknownModel {
                    model: model.clone(),
                }))?
        };
        let deployment = state.providers.get(&provider_id).cloned().ok_or_else(|| {
            GatewayError::BadRequest(format!("no egress configured for provider {provider_id}"))
        })?;

        let est_tokens = estimate_tokens(req);

        state
            .limiter
            .acquire(&key.id, &key.limits, est_tokens)
            .map_err(|e| GatewayError::Spine(e))?;

        let est_cost = match state.estimate_cost(&model, est_tokens) {
            Ok(c) => c,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(e);
            }
        };
        let reservation = match state.ledger.reserve(&key.id, est_cost) {
            Ok(r) => r,
            Err(e) => {
                state.limiter.release_parallel(&key.id);
                return Err(GatewayError::Spine(e));
            }
        };

        if let GuardVerdict::Deny { reason } = state.guard.pre(req) {
            let _ = state.ledger.release(reservation);
            state.limiter.release_parallel(&key.id);
            return Err(GatewayError::BadRequest(format!("guard denied: {reason}")));
        }

        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let inner = match deployment
            .provider
            .stream(req, &deployment.credentials, &idempotency_key)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = state.ledger.release(reservation);
                state.limiter.release_parallel(&key.id);
                return Err(GatewayError::Provider(e));
            }
        };

        let wrapped = Self::wrap_stream_for_commit(state, key.id.clone(), model, reservation, inner);
        Ok(CompletedStream { stream: Box::pin(wrapped), idempotency_key })
    }

    /// Wrap a provider delta stream so the terminal usage commits cost + releases
    /// the parallel slot exactly once, whether the stream completes or is dropped.
    fn wrap_stream_for_commit<C: gateway_spine::Clock + 'static>(
        state: Arc<AppState<C>>,
        key_id: String,
        model: String,
        reservation: ReservationId,
        inner: std::pin::Pin<Box<dyn Stream<Item = Result<StreamDelta, ProviderError>> + Send>>,
    ) -> impl Stream<Item = Result<StreamDelta, ProviderError>> + Send {
        // State carried across the stream: the latest usage seen + a guard that
        // commits on Drop so an aborted stream still trues-up.
        struct CommitGuard<C: gateway_spine::Clock> {
            state: Arc<AppState<C>>,
            key_id: String,
            model: String,
            reservation: Option<ReservationId>,
            last_usage: Option<gateway_spine::TokenUsage>,
        }
        impl<C: gateway_spine::Clock> Drop for CommitGuard<C> {
            fn drop(&mut self) {
                if let Some(res) = self.reservation.take() {
                    let cost = {
                        let reg = self.state.registry.read().unwrap();
                        self.last_usage
                            .and_then(|u| reg.cost(&self.model, &u))
                            .unwrap_or(Usd::ZERO)
                    };
                    let _ = self.state.ledger.commit(res, cost);
                    self.state.limiter.release_parallel(&self.key_id);
                    self.state.audit.record(AuditEvent {
                        ts_ms: self.state.clock.now_ms(),
                        actor: self.key_id.clone(),
                        action: "request.complete".into(),
                        target: self.model.clone(),
                        outcome: "ok".into(),
                        detail: Some(format!("{} µUSD (stream)", cost.micros())),
                    });
                }
            }
        }

        let guard = CommitGuard {
            state,
            key_id,
            model,
            reservation: Some(reservation),
            last_usage: None,
        };

        futures::stream::unfold((inner, guard), |(mut inner, mut guard)| async move {
            match inner.next().await {
                Some(Ok(delta)) => {
                    if let Some(u) = delta.usage {
                        guard.last_usage = Some(u);
                    }
                    Some((Ok(delta), (inner, guard)))
                }
                Some(Err(e)) => Some((Err(e), (inner, guard))),
                None => None, // `guard` drops here → commit fires
            }
        })
    }
```

Add the streaming test inside the existing `#[cfg(test)] mod tests` block in `gateway.rs` (append after `provider_error_releases_reservation`):

```rust
    #[tokio::test]
    async fn streaming_commits_from_terminal_delta_usage() {
        use gateway_llm::FinishReason;

        struct StreamProvider {
            usage: TokenUsage,
        }
        #[async_trait]
        impl Provider for StreamProvider {
            fn id(&self) -> &str { "stream" }
            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_streaming: true,
                    supports_tools: false,
                    supports_vision: false,
                    supports_idempotency: true,
                }
            }
            async fn chat(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn stream(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<DeltaStream, ProviderError> {
                let deltas = vec![
                    Ok(StreamDelta::text("hel")),
                    Ok(StreamDelta::text("lo")),
                    Ok(StreamDelta::finish(FinishReason::Stop, self.usage)),
                ];
                Ok(Box::pin(futures::stream::iter(deltas)))
            }
        }

        let mut ks = StaticKeyStore::new();
        ks.insert(key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        let usage = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(StreamProvider { usage }),
                credentials: Arc::new(Credentials::new("x")),
            },
        );
        let state = Arc::new(AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default());
        let completed = Gateway::run_stream(state.clone(), &k, &chat_req()).await.unwrap();

        // drain the whole stream
        let mut s = completed.stream;
        let mut chunks = 0;
        while let Some(item) = s.next().await {
            item.unwrap();
            chunks += 1;
        }
        drop(s); // ensure the commit guard drops
        assert_eq!(chunks, 3);
        assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO);
    }

    #[tokio::test]
    async fn aborted_stream_still_commits_partial_usage() {
        use gateway_llm::FinishReason;

        struct AbortProvider {
            usage: TokenUsage,
        }
        #[async_trait]
        impl Provider for AbortProvider {
            fn id(&self) -> &str { "abort" }
            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_streaming: true,
                    supports_tools: false,
                    supports_vision: false,
                    supports_idempotency: true,
                }
            }
            async fn chat(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn stream(
                &self,
                _req: &ChatRequest,
                _creds: &Credentials,
                _idempotency_key: &str,
            ) -> Result<DeltaStream, ProviderError> {
                // usage arrives on the FIRST delta, then more content would follow
                let deltas = vec![
                    Ok(StreamDelta::finish(FinishReason::Stop, self.usage)),
                    Ok(StreamDelta::text("more")),
                ];
                Ok(Box::pin(futures::stream::iter(deltas)))
            }
        }

        let mut ks = StaticKeyStore::new();
        ks.insert(key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default()));
        let mut providers = ProviderRegistry::new();
        let usage = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(AbortProvider { usage }),
                credentials: Arc::new(Credentials::new("x")),
            },
        );
        let state = Arc::new(AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);

        let k = key(Some(Usd::from_dollars_f64(1.0)), None, RateLimits::default());
        let completed = Gateway::run_stream(state.clone(), &k, &chat_req()).await.unwrap();

        // read ONLY the first delta (which carried usage), then DROP the stream.
        let mut s = completed.stream;
        let first = s.next().await.unwrap();
        first.unwrap();
        drop(s); // abort → CommitGuard drops → commit fires from last_usage

        assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500), "usage not lost on abort");
        assert_eq!(state.ledger.reserved("key_1"), Usd::ZERO, "no stranded reservation");
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p gateway-control gateway::`
Expected: 7 tests PASS (5 from Task 7 + 2 streaming).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/gateway.rs
git commit -s -m "feat(control): streaming lifecycle — commit cost from terminal delta, abort-safe"
```

---

### Task 9: HTTP wire types for `/v1/chat/completions` (OpenAI-shaped request + response)

**Files:**
- Create: `crates/gateway-control/src/wire.rs`
- Modify: `crates/gateway-control/src/lib.rs`

P1.4 ships the OpenAI Chat Completions dialect as the canonical ingress (the others reuse this shape or are stubbed; full dialect translation is P1.3, which already defines the unified types — here we only need the *HTTP-boundary* request/response structs and the conversion to/from `gateway_llm` unified types for the OpenAI shape). To stay in P1.4 scope we define a minimal OpenAI request that maps onto `ChatRequest` and an OpenAI response built from `ChatResponse`, with `usage.cost` included.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/wire.rs`:

```rust
//! HTTP-boundary wire types for the OpenAI `/v1/chat/completions` dialect, and
//! their conversions to/from the unified `gateway_llm` types. P1.3 owns full
//! cross-dialect translation + the fidelity matrix; P1.4 needs just enough to
//! accept an OpenAI request and emit an OpenAI response with `usage.cost`. The
//! Anthropic/Gemini/Responses ingress dialects reuse the unified types via the
//! P1.3 translators; until those land, `/v1/messages` etc. accept the unified
//! shape (documented in the route table).

use gateway_llm::{ChatRequest, ChatResponse, ContentPart, Message, Role};
use gateway_spine::Usd;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct WireChatRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireMessage {
    pub role: String,
    /// OpenAI allows string or array content; P1.4 accepts the string form.
    pub content: String,
}

impl WireChatRequest {
    pub fn to_unified(&self) -> ChatRequest {
        let messages = self
            .messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User,
                };
                Message::text(role, m.content.clone())
            })
            .collect();
        let mut req = ChatRequest::new(self.model.clone(), messages);
        req.temperature = self.temperature;
        req.max_tokens = self.max_tokens;
        req.stream = self.stream;
        req
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WireChatResponse {
    pub id: String,
    pub object: &'static str,
    pub model: String,
    pub choices: Vec<WireChoice>,
    pub usage: WireUsage,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireChoice {
    pub index: i64,
    pub message: WireOutMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireOutMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    /// Oximy extension (design §5): authoritative call-time USD.
    pub cost: f64,
}

impl WireChatResponse {
    /// Build an OpenAI-shaped response from the unified response + committed cost.
    pub fn from_unified(resp: &ChatResponse, cost: Usd) -> Self {
        let content = resp
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let finish_reason = match resp.finish_reason {
            gateway_llm::FinishReason::Stop => "stop",
            gateway_llm::FinishReason::Length => "length",
            gateway_llm::FinishReason::ToolCalls => "tool_calls",
            gateway_llm::FinishReason::ContentFilter => "content_filter",
            gateway_llm::FinishReason::Unknown => "stop",
        };
        WireChatResponse {
            id: resp
                .provider_response_id
                .clone()
                .unwrap_or_else(|| "chatcmpl-oximy".into()),
            object: "chat.completion",
            model: resp.model.clone(),
            choices: vec![WireChoice {
                index: 0,
                message: WireOutMessage { role: "assistant", content },
                finish_reason: finish_reason.into(),
            }],
            usage: WireUsage {
                prompt_tokens: resp.usage.input_tokens + resp.usage.cache_read_tokens,
                completion_tokens: resp.usage.output_tokens,
                total_tokens: resp.usage.total(),
                cost: cost.as_dollars_f64(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::{ChatResponse, FinishReason};
    use gateway_spine::TokenUsage;

    #[test]
    fn request_deserializes_and_maps_to_unified() {
        let json = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"max_tokens":256,"stream":true}"#;
        let wire: WireChatRequest = serde_json::from_str(json).unwrap();
        let unified = wire.to_unified();
        assert_eq!(unified.model, "gpt-4o");
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.max_tokens, Some(256));
        assert!(unified.stream);
        assert_eq!(unified.messages[0].text_content(), "hi");
    }

    #[test]
    fn response_includes_cost_and_openai_shape() {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![ContentPart::text("hello")],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
            provider_response_id: Some("resp_1".into()),
        };
        let wire = WireChatResponse::from_unified(&resp, Usd::from_micros(7_500));
        assert_eq!(wire.object, "chat.completion");
        assert_eq!(wire.choices[0].message.content, "hello");
        assert_eq!(wire.choices[0].finish_reason, "stop");
        assert_eq!(wire.usage.prompt_tokens, 1000);
        assert_eq!(wire.usage.completion_tokens, 500);
        assert_eq!(wire.usage.total_tokens, 1500);
        assert!((wire.usage.cost - 0.0075).abs() < 1e-9);

        // round-trips through serde to the OpenAI JSON shape
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["usage"]["cost"], 0.0075);
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod wire;

pub use wire::{WireChatRequest, WireChatResponse};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control wire::`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/wire.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): OpenAI-shaped wire types with usage.cost extension"
```

---

### Task 10: SSE serialization for streaming chat completions

**Files:**
- Create: `crates/gateway-control/src/sse_out.rs`
- Modify: `crates/gateway-control/src/lib.rs`

The streaming path must emit the OpenAI `data: {chunk}\n\n` SSE event sequence and the terminal `data: [DONE]\n\n` (strict clients reject partial sequences — design §5). We render each `StreamDelta` to an OpenAI chunk JSON.

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-control/src/sse_out.rs`:

```rust
//! Renders unified `StreamDelta`s into the OpenAI `chat.completion.chunk` SSE
//! wire sequence. The exact event order matters: each delta → one
//! `data: {chunk}\n\n`; a terminal `data: [DONE]\n\n` closes the stream (strict
//! SDK clients reject a missing `[DONE]`). The final chunk carries `usage`
//! (incl. the Oximy `cost`) when present — never dropped on abort (the lifecycle
//! commits regardless; this only formats what arrived).

use gateway_llm::StreamDelta;
use gateway_spine::Usd;

/// Format one delta as a single SSE `data:` line block (no trailing `[DONE]`).
pub fn delta_to_sse(model: &str, delta: &StreamDelta, cost: Option<Usd>) -> String {
    let mut chunk = serde_json::json!({
        "id": "chatcmpl-oximy",
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": serde_json::Value::Null,
        }],
    });

    if let Some(text) = &delta.content_delta {
        chunk["choices"][0]["delta"]["content"] = serde_json::Value::String(text.clone());
    }
    if let Some(reason) = delta.finish_reason {
        let s = match reason {
            gateway_llm::FinishReason::Stop => "stop",
            gateway_llm::FinishReason::Length => "length",
            gateway_llm::FinishReason::ToolCalls => "tool_calls",
            gateway_llm::FinishReason::ContentFilter => "content_filter",
            gateway_llm::FinishReason::Unknown => "stop",
        };
        chunk["choices"][0]["finish_reason"] = serde_json::Value::String(s.into());
    }
    if let Some(usage) = &delta.usage {
        chunk["usage"] = serde_json::json!({
            "prompt_tokens": usage.input_tokens + usage.cache_read_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.total(),
            "cost": cost.map(|c| c.as_dollars_f64()),
        });
    }

    format!("data: {}\n\n", chunk)
}

/// The terminal sentinel every OpenAI stream must end with.
pub fn done_event() -> String {
    "data: [DONE]\n\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::FinishReason;
    use gateway_spine::TokenUsage;

    #[test]
    fn content_delta_renders_chunk() {
        let d = StreamDelta::text("hel");
        let s = delta_to_sse("gpt-4o", &d, None);
        assert!(s.starts_with("data: "));
        assert!(s.ends_with("\n\n"));
        let body = s.strip_prefix("data: ").unwrap().trim_end();
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "hel");
    }

    #[test]
    fn terminal_delta_carries_usage_and_cost() {
        let d = StreamDelta::finish(
            FinishReason::Stop,
            TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
        );
        let s = delta_to_sse("gpt-4o", &d, Some(Usd::from_micros(7_500)));
        let body = s.strip_prefix("data: ").unwrap().trim_end();
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["total_tokens"], 1500);
        assert_eq!(v["usage"]["cost"], 0.0075);
    }

    #[test]
    fn done_event_is_exact() {
        assert_eq!(done_event(), "data: [DONE]\n\n");
    }
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod sse_out;

pub use sse_out::{delta_to_sse, done_event};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-control sse_out::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/src/sse_out.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): OpenAI SSE chunk rendering + [DONE] terminal"
```

---

### Task 11: The Axum router + handlers

**Files:**
- Create: `crates/gateway-control/src/server.rs`
- Modify: `crates/gateway-control/src/lib.rs`

Now wire the HTTP surface: build a `Router` over `Arc<AppState>`, with handlers that do only HTTP work (extract bearer, deserialize, call `Gateway::run`/`run_stream`, set the `x-overhead-duration-ms` + `usage.cost` headers, serialize). `/v1/models` lists the registry; `/v1/embeddings` is wired through auth and returns a typed `Unsupported`; `/v1/responses` and `/v1/messages` accept the same OpenAI body for P1.4 (full dialect translation is P1.3) and share the chat handler.

- [ ] **Step 1: Write the failing test (in-process router, no socket)**

Create `crates/gateway-control/src/server.rs`:

```rust
//! The Axum HTTP surface. Handlers are thin: extract the bearer header,
//! deserialize the body, delegate the whole lifecycle to `Gateway`, set the
//! `x-overhead-duration-ms` benchmark header (design §5/§9) and serialize. The
//! router is built over `Arc<AppState<SystemClock>>` for production; tests build
//! it over `Arc<AppState<MockClock>>` and drive it with `tower::ServiceExt`.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::StreamExt;
use gateway_spine::Clock;

use crate::auth::authenticate;
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::sse_out::{delta_to_sse, done_event};
use crate::state::AppState;
use crate::wire::{WireChatRequest, WireChatResponse};

/// Build the full `/v1/*` router over a shared state.
pub fn router<C: Clock + 'static>(state: Arc<AppState<C>>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions::<C>))
        .route("/v1/responses", post(chat_completions::<C>))
        .route("/v1/messages", post(chat_completions::<C>))
        .route("/v1/embeddings", post(embeddings::<C>))
        .route("/v1/models", get(models::<C>))
        .with_state(state)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
}

async fn chat_completions<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Json(body): Json<WireChatRequest>,
) -> Response {
    let started = Instant::now();
    let key = match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let req = body.to_unified();

    if req.stream {
        match Gateway::run_stream(state.clone(), &key, &req).await {
            Ok(completed) => {
                let model = req.model.clone();
                let overhead = started.elapsed().as_millis() as u64;
                let inner = completed.stream;
                // Map each unified delta to an SSE frame, then append [DONE].
                let sse = inner
                    .map(move |item| {
                        let frame = match item {
                            Ok(delta) => delta_to_sse(&model, &delta, None),
                            Err(e) => format!(
                                "data: {}\n\n",
                                serde_json::json!({"error": {"message": e.to_string()}})
                            ),
                        };
                        Ok::<_, std::convert::Infallible>(frame)
                    })
                    .chain(futures::stream::once(async {
                        Ok::<_, std::convert::Infallible>(done_event())
                    }));
                let mut resp = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("x-overhead-duration-ms", overhead.to_string())
                    .header("x-idempotency-key", completed.idempotency_key)
                    .body(Body::from_stream(sse))
                    .unwrap();
                resp.headers_mut().insert(
                    "cache-control",
                    header::HeaderValue::from_static("no-cache"),
                );
                resp
            }
            Err(e) => e.into_response(),
        }
    } else {
        match Gateway::run(&state, &key, &req).await {
            Ok(completed) => {
                let overhead = started.elapsed().as_millis() as u64;
                let wire = WireChatResponse::from_unified(&completed.response, completed.cost);
                let mut resp = Json(wire).into_response();
                resp.headers_mut().insert(
                    "x-overhead-duration-ms",
                    header::HeaderValue::from_str(&overhead.to_string()).unwrap(),
                );
                resp.headers_mut().insert(
                    "x-idempotency-key",
                    header::HeaderValue::from_str(&completed.idempotency_key).unwrap(),
                );
                resp
            }
            Err(e) => e.into_response(),
        }
    }
}

async fn embeddings<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    // Auth still applies (auth-by-default), then a typed 501 until P5.
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => GatewayError::Unsupported("embeddings".into()).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn models<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    let reg = state.registry.read().unwrap();
    // OpenAI `/v1/models` list shape; we add pricing/context (design §5 machine-
    // readable model catalog). Iterate a stable, sorted id list.
    let mut ids: Vec<String> = Vec::new();
    for id in ["__none__"] {
        let _ = id; // placeholder loop replaced below
    }
    // ModelRegistry doesn't expose iteration in P1.1; expose it via a helper.
    let data: Vec<serde_json::Value> = reg
        .ids()
        .into_iter()
        .filter_map(|id| reg.get(&id).map(|e| {
            serde_json::json!({
                "id": e.id,
                "object": "model",
                "owned_by": e.provider,
                "context_window": e.context_window,
                "pricing": {
                    "input_per_mtok_micros": e.price.input_per_mtok,
                    "output_per_mtok_micros": e.price.output_per_mtok,
                },
            })
        }))
        .collect();
    let _ = ids;
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::AllowAll;
    use crate::keystore::StaticKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use gateway_llm::{
        ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
        ProviderCapabilities, ProviderError,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
    };
    use http::Request;
    use tower::ServiceExt;

    struct Echo;
    #[async_trait]
    impl Provider for Echo {
        fn id(&self) -> &str { "echo" }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: true,
                supports_tools: false,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![ContentPart::text("hello")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
                provider_response_id: Some("resp_1".into()),
            })
        }
        async fn stream(
            &self,
            req: &ChatRequest,
            _creds: &Credentials,
            _idempotency_key: &str,
        ) -> Result<DeltaStream, ProviderError> {
            let model = req.model.clone();
            let _ = model;
            let deltas = vec![
                Ok(StreamDelta_text("hel")),
                Ok(StreamDelta_text("lo")),
                Ok(gateway_llm::StreamDelta::finish(
                    FinishReason::Stop,
                    TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
                )),
            ];
            Ok(Box::pin(futures::stream::iter(deltas)))
        }
    }

    // small helper to avoid importing StreamDelta::text under a name clash
    fn StreamDelta_text(s: &str) -> gateway_llm::StreamDelta {
        gateway_llm::StreamDelta::text(s)
    }

    fn gpt4o() -> ModelEntry {
        ModelEntry {
            id: "gpt-4o".into(),
            provider: "openai".into(),
            price: ModelPrice {
                input_per_mtok: 2_500_000,
                output_per_mtok: 10_000_000,
                cache_read_per_mtok: 1_250_000,
                cache_write_per_mtok: 0,
            },
            context_window: Some(128_000),
            max_output_tokens: Some(16_384),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        }
    }

    fn test_state() -> Arc<AppState<MockClock>> {
        let mut ks = StaticKeyStore::new();
        ks.insert(VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        });
        let mut providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment { provider: Arc::new(Echo), credentials: Arc::new(Credentials::new("up")) },
        );
        let state = Arc::new(AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(AllowAll),
            Arc::new(MemoryAudit::new()),
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state.ledger.set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);
        state
    }

    #[tokio::test]
    async fn unauthenticated_chat_is_401() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authenticated_chat_returns_cost_and_overhead_header() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("x-overhead-duration-ms"));
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
        assert_eq!(v["usage"]["cost"], 0.0075);
    }

    #[tokio::test]
    async fn streaming_chat_emits_sse_then_done() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("chat.completion.chunk"));
        assert!(text.contains("\"content\":\"hel\""));
        assert!(text.trim_end().ends_with("data: [DONE]"));
    }

    #[tokio::test]
    async fn models_lists_registry() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["object"], "list");
        assert_eq!(v["data"][0]["id"], "gpt-4o");
    }

    #[tokio::test]
    async fn embeddings_authed_is_501() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"text-embedding-3-small","input":"hi"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
```

> The `/v1/models` handler calls `reg.ids()`, which **P1.1 did not expose**. Add a tiny iteration helper to the spine registry in the next step. (Remove the dead `for id in ["__none__"]` placeholder loop and the `let _ = ids;` — they are written above only to make the intent explicit; the final handler uses `reg.ids()` directly. Delete those two placeholder lines before running.)

- [ ] **Step 2: Add `ModelRegistry::ids()` to the spine**

In `crates/gateway-spine/src/registry.rs`, add inside `impl ModelRegistry` (after `is_empty`):

```rust
    /// Sorted list of all registered model ids (for `/v1/models` + catalog UI).
    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }
```

Add a test in the same file's `#[cfg(test)] mod tests` (after `get_exposes_capabilities`):

```rust
    #[test]
    fn ids_are_sorted() {
        let mut r = ModelRegistry::new();
        let mut e2 = entry();
        e2.id = "zeta".into();
        r.insert(e2);
        r.insert(entry()); // "gpt-4o"
        assert_eq!(r.ids(), vec!["gpt-4o".to_string(), "zeta".to_string()]);
    }
```

Then remove the placeholder lines from `models` in `server.rs` so it reads cleanly:

```rust
async fn models<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    let reg = state.registry.read().unwrap();
    let data: Vec<serde_json::Value> = reg
        .ids()
        .into_iter()
        .filter_map(|id| {
            reg.get(&id).map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "object": "model",
                    "owned_by": e.provider,
                    "context_window": e.context_window,
                    "pricing": {
                        "input_per_mtok_micros": e.price.input_per_mtok,
                        "output_per_mtok_micros": e.price.output_per_mtok,
                    },
                })
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}
```

Add to `crates/gateway-control/src/lib.rs`:

```rust
pub mod server;

pub use server::router;
```

- [ ] **Step 3: Run tests (spine + control)**

Run: `cargo test -p gateway-spine registry::` then `cargo test -p gateway-control server::`
Expected: spine registry now 4 tests PASS; control server 5 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-spine/src/registry.rs crates/gateway-control/src/server.rs crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): axum router + chat/models/embeddings handlers with overhead header"
```

---

### Task 12: End-to-end integration test — the full ingress over a mocked provider

**Files:**
- Create: `crates/gateway-control/tests/http_lifecycle.rs`

A black-box test that exercises the assembled router exactly as an OpenAI SDK would: a budget-blocked request returns 429 without provider egress; a successful request bills the ledger; a second request after spend respects the remaining budget. This is the proof the wire ↔ spine ↔ egress wiring holds.

- [ ] **Step 1: Write the test**

Create `crates/gateway-control/tests/http_lifecycle.rs`:

```rust
//! Black-box HTTP test: drive the assembled `/v1/*` router with `tower::oneshot`
//! and assert the governance lifecycle is observable over the wire — auth,
//! fail-closed budget (429, no egress), authoritative cost in the body, and
//! budget depletion across requests. Mirrors what an OpenAI SDK sees.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use gateway_control::guard::AllowAll;
use gateway_control::keystore::StaticKeyStore;
use gateway_control::providers::{Deployment, ProviderRegistry};
use gateway_control::server::router;
use gateway_control::state::AppState;
use gateway_llm::{
    ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
    ProviderCapabilities, ProviderError,
};
use gateway_spine::{
    MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
};
use http::Request;
use tower::ServiceExt;

struct Counting {
    calls: AtomicUsize,
}

#[async_trait]
impl Provider for Counting {
    fn id(&self) -> &str {
        "counting"
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: false,
            supports_tools: false,
            supports_vision: false,
            supports_idempotency: true,
        }
    }
    async fn chat(
        &self,
        req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ChatResponse {
            model: req.model.clone(),
            content: vec![ContentPart::text("ok")],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() },
            provider_response_id: Some("r".into()),
        })
    }
    async fn stream(
        &self,
        _req: &ChatRequest,
        _creds: &Credentials,
        _idempotency_key: &str,
    ) -> Result<DeltaStream, ProviderError> {
        unreachable!()
    }
}

fn gpt4o() -> ModelEntry {
    ModelEntry {
        id: "gpt-4o".into(),
        provider: "openai".into(),
        price: ModelPrice {
            input_per_mtok: 2_500_000,
            output_per_mtok: 10_000_000,
            cache_read_per_mtok: 1_250_000,
            cache_write_per_mtok: 0,
        },
        context_window: Some(128_000),
        max_output_tokens: Some(16_384),
        supports_tools: true,
        supports_vision: true,
        supports_streaming: true,
    }
}

fn build(budget: Usd) -> (Arc<AppState<MockClock>>, Arc<Counting>) {
    let provider = Arc::new(Counting { calls: AtomicUsize::new(0) });
    let mut ks = StaticKeyStore::new();
    ks.insert(VirtualKey {
        id: "key_1".into(),
        token_hash: VirtualKey::hash_secret("sk-good"),
        token_prefix: "sk-good".into(),
        max_budget: Some(budget),
        limits: RateLimits::default(),
        model_allowlist: None,
        expires_at: None,
        revoked: false,
        parent_id: None,
    });
    let mut providers = ProviderRegistry::new();
    providers.insert(
        "openai",
        Deployment { provider: provider.clone(), credentials: Arc::new(Credentials::new("up")) },
    );
    let state = Arc::new(AppState::with_parts(
        Arc::new(ks),
        Arc::new(MockClock::new(0)),
        providers,
        Arc::new(AllowAll),
        Arc::new(MemoryAudit::new()),
    ));
    state.registry.write().unwrap().insert(gpt4o());
    state.ledger.set_budget("key_1", Some(budget), Usd::ZERO);
    (state, provider)
}

fn chat_body() -> Body {
    Body::from(r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#)
}

#[tokio::test]
async fn successful_request_bills_and_returns_cost() {
    let (state, provider) = build(Usd::from_dollars_f64(10.0));
    let app = router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", "Bearer sk-good")
                .header("content-type", "application/json")
                .body(chat_body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["usage"]["cost"], 0.0075);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.ledger.spent("key_1"), Usd::from_micros(7_500));
}

#[tokio::test]
async fn budget_blocked_request_is_429_without_egress() {
    // budget so small the worst-case reserve fails before any call
    let (state, provider) = build(Usd::from_micros(1));
    let app = router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", "Bearer sk-good")
                .header("content-type", "application/json")
                .body(chat_body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0, "fail-closed: provider never called");
    assert_eq!(state.ledger.spent("key_1"), Usd::ZERO);
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p gateway-control --test http_lifecycle`
Expected: 2 tests PASS.

- [ ] **Step 3: Run the whole crate + commit**

Run: `cargo test -p gateway-control`
Expected: all unit tests + `http_lifecycle` PASS.

```bash
cargo fmt --all && cargo clippy -p gateway-control --all-targets -- -D warnings
git add crates/gateway-control/tests/http_lifecycle.rs
git commit -s -m "test(control): black-box HTTP lifecycle — auth, fail-closed 429, billed cost"
```

---

### Task 13: Finalize `lib.rs` module surface + workspace gate

**Files:**
- Modify: `crates/gateway-control/src/lib.rs`

- [ ] **Step 1: Confirm the full module surface**

Ensure `crates/gateway-control/src/lib.rs` reads exactly (doc comment + `#![forbid(unsafe_code)]`, all modules, the key re-exports, no `CRATE` placeholder):

```rust
//! # gateway-control
//!
//! The HTTP ingress + per-request governance lifecycle over the spine. Three
//! thin clients (REST API, admin-MCP, CLI) share one core; P1.4 ships the REST
//! data-plane (`/v1/*`) and the lifecycle that wires the spine (auth, budgets,
//! rate limits, audit) to the `gateway-llm` egress (streaming, idempotency).
//! Admin CRUD / admin-MCP / CLI land in P1.6 and P3.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` (§2 invariants, §6 lifecycle).

#![forbid(unsafe_code)]

pub mod auth;
pub mod error;
pub mod gateway;
pub mod guard;
pub mod keystore;
pub mod providers;
pub mod server;
pub mod sse_out;
pub mod state;
pub mod wire;

pub use auth::{authenticate, parse_bearer};
pub use error::GatewayError;
pub use gateway::{Completed, CompletedStream, Gateway};
pub use guard::{AllowAll, GuardHook, GuardVerdict};
pub use keystore::{KeyStore, StaticKeyStore};
pub use providers::{Deployment, ProviderRegistry};
pub use server::router;
pub use sse_out::{delta_to_sse, done_event};
pub use state::AppState;
pub use wire::{WireChatRequest, WireChatResponse};
```

- [ ] **Step 2: Full workspace gate**

Run:
```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test -p gateway-spine
cargo test -p gateway-control
```
Expected: fmt clean; clippy clean across the workspace; both crates fully green.

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-control/src/lib.rs
git commit -s -m "feat(control): finalize gateway-control module surface for P1.4"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-control` is fully green (unit modules: `error`, `keystore`, `providers`, `guard`, `state`, `auth`, `gateway`, `wire`, `sse_out`, `server` + the `http_lifecycle` integration test).
- [ ] `cargo test -p gateway-spine` still green (the `Arc<C>: Clock` impl, `RateLimiter::now_ms`, and `ModelRegistry::ids()` additions did not regress P1.1).
- [ ] `cargo clippy --all-targets -- -D warnings` clean workspace-wide; `cargo fmt --all --check` clean.
- [ ] **Auth-by-default proven:** every `/v1/*` route returns 401 with no bearer key, and the `MockProvider` records zero calls on any auth failure.
- [ ] **Fail-closed proven over HTTP:** a budget-blocked `/v1/chat/completions` returns 429 with the provider never called and the ledger un-billed (`http_lifecycle::budget_blocked_request_is_429_without_egress`).
- [ ] **Commit-once / true-up proven:** a successful request bills exactly the registry cost from provider usage, leaves zero reserved, and surfaces `usage.cost` in the body (`http_lifecycle::successful_request_bills_and_returns_cost`).
- [ ] **No-lost-usage-on-abort proven:** dropping a stream after the usage-bearing delta still commits the cost and releases the reservation (`gateway::aborted_stream_still_commits_partial_usage`).
- [ ] **Streaming sequence correct:** the SSE response is `chat.completion.chunk` frames terminated by `data: [DONE]` (`server::streaming_chat_emits_sse_then_done`).
- [ ] `x-overhead-duration-ms` header present on every chat response (streaming + non-streaming).

**Interfaces this milestone EXPOSES for later milestones** (import via `use gateway_control::...`):
- `AppState<C: Clock>` — the shared service container (`registry: RwLock<ModelRegistry>`, `ledger`, `limiter`, `keys`, `providers`, `guard`, `audit`, `clock`). **P1.5** adds a `cache` field + `CacheHook` seam; **P1.6** swaps `keys`/persistence behind the same `dyn KeyStore`; **P1.7** swaps `audit` for the columnar sink; **P1.8** builds it in the binary.
- `Gateway::run` / `Gateway::run_stream` — the socket-free lifecycle. **P1.5** inserts the cache lookup + routing/fallback array into this exact call order (cache short-circuit before reserve; fallback only before first token). **P1.7** taps the commit point for the telemetry write.
- `KeyStore` trait + `StaticKeyStore` — **P1.6** implements the persistent CRUD store against this trait.
- `GuardHook` trait + `AllowAll` — **P4** implements real PII/injection/moderation stages here.
- `ProviderRegistry` / `Deployment` — **P1.5** grows this into multi-deployment + fallback; **P1.8** populates it from config (P1.6).
- `GatewayError` — the canonical `SpineError`/`ProviderError` → HTTP-status mapping every other handler reuses.
- `router(Arc<AppState<C>>) -> axum::Router` — **P1.7** layers a Prometheus/metrics route + telemetry middleware onto it; **P1.8** nests the dashboard and `oximy-gateway up` serves it.
- Spine additions (re-exported from `gateway_spine`): `impl Clock for Arc<C>`, `RateLimiter::now_ms`, `ModelRegistry::ids()`.

**Next:** `2026-06-10-p1-05-cache-and-registry-reload.md` — the exact-match cache (200s only, streaming replay, `HIT/MISS/age` headers) inserted into `Gateway::run`/`run_stream` before the budget reserve, plus hot-reload of the model registry from `models.dev`/local overrides into `AppState.registry`.
