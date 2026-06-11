//! Build a `ModelRegistry` from JSON source files.
//!
//! Two input formats are supported:
//!
//! 1. **Flat array** (`models.json` format) — the legacy bundled catalog and the
//!    user overrides file.  Each element is a `SourceModel` with a flat set of
//!    fields (`id`, `provider`, `input`, `output`, …).
//!
//! 2. **models.dev API snapshot** — the nested map format emitted by the live
//!    `models.dev` catalog:
//!    ```json
//!    { "<providerId>": { "id": "...", "name": "...", "models": {
//!        "<modelId>": { "id": "...", "name": "...",
//!          "cost": { "input": f64, "output": f64, ... },
//!          "limit": { "context": i64?, "output": i64? },
//!          "modalities": { "input": [str], "output": [str] },
//!          "tool_call": bool?, "reasoning": bool? } } } }
//!    ```
//!    Each model is registered under a **namespaced id** `{provider_id}/{model_id}`
//!    to guarantee uniqueness across the 5000+ model catalog. For the `openrouter`
//!    provider, the raw model id (e.g. `openai/gpt-4o-mini`) is *also* registered
//!    as an alias so existing routing still resolves.
//!
//! Overrides MERGE over the catalog by model id: a model present in both takes the
//! override's fields; a model only in overrides is added. Prices in the source
//! JSON are expressed in DOLLARS-per-million-tokens (the models.dev convention)
//! and converted here to the spine's i64 µUSD-per-million-tokens — the one place
//! that f64→integer conversion happens, and it rounds half-up, never truncates.

use std::collections::HashMap;
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

// ── models.dev nested-format parser ──────────────────────────────────────────

/// Deserialization types for the models.dev API snapshot format.
/// All fields use `default` so forward-compatible unknowns are ignored.
#[derive(Debug, serde::Deserialize)]
struct DevCost {
    #[serde(default)]
    input: f64,
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
}

