//! The virtual key: the unit of governance. The secret is never stored — only
//! its SHA-256 hex hash and a display prefix. A key carries an optional USD
//! budget, rate limits, a model allowlist, and an expiry; revocation and expiry
//! make it unusable. (Persistence + parent/child attenuation: P1.6 / P3.)

use sha2::{Digest, Sha256};

use crate::error::SpineError;
use crate::money::Usd;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct RateLimits {
    /// Requests per minute.
    pub rpm: Option<i64>,
    /// Tokens per minute.
    pub tpm: Option<i64>,
    /// Max concurrent in-flight requests.
    pub max_parallel: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualKey {
    pub id: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub max_budget: Option<Usd>,
    pub limits: RateLimits,
    /// `None` = all models allowed.
    pub model_allowlist: Option<Vec<String>>,
    /// Namespaced MCP tool allowlist (`server__tool`). `None` = all tools allowed.
    /// Carried on the key so the federation ACL can be re-seeded after a restart.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Unix epoch millis; `None` = never expires.
    pub expires_at: Option<i64>,
    pub revoked: bool,
    pub parent_id: Option<String>,
}

impl VirtualKey {
    /// SHA-256 hex of a secret. The one place hashing happens.
    pub fn hash_secret(secret: &str) -> String {
        let mut h = Sha256::new();
        h.update(secret.as_bytes());
        hex::encode(h.finalize())
    }

    pub fn verify(&self, secret: &str) -> bool {
        Self::hash_secret(secret) == self.token_hash
    }

    /// Usable = not revoked and not past expiry at `now_ms`.
    pub fn ensure_usable(&self, now_ms: i64) -> Result<(), SpineError> {
        if self.revoked {
            return Err(SpineError::KeyRevoked {
                key_id: self.id.clone(),
            });
        }
        if let Some(exp) = self.expires_at
            && now_ms >= exp
        {
            return Err(SpineError::KeyExpired {
                key_id: self.id.clone(),
            });
        }
        Ok(())
    }

    pub fn allows_model(&self, model: &str) -> bool {
        match &self.model_allowlist {
            None => true,
            Some(list) => list.iter().any(|m| m == model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> VirtualKey {
        VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-secret"),
            token_prefix: "sk-secre".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            tool_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        }
    }

    #[test]
    fn hash_is_deterministic_and_hides_secret() {
        let h = VirtualKey::hash_secret("sk-secret");
        assert_eq!(h, VirtualKey::hash_secret("sk-secret"));
        assert_ne!(h, "sk-secret");
        assert_eq!(h.len(), 64); // sha256 hex
    }

    #[test]
    fn verify_matches_only_correct_secret() {
        let k = key();
        assert!(k.verify("sk-secret"));
        assert!(!k.verify("sk-wrong"));
    }

    #[test]
    fn revoked_key_is_unusable() {
        let mut k = key();
        k.revoked = true;
        assert!(matches!(
            k.ensure_usable(0),
            Err(SpineError::KeyRevoked { .. })
        ));
    }

    #[test]
    fn expired_key_is_unusable() {
        let mut k = key();
        k.expires_at = Some(1000);
        assert!(k.ensure_usable(999).is_ok());
        assert!(matches!(
            k.ensure_usable(1000),
            Err(SpineError::KeyExpired { .. })
        ));
    }

    #[test]
    fn allowlist_gates_models() {
        let mut k = key();
        assert!(k.allows_model("anything")); // None = all
        k.model_allowlist = Some(vec!["gpt-4o".into()]);
        assert!(k.allows_model("gpt-4o"));
        assert!(!k.allows_model("claude-3-5-sonnet"));
    }
}
