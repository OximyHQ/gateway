//! Build a `ModelRegistry` from two JSON files:
//!   1. a `models.dev`-shaped catalog (the bulk of the 1000+ models), and
//!   2. a local overrides file (operator edits: price fixes, custom/self-hosted
//!      models, capability tweaks).
//!
//! Overrides MERGE over the catalog by model id: a model present in both takes the
//! override's fields; a model only in overrides is added. Prices in the source
//! JSON are expressed in DOLLARS-per-million-tokens (the models.dev convention)
//! and converted here to the spine's i64 µUSD-per-million-tokens — the one place
//! that f64→integer conversion happens, and it rounds half-up, never truncates.

use std::path::Path;

use gateway_spine::{ModelEntry, ModelPrice, ModelRegistry};

use crate::error::CacheError;

/// The models.dev-shaped row we parse. Extra fields are ignored (forward-compatible).
#[derive(Debug, Clone, serde::Deserialize)]
struct SourceModel {
    id: String,
    #[serde(default)]
    provider: String,
    /// Dollars per million input tokens.
    #[serde(default)]
    input: f64,
    /// Dollars per million output tokens.
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
    #[serde(default)]
    context_window: Option<i64>,
    #[serde(default)]
    max_output_tokens: Option<i64>,
    #[serde(default)]
    supports_tools: bool,
    #[serde(default)]
    supports_vision: bool,
    #[serde(default = "default_true")]
    supports_streaming: bool,
}

fn default_true() -> bool {
    true
}

/// Convert dollars-per-mtok (f64) to µUSD-per-mtok (i64), rounding half-up.
fn dollars_per_mtok_to_micros(d: f64) -> i64 {
    (d * 1_000_000.0).round() as i64
}

impl SourceModel {
    fn into_entry(self) -> ModelEntry {
        ModelEntry {
            id: self.id,
            provider: self.provider,
            price: ModelPrice {
                input_per_mtok: dollars_per_mtok_to_micros(self.input),
                output_per_mtok: dollars_per_mtok_to_micros(self.output),
                cache_read_per_mtok: dollars_per_mtok_to_micros(self.cache_read),
                cache_write_per_mtok: dollars_per_mtok_to_micros(self.cache_write),
            },
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            supports_tools: self.supports_tools,
            supports_vision: self.supports_vision,
            supports_streaming: self.supports_streaming,
        }
    }
}

fn parse_models(json: &str) -> Result<Vec<SourceModel>, CacheError> {
    serde_json::from_str::<Vec<SourceModel>>(json).map_err(CacheError::from)
}

/// Build a registry from catalog JSON + optional overrides JSON. Overrides win by id.
pub fn build_registry(
    catalog_json: &str,
    overrides_json: Option<&str>,
) -> Result<ModelRegistry, CacheError> {
    let mut registry = ModelRegistry::new();
    for m in parse_models(catalog_json)? {
        registry.insert(m.into_entry());
    }
    if let Some(ov) = overrides_json {
        for m in parse_models(ov)? {
            registry.insert(m.into_entry()); // insert replaces by id → override wins
        }
    }
    Ok(registry)
}

/// Build a registry by reading the two files off disk. A missing overrides path is
/// fine (treated as "no overrides"); a missing catalog path is an error.
pub fn build_registry_from_paths(
    catalog_path: &Path,
    overrides_path: Option<&Path>,
) -> Result<ModelRegistry, CacheError> {
    let catalog = std::fs::read_to_string(catalog_path).map_err(|e| {
        CacheError::RegistrySource(format!("catalog {}: {e}", catalog_path.display()))
    })?;
    let overrides =
        match overrides_path {
            Some(p) if p.exists() => Some(std::fs::read_to_string(p).map_err(|e| {
                CacheError::RegistrySource(format!("overrides {}: {e}", p.display()))
            })?),
            _ => None,
        };
    build_registry(&catalog, overrides.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::{TokenUsage, Usd};

    const CATALOG: &str = r#"[
        {"id":"gpt-4o","provider":"openai","input":2.5,"output":10.0,"cache_read":1.25,
         "context_window":128000,"max_output_tokens":16384,"supports_tools":true,"supports_vision":true},
        {"id":"claude-3-5-sonnet","provider":"anthropic","input":3.0,"output":15.0,
         "context_window":200000,"supports_tools":true}
    ]"#;

    #[test]
    fn parses_catalog_and_converts_prices() {
        let r = build_registry(CATALOG, None).unwrap();
        assert_eq!(r.len(), 2);
        let e = r.get("gpt-4o").unwrap();
        // $2.50/M → 2_500_000 µUSD/M
        assert_eq!(e.price.input_per_mtok, 2_500_000);
        assert_eq!(e.price.output_per_mtok, 10_000_000);
        assert_eq!(e.price.cache_read_per_mtok, 1_250_000);
        assert_eq!(e.context_window, Some(128_000));
        assert!(e.supports_streaming); // defaulted true
    }

    #[test]
    fn cost_matches_spine_after_conversion() {
        let r = build_registry(CATALOG, None).unwrap();
        let u = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        // 1000 × $2.50/M + 500 × $10/M = $0.0025 + $0.005 = $0.0075
        assert_eq!(r.cost("gpt-4o", &u), Some(Usd::from_micros(7_500)));
    }

    #[test]
    fn overrides_win_by_id_and_add_new() {
        let overrides = r#"[
            {"id":"gpt-4o","provider":"openai","input":2.0,"output":8.0},
            {"id":"my-local-llm","provider":"ollama","input":0.0,"output":0.0,"supports_tools":false}
        ]"#;
        let r = build_registry(CATALOG, Some(overrides)).unwrap();
        assert_eq!(r.len(), 3); // gpt-4o overridden, claude kept, local added
        assert_eq!(r.get("gpt-4o").unwrap().price.input_per_mtok, 2_000_000); // override price
        assert!(r.get("my-local-llm").is_some());
    }

    #[test]
    fn sub_cent_prices_round_half_up() {
        // $0.075/M → 75_000 µUSD/M
        let json = r#"[{"id":"cheap","provider":"x","input":0.075,"output":0.0}]"#;
        let r = build_registry(json, None).unwrap();
        assert_eq!(r.get("cheap").unwrap().price.input_per_mtok, 75_000);
    }

    #[test]
    fn malformed_json_errors() {
        assert!(build_registry("{not an array", None).is_err());
    }
}
