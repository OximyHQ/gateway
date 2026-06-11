//! Load pipeline: read → interpolate `${ENV}` → parse → schema-validate →
//! semantic-validate. `validate` runs the whole pipeline WITHOUT applying (the
//! `--dry-run` path). Referential checks the schema can't express live in
//! `validate_semantics` (a route's provider must exist; key ids unique).

use std::collections::HashSet;

use serde_json::Value;

use crate::error::ConfigError;
use crate::interpolate::interpolate;
use crate::model::Config;
use crate::schema::validate_structure;

/// Parse + validate a config string (already env-interpolated). Returns the typed
/// `Config` on success — this is the `validate` / `--dry-run` entry point.
pub fn validate(raw_json: &str) -> Result<Config, ConfigError> {
    let value: Value = serde_json::from_str(raw_json).map_err(|e| ConfigError::Parse {
        detail: e.to_string(),
    })?;
    validate_structure(&value)?;
    let config: Config = serde_json::from_value(value).map_err(|e| ConfigError::Parse {
        detail: e.to_string(),
    })?;
    validate_semantics(&config)?;
    Ok(config)
}

/// Full load: interpolate `${ENV}` first, then `validate`.
pub fn load(
    raw_with_env_refs: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, ConfigError> {
    let interpolated = interpolate(raw_with_env_refs, lookup)?;
    validate(&interpolated)
}

/// Cross-row referential integrity the JSON Schema can't express.
pub fn validate_semantics(config: &Config) -> Result<(), ConfigError> {
    // Unique provider ids.
    let mut provider_ids = HashSet::new();
    for p in &config.providers {
        if !provider_ids.insert(&p.id) {
            return Err(ConfigError::Validation {
                detail: format!("duplicate provider id: {}", p.id),
            });
        }
    }
    // Unique key ids.
    let mut key_ids = HashSet::new();
    for k in &config.keys {
        if !key_ids.insert(&k.id) {
            return Err(ConfigError::Validation {
                detail: format!("duplicate key id: {}", k.id),
            });
        }
    }
    // Every route references a declared provider.
    for r in &config.routes {
        if !provider_ids.contains(&r.provider) {
            return Err(ConfigError::Validation {
                detail: format!("route {} references unknown provider {}", r.id, r.provider),
            });
        }
    }
    // Every guardrail attachment references a declared key.
    for g in &config.guardrails {
        if !key_ids.contains(&g.key_id) {
            return Err(ConfigError::Validation {
                detail: format!("guardrail attachment references unknown key {}", g.key_id),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::interpolate::map_lookup;

    #[test]
    fn validate_accepts_a_good_config() {
        let raw = r#"{
            "providers": [{ "id": "openai", "kind": "openai" }],
            "keys": [{ "id": "k1", "max_budget_usd": 10.0 }],
            "routes": [{ "id": "r1", "model": "gpt-4o", "provider": "openai" }]
        }"#;
        let c = validate(raw).unwrap();
        assert_eq!(c.routes.len(), 1);
    }

    #[test]
    fn route_to_unknown_provider_is_rejected() {
        let raw = r#"{
            "providers": [{ "id": "openai", "kind": "openai" }],
            "routes": [{ "id": "r1", "model": "gpt-4o", "provider": "ghost" }]
        }"#;
        assert!(matches!(validate(raw), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn duplicate_key_ids_are_rejected() {
        let raw = r#"{ "keys": [{ "id": "k1" }, { "id": "k1" }] }"#;
        assert!(matches!(validate(raw), Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn load_interpolates_then_validates() {
        let m: HashMap<String, String> = [("OPENAI_API_KEY".to_string(), "sk-live".to_string())]
            .into_iter()
            .collect();
        let raw = r#"{ "providers": [{ "id": "openai", "kind": "openai", "api_key": "${OPENAI_API_KEY}" }] }"#;
        let c = load(raw, &map_lookup(&m)).unwrap();
        assert_eq!(c.providers[0].api_key.as_deref(), Some("sk-live"));
    }

    #[test]
    fn load_fails_closed_on_missing_env() {
        let m: HashMap<String, String> = HashMap::new();
        let raw =
            r#"{ "providers": [{ "id": "openai", "kind": "openai", "api_key": "${MISSING}" }] }"#;
        assert!(matches!(
            load(raw, &map_lookup(&m)),
            Err(ConfigError::Interpolation { .. })
        ));
    }
}
