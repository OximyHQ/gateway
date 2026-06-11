//! The cache key. SHA-256 over a CANONICAL byte string built from:
//!   tenant_id · namespace · endpoint · model · canonical(request_body)
//! `canonical(body)` is the request JSON with (a) keys sorted recursively and
//! (b) the cache-control envelope (`oximy_cache`) stripped — so two requests that
//! differ only in their cache directives (or in key ordering) collide, while any
//! semantic difference (a changed message, temperature, tool) produces a fresh
//! key. The tenant_id is ALWAYS part of the key: one tenant can never read
//! another tenant's cached completion (isolation invariant, design §5).

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CacheKey(String);

impl CacheKey {
    /// Build a key. `body` is the raw request JSON value (already deserialized).
    pub fn compute(
        tenant_id: &str,
        namespace: Option<&str>,
        endpoint: &str,
        model: &str,
        body: &serde_json::Value,
    ) -> Self {
        let canonical = canonicalize(body);
        let mut h = Sha256::new();
        // Length-prefixed framing so field boundaries can't be smuggled across
        // (e.g. tenant "a"+model "b" must differ from tenant "ab"+model "").
        for field in [tenant_id, namespace.unwrap_or(""), endpoint, model] {
            h.update((field.len() as u64).to_le_bytes());
            h.update(field.as_bytes());
        }
        let body_bytes = canonical.as_bytes();
        h.update((body_bytes.len() as u64).to_le_bytes());
        h.update(body_bytes);
        CacheKey(hex::encode(h.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Recursively sort object keys and drop the `oximy_cache` envelope, returning a
/// stable canonical JSON string. Arrays keep order (order is semantic).
fn canonicalize(value: &serde_json::Value) -> String {
    fn norm(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut sorted: std::collections::BTreeMap<String, serde_json::Value> =
                    std::collections::BTreeMap::new();
                for (k, val) in map {
                    if k == "oximy_cache" {
                        continue; // cache directives never affect the key
                    }
                    sorted.insert(k.clone(), norm(val));
                }
                serde_json::Value::Object(sorted.into_iter().collect())
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(norm).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&norm(value)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_is_64_hex_chars() {
        let k = CacheKey::compute(
            "t1",
            None,
            "/v1/chat/completions",
            "gpt-4o",
            &json!({"a":1}),
        );
        assert_eq!(k.as_str().len(), 64);
        assert!(k.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn object_key_order_does_not_matter() {
        let a = CacheKey::compute("t", None, "/e", "m", &json!({"x":1,"y":2}));
        let b = CacheKey::compute("t", None, "/e", "m", &json!({"y":2,"x":1}));
        assert_eq!(a, b);
    }

    #[test]
    fn cache_directives_do_not_affect_key() {
        let plain = json!({"messages":[{"role":"user","content":"hi"}]});
        let with_ctl =
            json!({"messages":[{"role":"user","content":"hi"}], "oximy_cache":{"ttl_secs":5}});
        assert_eq!(
            CacheKey::compute("t", None, "/e", "m", &plain),
            CacheKey::compute("t", None, "/e", "m", &with_ctl),
        );
    }

    #[test]
    fn semantic_difference_changes_key() {
        let a = CacheKey::compute(
            "t",
            None,
            "/e",
            "m",
            &json!({"messages":[{"content":"hi"}]}),
        );
        let b = CacheKey::compute(
            "t",
            None,
            "/e",
            "m",
            &json!({"messages":[{"content":"bye"}]}),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn tenant_isolation() {
        let body = json!({"messages":[{"content":"hi"}]});
        let a = CacheKey::compute("tenant-a", None, "/e", "m", &body);
        let b = CacheKey::compute("tenant-b", None, "/e", "m", &body);
        assert_ne!(a, b, "different tenants must never share a key");
    }

    #[test]
    fn namespace_isolation() {
        let body = json!({"messages":[{"content":"hi"}]});
        let a = CacheKey::compute("t", Some("exp-1"), "/e", "m", &body);
        let b = CacheKey::compute("t", Some("exp-2"), "/e", "m", &body);
        assert_ne!(a, b);
    }

    #[test]
    fn array_order_is_semantic() {
        let a = CacheKey::compute("t", None, "/e", "m", &json!({"msgs":[1,2]}));
        let b = CacheKey::compute("t", None, "/e", "m", &json!({"msgs":[2,1]}));
        assert_ne!(a, b, "reordering messages is a different request");
    }

    #[test]
    fn field_framing_prevents_smuggling() {
        // tenant "a", model "bc" must differ from tenant "ab", model "c".
        let body = json!({});
        let a = CacheKey::compute("a", None, "/e", "bc", &body);
        let b = CacheKey::compute("ab", None, "/e", "c", &body);
        assert_ne!(a, b);
    }
}
