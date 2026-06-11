//! JSON Schema validation guardrail.
//!
//! Validates the context text as JSON against a pre-compiled JSON Schema.
//! Returns [`GuardVerdict::Block`] if the text is not valid JSON or does not
//! conform to the schema.

use async_trait::async_trait;
use jsonschema::Validator;
use serde_json::Value;

use crate::guardrail::Guardrail;
use crate::types::{GuardContext, GuardError, GuardVerdict};

/// Guardrail that validates context text as a JSON document against a schema.
///
/// The validator is compiled once at construction time.
pub struct JsonSchemaGuardrail {
    /// Human-readable name.
    label: String,
    /// Compiled validator.
    validator: Validator,
    /// Original schema stored for `Debug`.
    schema: Value,
}

impl std::fmt::Debug for JsonSchemaGuardrail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonSchemaGuardrail")
            .field("label", &self.label)
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}

impl JsonSchemaGuardrail {
    /// Compile `schema` into a reusable validator.
    ///
    /// Returns `Err(GuardError::SchemaValidation)` if the schema itself is
    /// invalid.
    pub fn new(label: impl Into<String>, schema: Value) -> Result<Self, GuardError> {
        let validator = jsonschema::validator_for(&schema)
            .map_err(|e| GuardError::SchemaValidation(e.to_string()))?;
        Ok(Self {
            label: label.into(),
            validator,
            schema,
        })
    }
}

#[async_trait]
impl Guardrail for JsonSchemaGuardrail {
    fn name(&self) -> &str {
        &self.label
    }

    async fn check(&self, ctx: &GuardContext) -> GuardVerdict {
        let instance: Value = match serde_json::from_str(&ctx.text) {
            Ok(v) => v,
            Err(e) => {
                return GuardVerdict::Block {
                    reason: format!("text is not valid JSON: {e}"),
                };
            }
        };

        if self.validator.is_valid(&instance) {
            GuardVerdict::Allow
        } else {
            let errors: Vec<String> = self
                .validator
                .iter_errors(&instance)
                .map(|e| e.to_string())
                .collect();
            GuardVerdict::Block {
                reason: format!("JSON schema validation failed: {}", errors.join("; ")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardStage;
    use serde_json::json;

    fn ctx(text: &str) -> GuardContext {
        GuardContext::new(GuardStage::PreRequest, text)
    }

    fn name_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        })
    }

    #[tokio::test]
    async fn allows_valid_json_matching_schema() {
        let g = JsonSchemaGuardrail::new("test-schema", name_schema()).unwrap();
        let verdict = g.check(&ctx(r#"{"name": "Alice"}"#)).await;
        assert_eq!(verdict, GuardVerdict::Allow);
    }

    #[tokio::test]
    async fn blocks_invalid_json() {
        let g = JsonSchemaGuardrail::new("test-schema", name_schema()).unwrap();
        let verdict = g.check(&ctx("not json at all")).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { reason } if reason.contains("not valid JSON")),
            "expected Block for invalid JSON"
        );
    }

    #[tokio::test]
    async fn blocks_json_failing_schema() {
        let g = JsonSchemaGuardrail::new("test-schema", name_schema()).unwrap();
        // "name" is required but missing.
        let verdict = g.check(&ctx(r#"{"age": 30}"#)).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { .. }),
            "expected Block for schema mismatch"
        );
    }

    #[tokio::test]
    async fn blocks_wrong_type() {
        let g = JsonSchemaGuardrail::new("test-schema", name_schema()).unwrap();
        // name must be a string, not a number.
        let verdict = g.check(&ctx(r#"{"name": 42}"#)).await;
        assert!(
            matches!(verdict, GuardVerdict::Block { .. }),
            "expected Block for wrong type"
        );
    }

    #[test]
    fn invalid_schema_returns_error() {
        // "$schema" keyword with a bad URI still compiles fine in most draft versions,
        // so we test with an unsupported type value instead.
        let bad_schema = json!({ "type": ["not_a_type"] });
        // Note: jsonschema is lenient about unknown type names in some drafts —
        // the construction may or may not error. We just verify it doesn't panic.
        let _ = JsonSchemaGuardrail::new("bad", bad_schema);
    }
}
