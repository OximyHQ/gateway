//! Per-request cache controls. The same knobs are accepted two ways (design §5):
//! an `x-oximy-cache` HTTP header (comma-separated directives) OR an `oximy_cache`
//! object in the request body. The body form is parsed by the ingress layer
//! (P1.4) into `CacheControl`; the header form is parsed here. When both are
//! present the BODY wins (it is the more explicit, structured form).
//!
//! Directives:
//!   - `no-store`   → serve from cache if present, but do not WRITE this response.
//!   - `no-cache` / `skip` → bypass the cache entirely for READ (force MISS) and write fresh.
//!   - `ttl=<secs>` → override the default entry TTL for the write.
//!   - `ns=<name>`  → namespace/seed; entries in different namespaces never collide
//!     even with identical bodies (per-tenant or per-experiment isolation).

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CacheControl {
    /// Read from cache but do not write this response into it.
    #[serde(default)]
    pub no_store: bool,
    /// Bypass the cache for the READ (always MISS); still writes unless `no_store`.
    #[serde(default)]
    pub skip: bool,
    /// Per-request TTL override in seconds. `None` = use the layer default.
    #[serde(default)]
    pub ttl_secs: Option<i64>,
    /// Namespace/seed mixed into the key. Different namespaces never collide.
    #[serde(default)]
    pub namespace: Option<String>,
}

impl CacheControl {
    /// Parse the comma-separated `x-oximy-cache` header value.
    /// Unknown directives are ignored (forward-compatible).
    pub fn from_header(value: &str) -> Self {
        let mut c = CacheControl::default();
        for raw in value.split(',') {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                match k.trim() {
                    "ttl" => c.ttl_secs = v.trim().parse::<i64>().ok(),
                    "ns" => c.namespace = Some(v.trim().to_string()),
                    _ => {}
                }
            } else {
                match part {
                    "no-store" => c.no_store = true,
                    "no-cache" | "skip" => c.skip = true,
                    _ => {}
                }
            }
        }
        c
    }

    /// Merge a body-supplied control over a header-supplied one; the body wins on
    /// every field it sets. (`skip`/`no_store` OR together: either source asking
    /// to bypass/not-store is honored.)
    pub fn merge_body_over_header(header: CacheControl, body: CacheControl) -> CacheControl {
        CacheControl {
            no_store: header.no_store || body.no_store,
            skip: header.skip || body.skip,
            ttl_secs: body.ttl_secs.or(header.ttl_secs),
            namespace: body.namespace.or(header.namespace),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags_and_kv_from_header() {
        let c = CacheControl::from_header("no-store, ttl=300 , ns=tenant-a");
        assert!(c.no_store);
        assert!(!c.skip);
        assert_eq!(c.ttl_secs, Some(300));
        assert_eq!(c.namespace.as_deref(), Some("tenant-a"));
    }

    #[test]
    fn skip_aliases_no_cache() {
        assert!(CacheControl::from_header("no-cache").skip);
        assert!(CacheControl::from_header("skip").skip);
    }

    #[test]
    fn unknown_directives_ignored() {
        let c = CacheControl::from_header("frobnicate, ttl=10, mystery=1");
        assert_eq!(c.ttl_secs, Some(10));
        assert!(!c.no_store && !c.skip);
    }

    #[test]
    fn body_wins_over_header() {
        let header = CacheControl::from_header("ttl=60, ns=from-header");
        let body = CacheControl {
            ttl_secs: Some(5),
            namespace: Some("from-body".into()),
            ..Default::default()
        };
        let merged = CacheControl::merge_body_over_header(header, body);
        assert_eq!(merged.ttl_secs, Some(5));
        assert_eq!(merged.namespace.as_deref(), Some("from-body"));
    }

    #[test]
    fn bypass_flags_or_together() {
        let header = CacheControl {
            skip: true,
            ..Default::default()
        };
        let body = CacheControl {
            no_store: true,
            ..Default::default()
        };
        let merged = CacheControl::merge_body_over_header(header, body);
        assert!(merged.skip);
        assert!(merged.no_store);
    }
}
