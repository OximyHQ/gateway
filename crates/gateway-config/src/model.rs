//! The one declarative config model. This is the single source of truth the UI,
//! API, CLI and Git all project to/from. Providers carry `${ENV}`-interpolated
//! secrets (never plaintext-at-rest in the file); keys/routes/guardrail
//! attachments/registry overrides are all rows here. Serializes as JSON
//! (YAML-compatible superset can be added later without changing this model).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub keys: Vec<KeyConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub guardrails: Vec<GuardrailAttachment>,
    #[serde(default)]
    pub registry_overrides: Vec<RegistryOverride>,
}

fn default_version() -> i64 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            providers: Vec::new(),
            keys: Vec::new(),
            routes: Vec::new(),
            guardrails: Vec::new(),
            registry_overrides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// `${OPENAI_API_KEY}`-style ref resolved at load; never the literal secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tpm: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteConfig {
    pub id: String,
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailAttachment {
    pub key_id: String,
    pub guardrail: String,
    #[serde(default)]
    pub stage: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryOverride {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_roundtrips_json() {
        let c = Config::default();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn missing_sections_default_to_empty() {
        // A minimal file with only a provider parses; the rest default.
        let json =
            r#"{"providers":[{"id":"openai","kind":"openai","api_key":"${OPENAI_API_KEY}"}]}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.version, 1);
        assert_eq!(c.providers.len(), 1);
        assert!(c.keys.is_empty());
        assert_eq!(c.providers[0].api_key.as_deref(), Some("${OPENAI_API_KEY}"));
    }
}
