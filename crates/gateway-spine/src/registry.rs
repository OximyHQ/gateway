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
        let u = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        assert_eq!(r.cost("gpt-4o", &u), Some(Usd::from_micros(7_500)));
    }

    #[test]
    fn unknown_model_returns_none_not_zero() {
        let r = ModelRegistry::new();
        let u = TokenUsage {
            input_tokens: 1000,
            ..Default::default()
        };
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
