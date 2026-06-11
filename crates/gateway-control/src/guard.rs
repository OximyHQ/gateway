//! The guardrail seam. The live lifecycle now runs a real
//! [`gateway_guard::GuardChain`] (held in [`crate::AppState::guard`]) at the
//! `PreRequest` and `PostResponse` stages around provider egress. This module
//! provides the chain builders the binary + tests wire in.
//!
//! The legacy no-op `AllowAll`/`GuardHook`/`GuardVerdict` types are retained for
//! backward compatibility of the public surface but are no longer on the hot
//! path — the chain is the source of truth.

use std::sync::Arc;

use gateway_guard::builtin::{PiiGuardrail, SecretsGuardrail};
use gateway_guard::{EnforcementMode, GuardChain};

use gateway_llm::ChatRequest;

/// The production default guard chain.
///
/// - **Secrets → Block (Enforce):** a recognised provider API key (OpenAI/AWS/
///   GitHub/Slack/GitLab shape) in the prompt or completion is a hard block.
///   Secrets are enforced (not merely observed) because forwarding a live key to
///   an upstream LLM is an exfiltration event we must stop before egress.
/// - **PII → Mask (Enforce):** emails/phones/cards/SSNs are redacted in-place so
///   the request still completes with `[EMAIL]`/`[PHONE]`/… placeholders rather
///   than failing — masking, not blocking, keeps the gateway useful by default.
///
/// Order matters: secrets are checked first so a secret-laden prompt blocks
/// before any masking work.
pub fn default_chain() -> GuardChain {
    GuardChain::new()
        .push(EnforcementMode::Enforce, Arc::new(SecretsGuardrail::new()))
        .push(EnforcementMode::Enforce, Arc::new(PiiGuardrail::new()))
}

/// An empty chain — runs no guardrails (the old `AllowAll` behaviour). Used by
/// tests and call sites that opt out of content guarding.
pub fn empty_chain() -> GuardChain {
    GuardChain::new()
}

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
