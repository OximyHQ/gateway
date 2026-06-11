//! The cache crate's error taxonomy. Store backends and the registry loader map
//! their failures into these. Cache errors are NEVER fatal to a request: the
//! lifecycle (P1.4) treats any `CacheError` on a read as a MISS and any error on
//! a write as a no-op — caching is a best-effort optimization, never a gate.

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache store backend error: {0}")]
    Backend(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("registry source error: {0}")]
    RegistrySource(String),
}

impl From<serde_json::Error> for CacheError {
    fn from(e: serde_json::Error) -> Self {
        CacheError::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_error_maps_to_serialization() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{not json");
        let e: CacheError = bad.unwrap_err().into();
        assert!(matches!(e, CacheError::Serialization(_)));
    }

    #[test]
    fn backend_error_displays() {
        let e = CacheError::Backend("connection refused".into());
        assert!(e.to_string().contains("connection refused"));
    }
}
