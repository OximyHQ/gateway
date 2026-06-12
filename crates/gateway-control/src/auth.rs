//! Bearer-token authentication: the auth-by-default chokepoint (design §2).
//! Every data route resolves the `Authorization: Bearer <secret>` header to a
//! `VirtualKey` and runs `ensure_usable(now)` BEFORE any governance or egress.
//! This is a plain function (not an Axum `FromRequestParts` extractor) so the
//! lifecycle can call it directly and tests don't need a full request — the
//! handler calls it first thing.

use gateway_spine::{Clock, VirtualKey};

use crate::error::GatewayError;
use crate::keystore::KeyStore;

/// Parse a bearer header value into its raw secret. `None` if missing/malformed.
pub fn parse_bearer(header: Option<&str>) -> Option<&str> {
    let value = header?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let secret = rest.trim();
    if secret.is_empty() {
        None
    } else {
        Some(secret)
    }
}

/// Resolve + validate a bearer secret into a usable `VirtualKey`. Fails closed:
/// missing header → 401 MissingAuth; unknown secret → 401 InvalidKey;
/// revoked/expired → 401 via `SpineError` mapping.
pub fn authenticate(
    keys: &dyn KeyStore,
    clock: &dyn Clock,
    auth_header: Option<&str>,
) -> Result<VirtualKey, GatewayError> {
    let secret = parse_bearer(auth_header).ok_or(GatewayError::MissingAuth)?;
    let key = keys.resolve(secret).ok_or(GatewayError::InvalidKey)?;
    key.ensure_usable(clock.now_ms())?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use gateway_spine::MockClock;

    fn store() -> StaticKeyStore {
        let mut s = StaticKeyStore::new();
        s.bootstrap("sk-good", None);
        s
    }

    #[test]
    fn parse_bearer_variants() {
        assert_eq!(parse_bearer(Some("Bearer sk-1")), Some("sk-1"));
        assert_eq!(parse_bearer(Some("bearer sk-2")), Some("sk-2"));
        assert_eq!(parse_bearer(Some("Bearer   sk-3  ")), Some("sk-3"));
        assert_eq!(parse_bearer(Some("Token sk-4")), None);
        assert_eq!(parse_bearer(Some("Bearer ")), None);
        assert_eq!(parse_bearer(None), None);
    }

    #[test]
    fn missing_header_is_missing_auth() {
        let s = store();
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, None).unwrap_err();
        assert!(matches!(err, GatewayError::MissingAuth));
    }

    #[test]
    fn unknown_secret_is_invalid_key() {
        let s = store();
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, Some("Bearer sk-nope")).unwrap_err();
        assert!(matches!(err, GatewayError::InvalidKey));
    }

    #[test]
    fn good_secret_resolves() {
        let s = store();
        let c = MockClock::new(0);
        let key = authenticate(&s, &c, Some("Bearer sk-good")).unwrap();
        assert_eq!(key.id, "key_bootstrap");
    }

    #[test]
    fn revoked_key_fails_closed() {
        let mut s = StaticKeyStore::new();
        let k = gateway_spine::VirtualKey {
            id: "k".into(),
            token_hash: gateway_spine::VirtualKey::hash_secret("sk-rev"),
            token_prefix: "sk-rev".into(),
            max_budget: None,
            limits: gateway_spine::RateLimits::default(),
            model_allowlist: None,
            tool_allowlist: None,
            expires_at: None,
            revoked: true,
            parent_id: None,
        };
        s.insert(k);
        let c = MockClock::new(0);
        let err = authenticate(&s, &c, Some("Bearer sk-rev")).unwrap_err();
        // revoked maps through SpineError -> 401
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
