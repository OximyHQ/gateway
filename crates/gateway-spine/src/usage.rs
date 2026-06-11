//! Token counts for one LLM call, in non-overlapping categories so cost is a
//! simple dot-product with prices. Providers that report overlapping buckets
//! (e.g. prompt_tokens that include cached) are normalized into these at the
//! translation layer (P1.3), never here.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
