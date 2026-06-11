# Phase 1.1 — Spine Foundation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the pure, in-memory core of `gateway-spine` — money/usage/cost types, a model pricing+capability registry, the virtual-key model, the **atomic budget ledger** (reserve/commit/release with no overspend under concurrency), the rate limiter, and the audit log — with the spine invariants proven by tests.

**Architecture:** Pure domain logic, no I/O. Money is integer-only (**µUSD**, 1 USD = 1_000_000 µUSD); model prices are `i64` µUSD-per-million-tokens so all cost math is exact integer arithmetic. State is in-memory behind `Mutex` with trait seams (`Clock`, `AuditSink`) so persistence (P1.6) and distribution swap in later without touching this logic. Unknown models yield `None` cost — never a guess (cost-correctness invariant).

**Tech Stack:** Rust 2024, `thiserror`, `serde`, `sha2`+`hex` (key hashing), `rand` (key generation). Tests use `std::thread`/`Arc` for concurrency proofs and a `MockClock` for time.

**Invariants this milestone enforces (design §2):** fail-closed budgets · no overspend under concurrency · cost-correctness (unknown → NULL). (No-double-billing and auth-by-default land in P1.4 where the request lifecycle lives; the budget ledger here provides the commit-once primitive.)

---

### Task 1: Add dependencies to `gateway-spine`

**Files:**
- Modify: `crates/gateway-spine/Cargo.toml`
- Modify: `Cargo.toml` (workspace — add shared dep versions)

- [ ] **Step 1: Add the dep versions to the workspace `[workspace.dependencies]`**

In root `Cargo.toml`, add under `[workspace.dependencies]` (after the existing `serde_json = "1"` line):

```toml
sha2 = "0.10"
hex = "0.4"
rand = "0.8"
```

- [ ] **Step 2: Reference them from `gateway-spine/Cargo.toml`**

Replace the `[dependencies]` section of `crates/gateway-spine/Cargo.toml` with:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
rand = { workspace = true }
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p gateway-spine`
Expected: builds (still the scaffold `lib.rs`).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/gateway-spine/Cargo.toml Cargo.lock
git commit -s -m "build(spine): add sha2, hex, rand deps"
```

---

### Task 2: `Usd` money type (µUSD, integer-only)

**Files:**
- Create: `crates/gateway-spine/src/money.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/money.rs`:

```rust
//! Integer-only USD. Unit: micro-dollars (µUSD); 1 USD = 1_000_000 µUSD.
//! Floats are never used for money — only `from_dollars_f64`/`as_dollars_f64`
//! exist for display and test ergonomics, never for accumulation.

use std::iter::Sum;
use std::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
    serde::Serialize, serde::Deserialize,
)]
pub struct Usd(i64);

impl Usd {
    pub const ZERO: Usd = Usd(0);

    pub const fn from_micros(micros: i64) -> Self {
        Usd(micros)
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    /// For tests/display only. Rounds to the nearest µUSD.
    pub fn from_dollars_f64(dollars: f64) -> Self {
        Usd((dollars * 1_000_000.0).round() as i64)
    }

    pub fn as_dollars_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

impl Add for Usd {
    type Output = Usd;
    fn add(self, rhs: Usd) -> Usd {
        Usd(self.0 + rhs.0)
    }
}
impl Sub for Usd {
    type Output = Usd;
    fn sub(self, rhs: Usd) -> Usd {
        Usd(self.0 - rhs.0)
    }
}
impl AddAssign for Usd {
    fn add_assign(&mut self, rhs: Usd) {
        self.0 += rhs.0;
    }
}
impl SubAssign for Usd {
    fn sub_assign(&mut self, rhs: Usd) {
        self.0 -= rhs.0;
    }
}
impl Sum for Usd {
    fn sum<I: Iterator<Item = Usd>>(iter: I) -> Usd {
        Usd(iter.map(|u| u.0).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micros_roundtrip() {
        let one_dollar = Usd::from_micros(1_000_000);
        assert_eq!(one_dollar.micros(), 1_000_000);
        assert_eq!(one_dollar.as_dollars_f64(), 1.0);
    }

    #[test]
    fn arithmetic_is_exact() {
        let mut total = Usd::ZERO;
        for _ in 0..3 {
            total += Usd::from_micros(100_000); // $0.10 each
        }
        assert_eq!(total, Usd::from_micros(300_000)); // exactly $0.30
        assert_eq!(total - Usd::from_micros(50_000), Usd::from_micros(250_000));
    }

    #[test]
    fn sum_iterator() {
        let v = [Usd::from_micros(1), Usd::from_micros(2), Usd::from_micros(3)];
        let s: Usd = v.into_iter().sum();
        assert_eq!(s, Usd::from_micros(6));
    }

    #[test]
    fn ordering() {
        assert!(Usd::from_micros(1) < Usd::from_micros(2));
        assert!(Usd::ZERO < Usd::from_micros(1));
    }
}
```

Add to `crates/gateway-spine/src/lib.rs` (after the `#![forbid(unsafe_code)]` line, replacing the `CRATE` placeholder block):

```rust
pub mod money;

pub use money::Usd;
```

- [ ] **Step 2: Run test to verify it fails, then passes once compiled**