#[derive(Debug, serde::Deserialize)]
struct DevLimit {
    #[serde(default)]
    context: Option<i64>,
    #[serde(default)]
    output: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct DevModalities {
    #[serde(default)]
    input: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct DevModel {
    id: String,
    #[serde(default)]
    cost: Option<DevCost>,
    #[serde(default)]
    limit: Option<DevLimit>,
    #[serde(default)]
    modalities: Option<DevModalities>,
    #[serde(default)]
    tool_call: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct DevProvider {
    #[serde(default)]
    models: HashMap<String, DevModel>,
}

/// Holds the two-pass output of the models.dev parser.
struct ParsedModelsDev {
    /// Namespaced primary entries: `{provider_id}/{model_id}` for every model in
    /// every provider. Always unique — no two providers produce the same key.
    primary: Vec<ModelEntry>,
    /// OpenRouter bare-id aliases: the raw `model.id` (e.g. `openai/gpt-4o-mini`)
    /// with `provider = "openrouter"`. Inserted AFTER primaries so they override
    /// any proxy/gateway provider that happens to use the same model id.
    openrouter_aliases: Vec<ModelEntry>,
}

/// Parse a models.dev API snapshot (nested provider → models map).
///
/// Namespacing: every model is registered under `{provider_id}/{model_id}` so
/// IDs are globally unique across all 5000+ catalog models. Additionally, for
/// the `openrouter` provider, the raw `model.id` (e.g. `openai/gpt-4o-mini`) is
/// collected as an alias. Aliases are inserted **after** primary entries in
/// `build_registry_from_models_dev` so they win over any proxy provider that
/// coincidentally uses the same model id — ensuring `openai/gpt-4o-mini` always
/// routes via OpenRouter when an `OPENROUTER_API_KEY` is present.
fn parse_models_dev(json: &str) -> Result<ParsedModelsDev, CacheError> {
    let catalog: HashMap<String, DevProvider> =
        serde_json::from_str(json).map_err(CacheError::from)?;

    let mut primary: Vec<ModelEntry> = Vec::with_capacity(catalog.len() * 8);
    let mut openrouter_aliases: Vec<ModelEntry> = Vec::new();

    for (provider_id, provider) in &catalog {
        for model in provider.models.values() {
            let cost = model.cost.as_ref();
            let limit = model.limit.as_ref();
            let supports_vision = model
                .modalities
                .as_ref()
                .map(|m| m.input.iter().any(|s| s == "image"))
                .unwrap_or(false);

            let price = ModelPrice {
                input_per_mtok: dollars_per_mtok_to_micros(cost.map(|c| c.input).unwrap_or(0.0)),
                output_per_mtok: dollars_per_mtok_to_micros(cost.map(|c| c.output).unwrap_or(0.0)),
                cache_read_per_mtok: dollars_per_mtok_to_micros(
                    cost.map(|c| c.cache_read).unwrap_or(0.0),
                ),
                cache_write_per_mtok: dollars_per_mtok_to_micros(
                    cost.map(|c| c.cache_write).unwrap_or(0.0),
                ),
            };

            let entry = ModelEntry {
                id: format!("{provider_id}/{}", model.id),
                provider: provider_id.clone(),
                price,
                context_window: limit.and_then(|l| l.context),
                max_output_tokens: limit.and_then(|l| l.output),
                supports_tools: model.tool_call.unwrap_or(false),
                supports_vision,
                supports_streaming: true,
            };
            primary.push(entry);

            // Collect openrouter aliases separately — inserted last so they
            // reliably win over same-id entries from proxy/aggregator providers.
            if provider_id == "openrouter" {
                openrouter_aliases.push(ModelEntry {
                    id: model.id.clone(),
                    provider: "openrouter".into(),
                    price,
                    context_window: limit.and_then(|l| l.context),
                    max_output_tokens: limit.and_then(|l| l.output),
                    supports_tools: model.tool_call.unwrap_or(false),
                    supports_vision,
                    supports_streaming: true,
                });
            }
        }
    }

    Ok(ParsedModelsDev {
        primary,
        openrouter_aliases,
    })
}

/// Build a `ModelRegistry` from a models.dev API snapshot JSON string.
///
/// Optionally applies a flat-array overrides JSON on top (same format as the
/// legacy `models.json` catalog).  Overrides win by id.
///
/// Insertion order:
/// 1. All primary namespaced entries (`{provider}/{model}`) — unique, no collision.
/// 2. OpenRouter bare aliases (`openai/gpt-4o-mini` → provider=openrouter) — last
///    so they override same-id proxy entries from other aggregators.
/// 3. User overrides — win by id over everything.
pub fn build_registry_from_models_dev(
    models_dev_json: &str,
    overrides_json: Option<&str>,
) -> Result<ModelRegistry, CacheError> {
    let mut registry = ModelRegistry::new();

    let parsed = parse_models_dev(models_dev_json)?;
    for entry in parsed.primary {
        registry.insert(entry);
    }
    // Insert openrouter aliases after all primaries so they win over proxy providers.
    for entry in parsed.openrouter_aliases {
        registry.insert(entry);
    }

    if let Some(ov) = overrides_json {
        for m in parse_models(ov)? {
            registry.insert(m.into_entry());
        }
    }

    Ok(registry)
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

    // ── models.dev format tests ───────────────────────────────────────────────

    const MODELS_DEV_FIXTURE: &str = r#"{
        "openai": {
            "id": "openai",
            "name": "OpenAI",
            "models": {
                "gpt-4o": {
                    "id": "gpt-4o",
                    "name": "GPT-4o",
                    "tool_call": true,
                    "modalities": { "input": ["text", "image"], "output": ["text"] },
                    "limit": { "context": 128000, "output": 16384 },
                    "cost": { "input": 2.5, "output": 10.0, "cache_read": 1.25, "cache_write": 0.0 }
                }
            }
        },
        "anthropic": {
            "id": "anthropic",
            "name": "Anthropic",
            "models": {
                "claude-3-5-sonnet-20241022": {
                    "id": "claude-3-5-sonnet-20241022",
                    "name": "Claude 3.5 Sonnet",
                    "tool_call": true,
                    "modalities": { "input": ["text"], "output": ["text"] },
                    "limit": { "context": 200000, "output": 8192 },
                    "cost": { "input": 3.0, "output": 15.0, "cache_read": 0.30, "cache_write": 3.75 }
                }
            }
        },
        "openrouter": {
            "id": "openrouter",
            "name": "OpenRouter",
            "models": {
                "openai/gpt-4o-mini": {
                    "id": "openai/gpt-4o-mini",
                    "name": "GPT-4o Mini",
                    "tool_call": true,
                    "modalities": { "input": ["text", "image"], "output": ["text"] },
                    "limit": { "context": 128000, "output": 16384 },
                    "cost": { "input": 0.15, "output": 0.6, "cache_read": 0.075, "cache_write": 0.0 }
                }
            }
        }
    }"#;

    #[test]
    fn models_dev_parses_and_counts() {
        // openai/gpt-4o, anthropic/claude-3-5-sonnet, openrouter/openai/gpt-4o-mini,
        // PLUS openrouter alias "openai/gpt-4o-mini" = 4 entries
        let r = build_registry_from_models_dev(MODELS_DEV_FIXTURE, None).unwrap();
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn models_dev_namespaced_id() {
        let r = build_registry_from_models_dev(MODELS_DEV_FIXTURE, None).unwrap();
        // Primary namespaced entry
        assert!(r.get("openai/gpt-4o").is_some());
        assert!(r.get("anthropic/claude-3-5-sonnet-20241022").is_some());
        assert!(r.get("openrouter/openai/gpt-4o-mini").is_some());
    }

    #[test]
    fn models_dev_price_conversion() {
        let r = build_registry_from_models_dev(MODELS_DEV_FIXTURE, None).unwrap();
        let e = r.get("openai/gpt-4o").unwrap();
        // $2.50/M → 2_500_000 µUSD/M
        assert_eq!(e.price.input_per_mtok, 2_500_000);
        assert_eq!(e.price.output_per_mtok, 10_000_000);
        assert_eq!(e.price.cache_read_per_mtok, 1_250_000);
        assert_eq!(e.context_window, Some(128_000));
        assert_eq!(e.max_output_tokens, Some(16_384));
        assert!(e.supports_tools);
        assert!(e.supports_vision); // image in modalities.input
        assert!(e.supports_streaming);
    }

    #[test]
    fn models_dev_openrouter_alias_resolves() {
        let r = build_registry_from_models_dev(MODELS_DEV_FIXTURE, None).unwrap();
        // Bare alias must exist and point to openrouter provider
        let alias = r.get("openai/gpt-4o-mini").unwrap();
        assert_eq!(alias.provider, "openrouter");
        // $0.15/M → 150_000 µUSD/M
        assert_eq!(alias.price.input_per_mtok, 150_000);
    }

    #[test]
    fn models_dev_no_cost_defaults_to_zero() {
        let json = r#"{
            "test": { "id": "test", "name": "Test",
                "models": { "nocost": { "id": "nocost", "name": "No Cost",
                    "modalities": { "input": ["text"], "output": ["text"] },
                    "limit": { "context": 8192, "output": 2048 } } } }
        }"#;
        let r = build_registry_from_models_dev(json, None).unwrap();
        let e = r.get("test/nocost").unwrap();
        assert_eq!(e.price.input_per_mtok, 0);
        assert_eq!(e.price.output_per_mtok, 0);
    }

    #[test]
    fn models_dev_overrides_win() {
        let overrides = r#"[{"id":"openai/gpt-4o","provider":"openai","input":1.0,"output":5.0}]"#;
        let r = build_registry_from_models_dev(MODELS_DEV_FIXTURE, Some(overrides)).unwrap();
        // Override replaces namespaced entry
        assert_eq!(
            r.get("openai/gpt-4o").unwrap().price.input_per_mtok,
            1_000_000
        );
    }

    #[test]
    fn models_dev_malformed_errors() {
        assert!(build_registry_from_models_dev("not json", None).is_err());
    }
}
