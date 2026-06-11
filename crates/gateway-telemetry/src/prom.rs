//! Authenticated Prometheus exposition. `/metrics` is NEVER open (design §2).
//! The handler takes the raw `Authorization` header value (if any) and the
//! configured scrape token, compares them in constant time, and returns either
//! a 401 or the OpenMetrics body. Transport (axum) is wired in P1.4; this is the
//! pure auth+render core.

use std::sync::Arc;

use subtle::ConstantTimeEq;

use crate::metrics::GatewayMetrics;

/// The Prometheus content type required by scrapers for OpenMetrics text.
pub const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";

/// Outcome of a `/metrics` request: status + body + content type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

pub struct MetricsEndpoint {
    metrics: Arc<GatewayMetrics>,
    /// Bearer token required to scrape. Empty string = a misconfiguration that
    /// we treat as "deny all" (never accidentally open).
    scrape_token: String,
}

impl MetricsEndpoint {
    pub fn new(metrics: Arc<GatewayMetrics>, scrape_token: impl Into<String>) -> Self {
        Self {
            metrics,
            scrape_token: scrape_token.into(),
        }
    }

    /// `authorization` is the raw header value, e.g. `Some("Bearer abc")`.
    pub fn handle(&self, authorization: Option<&str>) -> MetricsResponse {
        if !self.authorized(authorization) {
            return MetricsResponse {
                status: 401,
                content_type: "text/plain; charset=utf-8",
                body: "unauthorized\n".into(),
            };
        }
        MetricsResponse {
            status: 200,
            content_type: METRICS_CONTENT_TYPE,
            body: self.metrics.render(),
        }
    }

    fn authorized(&self, authorization: Option<&str>) -> bool {
        // Empty configured token = deny all (fail closed on misconfig).
        if self.scrape_token.is_empty() {
            return false;
        }
        let Some(header) = authorization else {
            return false;
        };
        let Some(presented) = header.strip_prefix("Bearer ") else {
            return false;
        };
        // Constant-time compare; length-mismatch short-circuits to false but the
        // equal-length path is timing-safe.
        let a = presented.as_bytes();
        let b = self.scrape_token.as_bytes();
        a.len() == b.len() && a.ct_eq(b).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(token: &str) -> MetricsEndpoint {
        MetricsEndpoint::new(Arc::new(GatewayMetrics::new()), token)
    }

    #[test]
    fn missing_auth_is_401() {
        let r = endpoint("secret").handle(None);
        assert_eq!(r.status, 401);
        assert!(!r.body.contains("gateway_requests"));
    }

    #[test]
    fn wrong_token_is_401() {
        let r = endpoint("secret").handle(Some("Bearer nope"));
        assert_eq!(r.status, 401);
    }

    #[test]
    fn non_bearer_scheme_is_401() {
        let r = endpoint("secret").handle(Some("Basic secret"));
        assert_eq!(r.status, 401);
    }

    #[test]
    fn correct_token_renders_metrics() {
        let r = endpoint("secret").handle(Some("Bearer secret"));
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, METRICS_CONTENT_TYPE);
        assert!(r.body.contains("gateway_dropped_rows_total"));
    }

    #[test]
    fn empty_configured_token_denies_all() {
        let r = endpoint("").handle(Some("Bearer "));
        assert_eq!(r.status, 401);
    }
}
