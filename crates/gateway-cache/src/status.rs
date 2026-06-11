//! The cache-status response metadata (design §5: cache `HIT/MISS/age` headers).
//! The layer returns a `CacheOutcome` from every read; P1.4 renders it into the
//! `x-oximy-cache-status` (HIT|MISS|BYPASS) and `x-oximy-cache-age-ms` headers.
//! BYPASS is distinct from MISS: it means the caller asked to `skip` the read,
//! so a MISS-rate metric isn't polluted by deliberate bypasses.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Hit,
    Miss,
    Bypass,
}

impl CacheStatus {
    pub fn as_header(self) -> &'static str {
        match self {
            CacheStatus::Hit => "HIT",
            CacheStatus::Miss => "MISS",
            CacheStatus::Bypass => "BYPASS",
        }
    }
}

/// What a cache read produced: a status, an optional age (only on HIT), and the
/// cached value (only on HIT). The lifecycle uses `value` if present, else calls
/// upstream.
pub struct CacheOutcome {
    pub status: CacheStatus,
    pub age_ms: Option<i64>,
    pub value: Option<crate::entry::CachedResponse>,
}

impl CacheOutcome {
    pub fn miss() -> Self {
        CacheOutcome {
            status: CacheStatus::Miss,
            age_ms: None,
            value: None,
        }
    }
    pub fn bypass() -> Self {
        CacheOutcome {
            status: CacheStatus::Bypass,
            age_ms: None,
            value: None,
        }
    }
    pub fn hit(value: crate::entry::CachedResponse, age_ms: i64) -> Self {
        CacheOutcome {
            status: CacheStatus::Hit,
            age_ms: Some(age_ms),
            value: Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_strings() {
        assert_eq!(CacheStatus::Hit.as_header(), "HIT");
        assert_eq!(CacheStatus::Miss.as_header(), "MISS");
        assert_eq!(CacheStatus::Bypass.as_header(), "BYPASS");
    }

    #[test]
    fn miss_and_bypass_have_no_value() {
        assert!(CacheOutcome::miss().value.is_none());
        assert!(CacheOutcome::miss().age_ms.is_none());
        assert_eq!(CacheOutcome::bypass().status, CacheStatus::Bypass);
    }
}
