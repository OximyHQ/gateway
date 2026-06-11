//! Tool-description pinning / rug-pull detection.
//!
//! Every tool that enters the registry gets its **description + input_schema**
//! hashed (SHA-256).  On subsequent re-registration (e.g. after a server
//! restart) the hash is compared; a mismatch triggers a `DescriptionHashChanged`
//! error so the operator is alerted before the new definition silently takes
//! effect.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Compute the description-pin hash for a tool.
///
/// Inputs: the tool's name, optional description, and its input_schema as a
/// canonical JSON string (keys sorted by `serde_json` default which is
/// insertion-order; callers should normalise if needed — for our purposes
/// round-trip through `serde_json::Value` is sufficient).
pub fn description_hash(name: &str, description: Option<&str>, input_schema: &Value) -> String {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update(b"\x00");
    h.update(description.unwrap_or("").as_bytes());
    h.update(b"\x00");
    h.update(input_schema.to_string().as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_deterministic() {
        let schema = json!({"type":"object","properties":{"x":{"type":"string"}}});
        let h1 = description_hash("echo", Some("echoes text"), &schema);
        let h2 = description_hash("echo", Some("echoes text"), &schema);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn different_description_yields_different_hash() {
        let schema = json!({"type":"object"});
        let h1 = description_hash("t", Some("original"), &schema);
        let h2 = description_hash("t", Some("CHANGED"), &schema);
        assert_ne!(h1, h2);
    }

    #[test]
    fn different_schema_yields_different_hash() {
        let h1 = description_hash("t", None, &json!({"type":"object"}));
        let h2 = description_hash("t", None, &json!({"type":"string"}));
        assert_ne!(h1, h2);
    }

    #[test]
    fn none_and_empty_description_are_equivalent() {
        let schema = json!({});
        let h1 = description_hash("t", None, &schema);
        let h2 = description_hash("t", Some(""), &schema);
        assert_eq!(h1, h2);
    }
}
