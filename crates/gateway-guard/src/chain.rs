//! Guard chain: ordered composition of guardrails with per-entry enforcement modes.
//!
//! # Composition semantics
//!
//! | Mode          | Block         | Mask                              | Flag        | Allow       |
//! |---------------|---------------|-----------------------------------|-------------|-------------|
//! | `Enforce`     | short-circuit | apply redaction, continue         | record only | continue    |
//! | `ObserveOnly` | record, skip  | record, skip (no text mutation)   | record only | continue    |
//! | `DryRun`      | record, skip  | record, skip (no text mutation)   | record only | continue    |
//!
//! Final verdict rules (in priority order):
//! 1. First `Block` from an `Enforce`-mode guardrail wins.
//! 2. If no block: first `Mask` from `Enforce` wins; the redacted text is the
//!    composition of all applied masks.
//! 3. If there were any `Flag` verdicts: return `Flag` with concatenated reasons.
//! 4. Otherwise: `Allow`.

use std::sync::Arc;

use crate::guardrail::Guardrail;
use crate::types::{EnforcementMode, GuardContext, GuardVerdict};

/// A single entry in a [`GuardChain`]: a guardrail paired with its enforcement mode.
pub struct GuardEntry {
    pub mode: EnforcementMode,
    pub guardrail: Arc<dyn Guardrail>,
}

impl std::fmt::Debug for GuardEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardEntry")
            .field("mode", &self.mode)
            .field("guardrail", &self.guardrail.name())
            .finish()
    }
}

/// The full result of running a chain.
#[derive(Debug, Clone)]
pub struct ChainVerdict {
    /// The effective verdict after applying all enforcement rules.
    pub final_verdict: GuardVerdict,
    /// Per-guardrail breakdown: `(name, mode, verdict)`.
    pub per_guardrail: Vec<(String, EnforcementMode, GuardVerdict)>,
}

/// An ordered sequence of [`GuardEntry`] items evaluated left-to-right.
#[derive(Debug, Default)]
pub struct GuardChain {
    entries: Vec<GuardEntry>,
}

impl GuardChain {
    /// Create an empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a guardrail with the given enforcement mode.
    pub fn push(mut self, mode: EnforcementMode, guardrail: Arc<dyn Guardrail>) -> Self {
        self.entries.push(GuardEntry { mode, guardrail });
        self
    }

    /// Run the chain against `ctx`.
    ///
    /// - `Enforce` mode: `Block` short-circuits; `Mask` composes.
    /// - `ObserveOnly` / `DryRun`: all guardrails run; nothing is enforced.
    pub async fn run(&self, ctx: &GuardContext) -> ChainVerdict {
        self.run_inner(ctx, false).await
    }

    /// Simulate the chain — equivalent to running every entry in `DryRun` mode.
    ///
    /// No enforcement is applied; the returned `per_guardrail` shows what
    /// *would* have happened in `Enforce` mode.
    pub async fn simulate(&self, ctx: &GuardContext) -> ChainVerdict {
        self.run_inner(ctx, true).await
    }

