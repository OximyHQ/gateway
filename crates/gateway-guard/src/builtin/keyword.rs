//! Keyword banlist guardrail.
//!
//! Blocks any text that contains one or more words from a configurable
//! case-insensitive banlist.

use async_trait::async_trait;

use crate::guardrail::Guardrail;
use crate::types::{GuardContext, GuardVerdict};

/// Guardrail that blocks text containing any banned keyword (case-insensitive).
///
/// Keywords are lowercased at construction time; the incoming text is also
/// lowercased before matching so comparisons are always case-insensitive.
#[derive(Debug, Clone)]
pub struct KeywordBanlistGuardrail {
    /// Lower-cased banned keywords.
    keywords: Vec<String>,
}

impl KeywordBanlistGuardrail {
    /// Create a guardrail from an iterator of keyword strings.
    pub fn new(keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            keywords: keywords
                .into_iter()
                .map(|k| k.into().to_lowercase())
                .collect(),
        }
    }
}

#[async_trait]
impl Guardrail for KeywordBanlistGuardrail {
    fn name(&self) -> &str {
        "keyword-banlist"
    }

    async fn check(&self, ctx: &GuardContext) -> GuardVerdict {
        let lower = ctx.text.to_lowercase();
        for kw in &self.keywords {
            if lower.contains(kw.as_str()) {
                return GuardVerdict::Block {
                    reason: format!("banned keyword detected: {kw}"),
                };
            }
        }
        GuardVerdict::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardStage;

    fn ctx(text: &str) -> GuardContext {
        GuardContext::new(GuardStage::PreRequest, text)
    }

    #[tokio::test]
    async fn blocks_on_exact_match() {
        let g = KeywordBanlistGuardrail::new(["forbidden"]);
        let verdict = g.check(&ctx("this word is forbidden here")).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { .. }),
            "expected Block"
        );
    }

    #[tokio::test]
    async fn blocks_case_insensitively() {
        let g = KeywordBanlistGuardrail::new(["badword"]);
        let verdict = g.check(&ctx("There is a BADWORD in this sentence")).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { .. }),
            "expected Block for case-insensitive match"
        );
    }

    #[tokio::test]
    async fn allows_text_without_banned_keywords() {
        let g = KeywordBanlistGuardrail::new(["danger", "forbidden"]);
        let verdict = g.check(&ctx("This is a perfectly safe message.")).await;
        assert_eq!(verdict, GuardVerdict::Allow);
    }

    #[tokio::test]
    async fn empty_banlist_always_allows() {
        let g = KeywordBanlistGuardrail::new(Vec::<String>::new());
        let verdict = g.check(&ctx("anything at all")).await;
        assert_eq!(verdict, GuardVerdict::Allow);
    }
}