Run: `cargo test -p gateway-spine money::`
Expected: compiles and 4 tests PASS. (If it fails to compile, fix before moving on.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/money.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): integer-only Usd money type in µUSD"
```

---

### Task 3: `TokenUsage` type

**Files:**
- Create: `crates/gateway-spine/src/usage.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/usage.rs`:

```rust
//! Token counts for one LLM call, in non-overlapping categories so cost is a
//! simple dot-product with prices. Providers that report overlapping buckets
//! (e.g. prompt_tokens that include cached) are normalized into these at the
//! translation layer (P1.3), never here.

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct TokenUsage {
    /// Uncached input tokens (excludes cache reads/writes).
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Tokens served from a provider prompt cache (billed at the read rate).
    pub cache_read_tokens: i64,
    /// Tokens written into a provider prompt cache (billed at the write rate).
    pub cache_write_tokens: i64,
}

impl TokenUsage {
    pub fn total(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_sums_all_categories() {
        let u = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
        };
        assert_eq!(u.total(), 165);
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(TokenUsage::default().total(), 0);
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod usage;

pub use usage::TokenUsage;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine usage::`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/usage.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): TokenUsage with non-overlapping categories"
```

---

### Task 4: `ModelPrice` + exact integer cost

**Files:**
- Create: `crates/gateway-spine/src/pricing.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/pricing.rs`:

```rust
//! Model prices, stored as i64 µUSD-per-million-tokens so cost is exact integer
//! arithmetic. Example: $3.00 / 1M input tokens → 3_000_000 µUSD per million →
//! `input_per_mtok = 3_000_000`. $0.075 / 1M → `75_000`.

use crate::money::Usd;
use crate::usage::TokenUsage;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct ModelPrice {
    pub input_per_mtok: i64,
    pub output_per_mtok: i64,
    pub cache_read_per_mtok: i64,
    pub cache_write_per_mtok: i64,
}

impl ModelPrice {
    /// Cost of a usage record. Each line item is rounded half-up to the µUSD
    /// independently and summed. i128 intermediate prevents overflow.
    pub fn cost(&self, u: &TokenUsage) -> Usd {
        fn line(price_per_mtok: i64, tokens: i64) -> i64 {
            let numerator = price_per_mtok as i128 * tokens as i128;
            // round half up; tokens and prices are non-negative
            let micros = (numerator + 500_000) / 1_000_000;
            micros as i64
        }
        Usd::from_micros(
            line(self.input_per_mtok, u.input_tokens)
                + line(self.output_per_mtok, u.output_tokens)
                + line(self.cache_read_per_mtok, u.cache_read_tokens)
                + line(self.cache_write_per_mtok, u.cache_write_tokens),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GPT-4o-class: $2.50/M in, $10.00/M out.
    fn price() -> ModelPrice {
        ModelPrice {
            input_per_mtok: 2_500_000,
            output_per_mtok: 10_000_000,
            cache_read_per_mtok: 1_250_000,
            cache_write_per_mtok: 0,
        }
    }

    #[test]
    fn cost_of_round_numbers() {
        // 1M input + 1M output = $2.50 + $10.00 = $12.50
        let u = TokenUsage { input_tokens: 1_000_000, output_tokens: 1_000_000, ..Default::default() };
        assert_eq!(price().cost(&u), Usd::from_dollars_f64(12.5));
    }

    #[test]
    fn cost_of_small_call() {
        // 1000 in + 500 out = $0.0025 + $0.005 = $0.0075 = 7_500 µUSD
        let u = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
        assert_eq!(price().cost(&u), Usd::from_micros(7_500));
    }

    #[test]
    fn cache_reads_are_discounted() {
        // 10_000 cache-read tokens at $1.25/M = $0.0125 = 12_500 µUSD
        let u = TokenUsage { cache_read_tokens: 10_000, ..Default::default() };
        assert_eq!(price().cost(&u), Usd::from_micros(12_500));
    }

    #[test]
    fn zero_usage_is_zero_cost() {
        assert_eq!(price().cost(&TokenUsage::default()), Usd::ZERO);
    }

    #[test]
    fn sub_micro_prices_round_half_up() {
        // $0.075/M input, 1 token → 0.075 µUSD → rounds to 0; 7 tokens → 0.525 → 1
        let p = ModelPrice { input_per_mtok: 75_000, ..Default::default() };
        assert_eq!(p.cost(&TokenUsage { input_tokens: 1, ..Default::default() }), Usd::from_micros(0));
        assert_eq!(p.cost(&TokenUsage { input_tokens: 7, ..Default::default() }), Usd::from_micros(1));
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod pricing;

pub use pricing::ModelPrice;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine pricing::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/pricing.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): ModelPrice with exact integer cost computation"
```

---

### Task 5: `ModelRegistry` with unknown-model = NULL discipline

**Files:**
- Create: `crates/gateway-spine/src/registry.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/registry.rs`:

```rust
//! In-memory model registry: id → price + capabilities. Unknown models return
//! `None` cost — the gateway never guesses a price (cost-correctness invariant).
//! Hot-reload from models.dev / local overrides lands in P1.5; this is the
//! lookup core it will populate.

use std::collections::HashMap;

use crate::money::Usd;
use crate::pricing::ModelPrice;
use crate::usage::TokenUsage;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub price: ModelPrice,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_streaming: bool,
}

#[derive(Debug, Default)]
pub struct ModelRegistry {
    entries: HashMap<String, ModelEntry>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: ModelEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    pub fn get(&self, model_id: &str) -> Option<&ModelEntry> {
        self.entries.get(model_id)
    }

    /// `None` if the model is unknown — never a guessed price.
    pub fn cost(&self, model_id: &str, usage: &TokenUsage) -> Option<Usd> {
        self.entries.get(model_id).map(|e| e.price.cost(usage))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ModelEntry {
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

    #[test]
    fn known_model_returns_cost() {
        let mut r = ModelRegistry::new();
        r.insert(entry());
        let u = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
        assert_eq!(r.cost("gpt-4o", &u), Some(Usd::from_micros(7_500)));
    }

    #[test]
    fn unknown_model_returns_none_not_zero() {
        let r = ModelRegistry::new();
        let u = TokenUsage { input_tokens: 1000, ..Default::default() };
        assert_eq!(r.cost("mystery-model", &u), None);
    }

    #[test]
    fn get_exposes_capabilities() {
        let mut r = ModelRegistry::new();
        r.insert(entry());
        assert!(r.get("gpt-4o").unwrap().supports_tools);
        assert_eq!(r.get("gpt-4o").unwrap().context_window, Some(128_000));
        assert!(r.get("nope").is_none());
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod registry;

pub use registry::{ModelEntry, ModelRegistry};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine registry::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/registry.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): ModelRegistry with unknown-model NULL discipline"
```

---

### Task 6: `SpineError` taxonomy

**Files:**
- Create: `crates/gateway-spine/src/error.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/error.rs`:

```rust
//! The spine's error taxonomy. These map to HTTP statuses at the server layer
//! (P1.4): BudgetExceeded/RateLimited → 429, Key* → 401/403, ModelNotAllowed →
//! 403, UnknownModel → 400.

use crate::money::Usd;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RateDimension {
    Requests,
    Tokens,
    Parallel,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SpineError {
    #[error("budget exceeded for key {key_id}: would spend {would_spend_micros} µUSD of {budget_micros} µUSD")]
    BudgetExceeded {
        key_id: String,
        would_spend_micros: i64,
        budget_micros: i64,
    },
    #[error("rate limit exceeded for key {key_id}: {dimension:?}")]
    RateLimited {
        key_id: String,
        dimension: RateDimension,
    },
    #[error("key {key_id} is revoked")]
    KeyRevoked { key_id: String },
    #[error("key {key_id} has expired")]
    KeyExpired { key_id: String },
    #[error("model {model} is not allowed for key {key_id}")]
    ModelNotAllowed { key_id: String, model: String },
    #[error("unknown model: {model}")]
    UnknownModel { model: String },
    #[error("no such reservation")]
    NoSuchReservation,
    #[error("no such key: {key_id}")]
    NoSuchKey { key_id: String },
}

impl SpineError {
    pub fn budget_exceeded(key_id: &str, would_spend: Usd, budget: Usd) -> Self {
        SpineError::BudgetExceeded {
            key_id: key_id.to_string(),
            would_spend_micros: would_spend.micros(),
            budget_micros: budget.micros(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_exceeded_constructor_and_display() {
        let e = SpineError::budget_exceeded("k1", Usd::from_micros(1_100_000), Usd::from_micros(1_000_000));
        assert!(matches!(e, SpineError::BudgetExceeded { .. }));
        assert!(e.to_string().contains("1100000 µUSD of 1000000"));
    }

    #[test]
    fn rate_dimension_in_message() {
        let e = SpineError::RateLimited { key_id: "k".into(), dimension: RateDimension::Tokens };
        assert!(e.to_string().contains("Tokens"));
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod error;

pub use error::{RateDimension, SpineError};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine error::`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/error.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): SpineError taxonomy"
```

---

### Task 7: `VirtualKey` model + hashing/validation

**Files:**
- Create: `crates/gateway-spine/src/key.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/key.rs`:

```rust
//! The virtual key: the unit of governance. The secret is never stored — only
//! its SHA-256 hex hash and a display prefix. A key carries an optional USD
//! budget, rate limits, a model allowlist, and an expiry; revocation and expiry
//! make it unusable. (Persistence + parent/child attenuation: P1.6 / P3.)

use sha2::{Digest, Sha256};

use crate::error::SpineError;
use crate::money::Usd;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct RateLimits {
    /// Requests per minute.
    pub rpm: Option<i64>,
    /// Tokens per minute.
    pub tpm: Option<i64>,
    /// Max concurrent in-flight requests.
    pub max_parallel: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualKey {
    pub id: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub max_budget: Option<Usd>,
    pub limits: RateLimits,
    /// `None` = all models allowed.
    pub model_allowlist: Option<Vec<String>>,
    /// Unix epoch millis; `None` = never expires.
    pub expires_at: Option<i64>,
    pub revoked: bool,
    pub parent_id: Option<String>,
}

impl VirtualKey {
    /// SHA-256 hex of a secret. The one place hashing happens.
    pub fn hash_secret(secret: &str) -> String {
        let mut h = Sha256::new();
        h.update(secret.as_bytes());
        hex::encode(h.finalize())
    }

    pub fn verify(&self, secret: &str) -> bool {
        Self::hash_secret(secret) == self.token_hash
    }

    /// Usable = not revoked and not past expiry at `now_ms`.
    pub fn ensure_usable(&self, now_ms: i64) -> Result<(), SpineError> {
        if self.revoked {
            return Err(SpineError::KeyRevoked { key_id: self.id.clone() });
        }
        if let Some(exp) = self.expires_at {
            if now_ms >= exp {
                return Err(SpineError::KeyExpired { key_id: self.id.clone() });
            }
        }
        Ok(())
    }

    pub fn allows_model(&self, model: &str) -> bool {
        match &self.model_allowlist {
            None => true,
            Some(list) => list.iter().any(|m| m == model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> VirtualKey {
        VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-secret"),
            token_prefix: "sk-secre".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[test]
    fn hash_is_deterministic_and_hides_secret() {
        let h = VirtualKey::hash_secret("sk-secret");
        assert_eq!(h, VirtualKey::hash_secret("sk-secret"));
        assert_ne!(h, "sk-secret");
        assert_eq!(h.len(), 64); // sha256 hex
    }

    #[test]
    fn verify_matches_only_correct_secret() {
        let k = key();
        assert!(k.verify("sk-secret"));
        assert!(!k.verify("sk-wrong"));
    }

    #[test]
    fn revoked_key_is_unusable() {
        let mut k = key();
        k.revoked = true;
        assert!(matches!(k.ensure_usable(0), Err(SpineError::KeyRevoked { .. })));
    }

    #[test]
    fn expired_key_is_unusable() {
        let mut k = key();
        k.expires_at = Some(1000);
        assert!(k.ensure_usable(999).is_ok());
        assert!(matches!(k.ensure_usable(1000), Err(SpineError::KeyExpired { .. })));
    }

    #[test]
    fn allowlist_gates_models() {
        let mut k = key();
        assert!(k.allows_model("anything")); // None = all
        k.model_allowlist = Some(vec!["gpt-4o".into()]);
        assert!(k.allows_model("gpt-4o"));
        assert!(!k.allows_model("claude-3-5-sonnet"));
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod key;

pub use key::{RateLimits, VirtualKey};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine key::`
Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/key.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): VirtualKey model with hashing, expiry, allowlist"
```

---

### Task 8: `BudgetLedger` — atomic reserve/commit/release (single-threaded behavior)

**Files:**
- Create: `crates/gateway-spine/src/budget.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/budget.rs`:

```rust
//! The atomic budget ledger. Each key tracks `spent` and `reserved` under one
//! lock. `reserve` is FAIL-CLOSED: if the reservation would push
//! spent + reserved over the budget, it errors *before* any upstream call.
//! `commit` trues-up actual vs the estimate; `release` drops a reservation that
//! never billed. Unlimited budget (`None`) always reserves. In-memory for P1.1;
//! P1.6 swaps the backing store, P-later distributes it.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::SpineError;
use crate::money::Usd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservationId(u64);

#[derive(Debug, Default)]
struct KeyBudget {
    budget: Option<Usd>,
    spent: Usd,
    reserved: Usd,
}

#[derive(Debug, Clone)]
struct Reservation {
    key_id: String,
    estimate: Usd,
}

#[derive(Default)]
struct Inner {
    budgets: HashMap<String, KeyBudget>,
    reservations: HashMap<u64, Reservation>,
    next_res: u64,
}

#[derive(Default)]
pub struct BudgetLedger {
    inner: Mutex<Inner>,
}

impl BudgetLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed or restore a key's budget and prior spend.
    pub fn set_budget(&self, key_id: &str, budget: Option<Usd>, spent: Usd) {
        let mut g = self.inner.lock().unwrap();
        g.budgets
            .insert(key_id.to_string(), KeyBudget { budget, spent, reserved: Usd::ZERO });
    }

    /// FAIL-CLOSED reservation. Errors if the key is unknown or would overspend.
    pub fn reserve(&self, key_id: &str, estimate: Usd) -> Result<ReservationId, SpineError> {
        let mut g = self.inner.lock().unwrap();
        {
            let kb = g
                .budgets
                .get_mut(key_id)
                .ok_or_else(|| SpineError::NoSuchKey { key_id: key_id.to_string() })?;
            if let Some(budget) = kb.budget {
                let would = kb.spent + kb.reserved + estimate;
                if would > budget {
                    return Err(SpineError::budget_exceeded(key_id, would, budget));
                }
            }
            kb.reserved += estimate;
        }
        let id = g.next_res;
        g.next_res += 1;
        g.reservations
            .insert(id, Reservation { key_id: key_id.to_string(), estimate });
        Ok(ReservationId(id))
    }

    /// Commit a reservation with the ACTUAL cost (true-up). Reserved -= estimate,
    /// spent += actual.
    pub fn commit(&self, res: ReservationId, actual: Usd) -> Result<(), SpineError> {
        let mut g = self.inner.lock().unwrap();
        let r = g.reservations.remove(&res.0).ok_or(SpineError::NoSuchReservation)?;
        let kb = g
            .budgets
            .get_mut(&r.key_id)
            .ok_or_else(|| SpineError::NoSuchKey { key_id: r.key_id.clone() })?;
        kb.reserved -= r.estimate;
        kb.spent += actual;
        Ok(())
    }

    /// Drop a reservation that never billed (e.g. request failed pre-call).
    pub fn release(&self, res: ReservationId) -> Result<(), SpineError> {
        let mut g = self.inner.lock().unwrap();
        let r = g.reservations.remove(&res.0).ok_or(SpineError::NoSuchReservation)?;
        if let Some(kb) = g.budgets.get_mut(&r.key_id) {
            kb.reserved -= r.estimate;
        }
        Ok(())
    }

    pub fn spent(&self, key_id: &str) -> Usd {
        let g = self.inner.lock().unwrap();
        g.budgets.get(key_id).map(|kb| kb.spent).unwrap_or(Usd::ZERO)
    }

    pub fn reserved(&self, key_id: &str) -> Usd {
        let g = self.inner.lock().unwrap();
        g.budgets.get(key_id).map(|kb| kb.reserved).unwrap_or(Usd::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_cannot_reserve() {
        let l = BudgetLedger::new();
        assert!(matches!(l.reserve("ghost", Usd::from_micros(1)), Err(SpineError::NoSuchKey { .. })));
    }

    #[test]
    fn unlimited_budget_always_reserves() {
        let l = BudgetLedger::new();
        l.set_budget("k", None, Usd::ZERO);
        for _ in 0..1000 {
            assert!(l.reserve("k", Usd::from_dollars_f64(1000.0)).is_ok());
        }
    }

    #[test]
    fn reserve_commit_trues_up_actual() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        let r = l.reserve("k", Usd::from_dollars_f64(0.50)).unwrap();
        assert_eq!(l.reserved("k"), Usd::from_dollars_f64(0.50));
        // actual came in lower than the estimate
        l.commit(r, Usd::from_dollars_f64(0.30)).unwrap();
        assert_eq!(l.reserved("k"), Usd::ZERO);
        assert_eq!(l.spent("k"), Usd::from_dollars_f64(0.30));
    }

    #[test]
    fn reserve_is_fail_closed_at_budget() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        // reserve $0.60 then $0.60 → second must fail (0.60 + 0.60 > 1.00)
        let _r1 = l.reserve("k", Usd::from_dollars_f64(0.60)).unwrap();
        assert!(matches!(
            l.reserve("k", Usd::from_dollars_f64(0.60)),
            Err(SpineError::BudgetExceeded { .. })
        ));
    }

    #[test]
    fn release_frees_the_reservation() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        let r = l.reserve("k", Usd::from_dollars_f64(0.90)).unwrap();
        l.release(r).unwrap();
        assert_eq!(l.reserved("k"), Usd::ZERO);
        // full budget available again
        assert!(l.reserve("k", Usd::from_dollars_f64(0.90)).is_ok());
    }

    #[test]
    fn double_commit_is_rejected() {
        let l = BudgetLedger::new();
        l.set_budget("k", None, Usd::ZERO);
        let r = l.reserve("k", Usd::from_micros(1)).unwrap();
        l.commit(r, Usd::from_micros(1)).unwrap();
        assert!(matches!(l.commit(r, Usd::from_micros(1)), Err(SpineError::NoSuchReservation)));
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod budget;

pub use budget::{BudgetLedger, ReservationId};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine budget::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/budget.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): atomic BudgetLedger with fail-closed reserve/commit/release"
```

---

### Task 9: Prove the no-overspend-under-concurrency invariant

**Files:**
- Create: `crates/gateway-spine/tests/budget_concurrency.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/tests/budget_concurrency.rs`:

```rust
//! Invariant proof (design §2): under heavy concurrent contention the ledger
//! must NEVER let committed spend exceed the budget, and must grant exactly the
//! number of reservations the budget allows.

use std::sync::Arc;
use std::thread;

use gateway_spine::{BudgetLedger, Usd};

#[test]
fn never_overspends_under_concurrency() {
    let ledger = Arc::new(BudgetLedger::new());
    // $1.00 budget; each call costs exactly $0.10 → at most 10 may succeed.
    ledger.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
    let cost = Usd::from_dollars_f64(0.10);

    let mut handles = Vec::new();
    for _ in 0..200 {
        let l = Arc::clone(&ledger);
        handles.push(thread::spawn(move || match l.reserve("k", cost) {
            Ok(r) => {
                // Always commit the full estimate (worst case for overspend).
                l.commit(r, cost).unwrap();
                true
            }
            Err(_) => false,
        }));
    }

    let successes = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|&ok| ok)
        .count();

    assert_eq!(successes, 10, "exactly 10 reservations of $0.10 fit in $1.00");
    assert_eq!(ledger.spent("k"), Usd::from_dollars_f64(1.0), "never overspends");
    assert_eq!(ledger.reserved("k"), Usd::ZERO, "no dangling reservations");
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p gateway-spine --test budget_concurrency`
Expected: PASS — `successes == 10`, spent is exactly $1.00. (If this is ever flaky, the ledger's locking is wrong — that is a real bug, not a test problem.)

- [ ] **Step 3: Commit**

```bash
git add crates/gateway-spine/tests/budget_concurrency.rs
git commit -s -m "test(spine): prove no overspend under concurrent reservations"
```

---

### Task 10: `Clock` + `RateLimiter` (RPM/TPM/parallel)

**Files:**
- Create: `crates/gateway-spine/src/clock.rs`
- Create: `crates/gateway-spine/src/ratelimit.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test (clock)**

Create `crates/gateway-spine/src/clock.rs`:

```rust
//! Time as an injectable dependency so rate-limit/expiry logic is testable
//! without sleeping. Unix epoch millis.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// Test clock: starts at a fixed value, advances only when told to.
#[derive(Debug)]
pub struct MockClock {
    ms: AtomicI64,
}

impl MockClock {
    pub fn new(start_ms: i64) -> Self {
        Self { ms: AtomicI64::new(start_ms) }
    }
    pub fn advance(&self, by_ms: i64) {
        self.ms.fetch_add(by_ms, Ordering::SeqCst);
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> i64 {
        self.ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_clock_advances() {
        let c = MockClock::new(1000);
        assert_eq!(c.now_ms(), 1000);
        c.advance(500);
        assert_eq!(c.now_ms(), 1500);
    }

    #[test]
    fn system_clock_is_positive() {
        assert!(SystemClock.now_ms() > 0);
    }
}
```

- [ ] **Step 2: Write the failing test (rate limiter)**

Create `crates/gateway-spine/src/ratelimit.rs`:

```rust
//! Per-key fixed-window RPM/TPM plus a live parallel counter. Acquire checks all
//! configured dimensions atomically; `release_parallel` is called when a request
//! finishes. Window is one minute, reset lazily on first acquire of a new minute.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::clock::Clock;
use crate::error::{RateDimension, SpineError};
use crate::key::RateLimits;

const WINDOW_MS: i64 = 60_000;

#[derive(Debug, Default, Clone, Copy)]
struct Window {
    window_start_ms: i64,
    requests: i64,
    tokens: i64,
    parallel: i64,
}

pub struct RateLimiter<C: Clock> {
    clock: C,
    inner: Mutex<HashMap<String, Window>>,
}

impl<C: Clock> RateLimiter<C> {
    pub fn new(clock: C) -> Self {
        Self { clock, inner: Mutex::new(HashMap::new()) }
    }

    /// Acquire one request slot + `est_tokens`. Fail-closed on any breached
    /// dimension; on success the counters are incremented (caller must later
    /// call `release_parallel`).
    pub fn acquire(
        &self,
        key_id: &str,
        limits: &RateLimits,
        est_tokens: i64,
    ) -> Result<(), SpineError> {
        let now = self.clock.now_ms();
        let mut g = self.inner.lock().unwrap();
        let w = g.entry(key_id.to_string()).or_default();

        // Roll the window (parallel is NOT reset — it tracks live in-flight work).
        if now - w.window_start_ms >= WINDOW_MS {
            w.window_start_ms = now;
            w.requests = 0;
            w.tokens = 0;
        }

        if let Some(rpm) = limits.rpm {
            if w.requests + 1 > rpm {
                return Err(SpineError::RateLimited { key_id: key_id.into(), dimension: RateDimension::Requests });
            }
        }
        if let Some(tpm) = limits.tpm {
            if w.tokens + est_tokens > tpm {
                return Err(SpineError::RateLimited { key_id: key_id.into(), dimension: RateDimension::Tokens });
            }
        }
        if let Some(maxp) = limits.max_parallel {
            if w.parallel + 1 > maxp {
                return Err(SpineError::RateLimited { key_id: key_id.into(), dimension: RateDimension::Parallel });
            }
        }

        w.requests += 1;
        w.tokens += est_tokens;
        w.parallel += 1;
        Ok(())
    }

    pub fn release_parallel(&self, key_id: &str) {
        let mut g = self.inner.lock().unwrap();
        if let Some(w) = g.get_mut(key_id) {
            if w.parallel > 0 {
                w.parallel -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;

    fn limits(rpm: Option<i64>, tpm: Option<i64>, par: Option<i64>) -> RateLimits {
        RateLimits { rpm, tpm, max_parallel: par }
    }

    #[test]
    fn rpm_blocks_after_limit_then_resets_next_window() {
        let clock = MockClock::new(0);
        let rl = RateLimiter::new(clock);
        let lim = limits(Some(2), None, None);

        assert!(rl.acquire("k", &lim, 0).is_ok());
        rl.release_parallel("k");
        assert!(rl.acquire("k", &lim, 0).is_ok());
        rl.release_parallel("k");
        // third in the same minute → blocked
        assert!(matches!(
            rl.acquire("k", &lim, 0),
            Err(SpineError::RateLimited { dimension: RateDimension::Requests, .. })
        ));

        // next minute resets
        rl.clock.advance(60_000);
        assert!(rl.acquire("k", &lim, 0).is_ok());
    }

    #[test]
    fn tpm_counts_estimated_tokens() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = limits(None, Some(1000), None);
        assert!(rl.acquire("k", &lim, 700).is_ok());
        rl.release_parallel("k");
        // 700 + 400 > 1000 → blocked on tokens
        assert!(matches!(
            rl.acquire("k", &lim, 400),
            Err(SpineError::RateLimited { dimension: RateDimension::Tokens, .. })
        ));
    }

    #[test]
    fn parallel_limit_tracks_in_flight() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = limits(None, None, Some(1));
        assert!(rl.acquire("k", &lim, 0).is_ok());
        // second concurrent → blocked
        assert!(matches!(
            rl.acquire("k", &lim, 0),
            Err(SpineError::RateLimited { dimension: RateDimension::Parallel, .. })
        ));
        // first finishes → slot frees
        rl.release_parallel("k");
        assert!(rl.acquire("k", &lim, 0).is_ok());
    }

    #[test]
    fn no_limits_means_unlimited() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = RateLimits::default();
        for _ in 0..10_000 {
            assert!(rl.acquire("k", &lim, 1_000_000).is_ok());
            rl.release_parallel("k");
        }
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod clock;
pub mod ratelimit;

pub use clock::{Clock, MockClock, SystemClock};
pub use ratelimit::RateLimiter;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p gateway-spine clock:: ratelimit::`
Expected: 2 clock + 4 ratelimit = 6 tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/clock.rs crates/gateway-spine/src/ratelimit.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): injectable Clock + RPM/TPM/parallel RateLimiter"
```

---

### Task 11: `AuditSink` + in-memory audit log

**Files:**
- Create: `crates/gateway-spine/src/audit.rs`
- Modify: `crates/gateway-spine/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gateway-spine/src/audit.rs`:

```rust
//! Append-only audit trail. The spine records admin actions and request
//! outcomes through this seam; P1.7 swaps in a durable sink. The same stream
//! will carry MCP tool-call audit in P2 — one audit log across both planes.

use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub ts_ms: i64,
    /// Key id, or "admin" for control-plane actions.
    pub actor: String,
    /// e.g. "key.create", "request.complete", "request.denied".
    pub action: String,
    pub target: String,
    /// "ok" | "denied" | "error".
    pub outcome: String,
    pub detail: Option<String>,
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[derive(Default)]
pub struct MemoryAudit {
    events: Mutex<Vec<AuditEvent>>,
}

impl MemoryAudit {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().unwrap().clone()
    }
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for MemoryAudit {
    fn record(&self, event: AuditEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_in_order() {
        let a = MemoryAudit::new();
        assert!(a.is_empty());
        a.record(AuditEvent {
            ts_ms: 1,
            actor: "admin".into(),
            action: "key.create".into(),
            target: "key_1".into(),
            outcome: "ok".into(),
            detail: None,
        });
        a.record(AuditEvent {
            ts_ms: 2,
            actor: "key_1".into(),
            action: "request.denied".into(),
            target: "gpt-4o".into(),
            outcome: "denied".into(),
            detail: Some("budget exceeded".into()),
        });
        let ev = a.events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].action, "key.create");
        assert_eq!(ev[1].outcome, "denied");
    }

    #[test]
    fn works_through_trait_object() {
        let sink: Box<dyn AuditSink> = Box::new(MemoryAudit::new());
        sink.record(AuditEvent {
            ts_ms: 0,
            actor: "admin".into(),
            action: "boot".into(),
            target: "gateway".into(),
            outcome: "ok".into(),
            detail: None,
        });
        // Downcast isn't needed; just prove the trait call compiles + runs.
    }
}
```

Add to `crates/gateway-spine/src/lib.rs`:

```rust
pub mod audit;

pub use audit::{AuditEvent, AuditSink, MemoryAudit};
```

- [ ] **Step 2: Run test**

Run: `cargo test -p gateway-spine audit::`
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/audit.rs crates/gateway-spine/src/lib.rs
git commit -s -m "feat(spine): AuditSink + in-memory audit log"
```

---

### Task 12: Finalize `lib.rs` + crate-level integration test

**Files:**
- Modify: `crates/gateway-spine/src/lib.rs`
- Create: `crates/gateway-spine/tests/spine_smoke.rs`

- [ ] **Step 1: Remove the scaffold placeholder and confirm the module surface**

Ensure `crates/gateway-spine/src/lib.rs` reads exactly (the doc comment + forbid attribute, then all module declarations and re-exports, and NO `CRATE` placeholder):

```rust
//! # gateway-spine
//!
//! The protocol-agnostic core every request flows through — tokens in, dollars
//! out, policy everywhere. Owns the non-negotiable invariants: fail-closed
//! budgets, no double-billing, no overspend under concurrency, auth-by-default,
//! cost-correctness.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` and `docs/plans/`.

#![forbid(unsafe_code)]

pub mod audit;
pub mod budget;
pub mod clock;
pub mod error;
pub mod key;
pub mod money;
pub mod pricing;
pub mod ratelimit;
pub mod registry;
pub mod usage;

pub use audit::{AuditEvent, AuditSink, MemoryAudit};
pub use budget::{BudgetLedger, ReservationId};
pub use clock::{Clock, MockClock, SystemClock};
pub use error::{RateDimension, SpineError};
pub use key::{RateLimits, VirtualKey};
pub use money::Usd;
pub use pricing::ModelPrice;
pub use ratelimit::RateLimiter;
pub use registry::{ModelEntry, ModelRegistry};
pub use usage::TokenUsage;
```

- [ ] **Step 2: Write an integration test that exercises the whole admission path**

Create `crates/gateway-spine/tests/spine_smoke.rs`:

```rust
//! End-to-end (in-memory) admission path: a key is created, a request is
//! admitted (usable → allowlist → rate limit → budget reserve), the call's
//! actual cost is priced from the registry and committed, and an audit event is
//! recorded. This is the shape the HTTP lifecycle (P1.4) will wire to real I/O.

use gateway_spine::{
    AuditEvent, AuditSink, BudgetLedger, Clock, MemoryAudit, ModelEntry, ModelPrice, ModelRegistry,
    MockClock, RateLimiter, RateLimits, TokenUsage, Usd, VirtualKey,
};

fn gpt4o_entry() -> ModelEntry {
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

#[test]
fn full_admission_and_commit_path() {
    // Setup
    let mut registry = ModelRegistry::new();
    registry.insert(gpt4o_entry());
    let ledger = BudgetLedger::new();
    let clock = MockClock::new(1_000_000);
    let limiter = RateLimiter::new(clock);
    let audit = MemoryAudit::new();

    let key = VirtualKey {
        id: "key_1".into(),
        token_hash: VirtualKey::hash_secret("sk-test"),
        token_prefix: "sk-test".into(),
        max_budget: Some(Usd::from_dollars_f64(1.0)),
        limits: RateLimits { rpm: Some(60), tpm: Some(100_000), max_parallel: Some(4) },
        model_allowlist: Some(vec!["gpt-4o".into()]),
        expires_at: None,
        revoked: false,
        parent_id: None,
    };
    ledger.set_budget(&key.id, key.max_budget, Usd::ZERO);

    let model = "gpt-4o";
    let est_tokens = 1500;

    // 1. key usable
    key.ensure_usable(limiter_now(&limiter)).unwrap();
    // 2. model allowed
    assert!(key.allows_model(model));
    // 3. rate limit
    limiter.acquire(&key.id, &key.limits, est_tokens).unwrap();
    // 4. budget reserve (estimate $0.05)
    let res = ledger.reserve(&key.id, Usd::from_dollars_f64(0.05)).unwrap();

    // ... upstream call happens here in P1.4; we simulate the returned usage ...
    let usage = TokenUsage { input_tokens: 1000, output_tokens: 500, ..Default::default() };
    let actual = registry.cost(model, &usage).expect("known model has a price");
    assert_eq!(actual, Usd::from_micros(7_500)); // $0.0075

    // 5. commit actual, release parallel slot
    ledger.commit(res, actual).unwrap();
    limiter.release_parallel(&key.id);

    // 6. audit
    audit.record(AuditEvent {
        ts_ms: 1_000_000,
        actor: key.id.clone(),
        action: "request.complete".into(),
        target: model.into(),
        outcome: "ok".into(),
        detail: Some(format!("{} µUSD", actual.micros())),
    });

    // Assertions on final state
    assert_eq!(ledger.spent(&key.id), Usd::from_micros(7_500));
    assert_eq!(ledger.reserved(&key.id), Usd::ZERO);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit.events()[0].outcome, "ok");
}

// Small helper so the test reads top-to-bottom; the limiter owns the clock.
fn limiter_now(limiter: &RateLimiter<MockClock>) -> i64 {
    // RateLimiter doesn't expose the clock; use a parallel SystemClock-free value.
    // The key has no expiry, so any non-negative value works.
    let _ = limiter;
    1_000_000
}
```

- [ ] **Step 3: Run the whole crate's tests**

Run: `cargo test -p gateway-spine`
Expected: all unit tests + `budget_concurrency` + `spine_smoke` PASS.

- [ ] **Step 4: Full gate, then commit**

```bash
cargo fmt --all && cargo clippy -p gateway-spine --all-targets -- -D warnings
git add crates/gateway-spine/src/lib.rs crates/gateway-spine/tests/spine_smoke.rs
git commit -s -m "feat(spine): finalize module surface + admission-path smoke test"
```

---

## Milestone exit criteria

- [ ] `cargo test -p gateway-spine` is fully green (unit + `budget_concurrency` + `spine_smoke`).
- [ ] `cargo clippy -p gateway-spine --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
- [ ] The three invariants this milestone owns are each proven by a test: fail-closed (`reserve_is_fail_closed_at_budget`), no-overspend-under-concurrency (`never_overspends_under_concurrency`), cost-correctness (`unknown_model_returns_none_not_zero`).
- [ ] No floats anywhere in money math (grep `f64` in `gateway-spine/src` → only in `money.rs` display helpers and price-table test fixtures).

**Next:** `2026-06-10-p1-02-llm-types-and-egress.md` — the unified LLM request/response/stream types and the first provider transports, which consume `TokenUsage`/`Usd`/`ModelRegistry` from this milestone.