    async fn run_inner(&self, ctx: &GuardContext, force_dry_run: bool) -> ChainVerdict {
        let mut per_guardrail: Vec<(String, EnforcementMode, GuardVerdict)> = Vec::new();

        // Working copy of the text (mutated by Mask in Enforce mode).
        let mut current_text = ctx.text.clone();
        // Track whether we have hit an enforced Block.
        let mut block_verdict: Option<GuardVerdict> = None;
        // Composed redacted text from all Enforce masks.
        let mut masked_text: Option<String> = None;
        // Collected Flag reasons.
        let mut flag_reasons: Vec<String> = Vec::new();

        for entry in &self.entries {
            let effective_mode = if force_dry_run {
                EnforcementMode::DryRun
            } else {
                entry.mode
            };

            // If we already have an enforced block, remaining guardrails in Enforce
            // mode are skipped (we still run ObserveOnly/DryRun ones — but since
            // we're already blocked we skip everything for simplicity consistent
            // with the spec: "short-circuits (remaining guardrails skipped)").
            if block_verdict.is_some() && effective_mode == EnforcementMode::Enforce {
                // Record as skipped — use Allow as a placeholder for skipped.
                per_guardrail.push((
                    entry.guardrail.name().to_owned(),
                    effective_mode,
                    GuardVerdict::Allow,
                ));
                continue;
            }

            // Build context with the (possibly mutated) text.
            let eval_ctx = GuardContext {
                text: current_text.clone(),
                ..ctx.clone()
            };

            let verdict = entry.guardrail.check(&eval_ctx).await;

            match (&verdict, effective_mode) {
                (GuardVerdict::Block { .. }, EnforcementMode::Enforce) => {
                    if block_verdict.is_none() {
                        block_verdict = Some(verdict.clone());
                    }
                }
                (GuardVerdict::Mask { redacted_text }, EnforcementMode::Enforce) => {
                    // Apply mask: update current text for subsequent guardrails.
                    let new_text = redacted_text.clone();
                    current_text = new_text.clone();
                    masked_text = Some(new_text);
                }
                (GuardVerdict::Flag { reason }, _) => {
                    flag_reasons.push(reason.clone());
                }
                // ObserveOnly / DryRun: record but do not act.
                _ => {}
            }

            per_guardrail.push((entry.guardrail.name().to_owned(), effective_mode, verdict));
        }

        // Determine final verdict.
        let final_verdict = if let Some(block) = block_verdict {
            block
        } else if let Some(redacted) = masked_text {
            GuardVerdict::Mask {
                redacted_text: redacted,
            }
        } else if !flag_reasons.is_empty() {
            GuardVerdict::Flag {
                reason: flag_reasons.join("; "),
            }
        } else {
            GuardVerdict::Allow
        };

        ChainVerdict {
            final_verdict,
            per_guardrail,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::builtin::{KeywordBanlistGuardrail, PiiGuardrail, SecretsGuardrail};
    use crate::types::GuardStage;

    fn ctx(text: &str) -> GuardContext {
        GuardContext::new(GuardStage::PreRequest, text)
    }

    // A guardrail that always returns Block.
    struct AlwaysBlock;
    #[async_trait::async_trait]
    impl Guardrail for AlwaysBlock {
        fn name(&self) -> &str {
            "always-block"
        }
        async fn check(&self, _ctx: &GuardContext) -> GuardVerdict {
            GuardVerdict::Block {
                reason: "always blocked".into(),
            }
        }
    }

    // A guardrail that always returns Flag.
    struct AlwaysFlag;
    #[async_trait::async_trait]
    impl Guardrail for AlwaysFlag {
        fn name(&self) -> &str {
            "always-flag"
        }
        async fn check(&self, _ctx: &GuardContext) -> GuardVerdict {
            GuardVerdict::Flag {
                reason: "flagged annotation".into(),
            }
        }
    }

    // A guardrail that records the text it saw.
    #[derive(Default)]
    struct TextRecorder {
        seen: std::sync::Mutex<Option<String>>,
    }
    #[async_trait::async_trait]
    impl Guardrail for TextRecorder {
        fn name(&self) -> &str {
            "recorder"
        }
        async fn check(&self, ctx: &GuardContext) -> GuardVerdict {
            *self.seen.lock().unwrap() = Some(ctx.text.clone());
            GuardVerdict::Allow
        }
    }
    impl TextRecorder {
        fn last_seen(&self) -> Option<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn observe_only_never_blocks() {
        let chain = GuardChain::new().push(EnforcementMode::ObserveOnly, Arc::new(AlwaysBlock));
        let result = chain.run(&ctx("anything")).await;
        assert_eq!(result.final_verdict, GuardVerdict::Allow);
        // The per-guardrail record should still show the Block verdict.
        assert!(matches!(
            result.per_guardrail[0].2,
            GuardVerdict::Block { .. }
        ));
    }

    #[tokio::test]
    async fn dry_run_reports_would_be_block() {
        let chain = GuardChain::new().push(EnforcementMode::DryRun, Arc::new(AlwaysBlock));
        let result = chain.run(&ctx("anything")).await;
        // DryRun should not enforce — final is Allow.
        assert_eq!(result.final_verdict, GuardVerdict::Allow);
        assert!(matches!(
            result.per_guardrail[0].2,
            GuardVerdict::Block { .. }
        ));
    }

    #[tokio::test]
    async fn enforce_short_circuits_on_block() {
        let recorder = Arc::new(TextRecorder::default());
        let chain = GuardChain::new()
            .push(EnforcementMode::Enforce, Arc::new(AlwaysBlock))
            .push(EnforcementMode::Enforce, recorder.clone());
        let result = chain.run(&ctx("test")).await;
        assert!(matches!(result.final_verdict, GuardVerdict::Block { .. }));
        // The recorder should not have been called (short-circuit).
        assert!(
            recorder.last_seen().is_none(),
            "recorder should be skipped after block"
        );
    }

    #[tokio::test]
    async fn simulate_returns_all_would_be_outcomes() {
        let chain = GuardChain::new()
            .push(EnforcementMode::Enforce, Arc::new(AlwaysBlock))
            .push(
                EnforcementMode::Enforce,
                Arc::new(KeywordBanlistGuardrail::new(["safe"])),
            );
        // simulate should run all guardrails.
        let result = chain.simulate(&ctx("safe text")).await;
        assert_eq!(result.per_guardrail.len(), 2);
        // Final verdict in simulate is DryRun — no enforcement.
        assert_eq!(result.final_verdict, GuardVerdict::Allow);
    }

    #[tokio::test]
    async fn mask_in_enforce_applies_redacted_text_for_subsequent() {
        let recorder = Arc::new(TextRecorder::default());
        let chain = GuardChain::new()
            .push(EnforcementMode::Enforce, Arc::new(PiiGuardrail::new()))
            .push(EnforcementMode::Enforce, recorder.clone());
        let email_text = "Contact alice@example.com for details";
        let result = chain.run(&ctx(email_text)).await;
        // PII should have masked.
        assert!(matches!(result.final_verdict, GuardVerdict::Mask { .. }));
        // The recorder should see the redacted text, not the original.
        let seen = recorder.last_seen().unwrap();
        assert!(
            !seen.contains("alice@example.com"),
            "subsequent guardrail should see redacted text, got: {seen}"
        );
        assert!(seen.contains("[EMAIL]"));
    }

    #[tokio::test]
    async fn flag_verdicts_collected_without_blocking() {
        let chain = GuardChain::new()
            .push(EnforcementMode::Enforce, Arc::new(AlwaysFlag))
            .push(EnforcementMode::Enforce, Arc::new(AlwaysFlag));
        let result = chain.run(&ctx("text")).await;
        assert!(
            matches!(result.final_verdict, GuardVerdict::Flag { .. }),
            "expected Flag as final verdict"
        );
        // Both flags should be in per_guardrail.
        assert_eq!(result.per_guardrail.len(), 2);
    }

    #[tokio::test]
    async fn clean_text_is_allowed() {
        let chain = GuardChain::new()
            .push(EnforcementMode::Enforce, Arc::new(SecretsGuardrail::new()))
            .push(
                EnforcementMode::Enforce,
                Arc::new(KeywordBanlistGuardrail::new(["banned"])),
            );
        let result = chain.run(&ctx("Totally clean text here.")).await;
        assert_eq!(result.final_verdict, GuardVerdict::Allow);
    }
}
