//! The JSON Schema that validates a config BEFORE it touches running state — the
//! one gate for UI = API = CLI = Git. Structural rules live here (required ids,
//! types, non-negative budgets); cross-row referential checks (a route's
//! provider exists) live in `load::validate_semantics`.

use serde_json::{Value, json};

use crate::error::ConfigError;

/// The config JSON Schema (draft 2020-12).
pub fn config_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "version": { "type": "integer", "minimum": 1 },
            "providers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "kind"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "kind": { "type": "string", "minLength": 1 },
                        "base_url": { "type": "string" },
                        "api_key": { "type": "string" }
                    }
                }
            },
            "keys": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "max_budget_usd": { "type": "number", "minimum": 0 },
                        "rpm": { "type": "integer", "minimum": 0 },
                        "tpm": { "type": "integer", "minimum": 0 },
                        "max_parallel": { "type": "integer", "minimum": 0 },
                        "model_allowlist": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "routes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "model", "provider"],
                    "properties": {
                        "id": { "type": "string", "minLength": 1 },
                        "model": { "type": "string", "minLength": 1 },
                        "provider": { "type": "string", "minLength": 1 }
                    }
                }
            }
        }
    })
}

/// Validate a raw JSON value against the schema. Returns the first error message.
pub fn validate_structure(value: &Value) -> Result<(), ConfigError> {
    let schema = config_schema();
    let compiled = jsonschema::validator_for(&schema).map_err(|e| ConfigError::Validation {
        detail: format!("schema compile: {e}"),
    })?;
    if let Some(err) = compiled.iter_errors(value).next() {
        return Err(ConfigError::Validation {
            detail: err.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config_passes() {
        let v = json!({
            "version": 1,
            "providers": [{ "id": "openai", "kind": "openai" }],
            "keys": [{ "id": "k1", "max_budget_usd": 10.0 }]
        });
        validate_structure(&v).unwrap();
    }

    #[test]
    fn provider_missing_id_fails() {
        let v = json!({ "providers": [{ "kind": "openai" }] });
        assert!(matches!(
            validate_structure(&v),
            Err(ConfigError::Validation { .. })
        ));
    }

    #[test]
    fn negative_budget_fails() {
        let v = json!({ "keys": [{ "id": "k1", "max_budget_usd": -5.0 }] });
        assert!(matches!(
            validate_structure(&v),
            Err(ConfigError::Validation { .. })
        ));
    }
}
