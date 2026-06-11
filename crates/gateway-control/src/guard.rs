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
