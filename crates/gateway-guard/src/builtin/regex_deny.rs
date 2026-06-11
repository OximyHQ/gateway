//! Regex denylist guardrail.
//!
//! Blocks text that matches any of a configurable set of regular expressions.
//! Patterns are compiled once at construction time for efficiency.

use async_trait::async_trait;
use regex::Regex;

use crate::guardrail::Guardrail;
use crate::types::{GuardContext, GuardError, GuardVerdict};

/// Guardrail that blocks text matching any regex in a denylist.
///
/// Patterns are compiled at construction time; pass invalid regex and
/// [`RegexDenylistGuardrail::new`] returns a [`GuardError::RegexCompile`].
#[derive(Debug)]
pub struct RegexDenylistGuardrail {
    patterns: Vec<CompiledPattern>,
}

struct CompiledPattern {
    /// Original pattern string (for error messages and debug output).
    source: String,
    re: Regex,
}

impl std::fmt::Debug for CompiledPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledPattern")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl RegexDenylistGuardrail {
    /// Compile all provided patterns.
    ///
    /// Returns `Err(GuardError::RegexCompile)` if any pattern is invalid.
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Result<Self, GuardError> {
        let compiled = patterns
            .into_iter()
            .map(|p| {
                let source = p.into();
                Regex::new(&source)
                    .map(|re| CompiledPattern {
                        source: source.clone(),
                        re,
                    })
                    .map_err(|e| GuardError::RegexCompile(format!("{source}: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { patterns: compiled })
    }
}

#[async_trait]
impl Guardrail for RegexDenylistGuardrail {
    fn name(&self) -> &str {
        "regex-denylist"
    }

    async fn check(&self, ctx: &GuardContext) -> GuardVerdict {
        for p in &self.patterns {
            if p.re.is_match(&ctx.text) {
                return GuardVerdict::Block {
                    reason: format!("text matched deny pattern: {}", p.source),
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
    async fn blocks_on_regex_match() {
        let g = RegexDenylistGuardrail::new([r"(?i)drop\s+table"]).unwrap();
        let verdict = g.check(&ctx("Please DROP TABLE users")).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { .. }),
            "expected Block on DROP TABLE, got {verdict:?}"
        );
    }

    #[tokio::test]
    async fn allows_non_matching_text() {
        let g = RegexDenylistGuardrail::new([r"(?i)drop\s+table"]).unwrap();
        let verdict = g.check(&ctx("SELECT * FROM users")).await;
        assert_eq!(verdict, GuardVerdict::Allow);
    }

    #[test]
    fn invalid_regex_returns_error() {
        let result = RegexDenylistGuardrail::new(["[invalid"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("regex compilation failed"));
    }

    #[tokio::test]
    async fn multiple_patterns_first_match_blocks() {
        let g = RegexDenylistGuardrail::new([r"\bfoo\b", r"\bbar\b"]).unwrap();
        let verdict = g.check(&ctx("something bar something")).await;
        assert!(matches!(verdict, GuardVerdict::Block { .. }));
    }
}
