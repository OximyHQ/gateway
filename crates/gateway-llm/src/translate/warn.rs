//! The no-silent-degradation taxonomy (design §5, Bifrost model). Translation
//! NEVER quietly drops a feature: a *lossy-but-safe* drop yields a `Warning`
//! (surfaced to the client via an overhead header / log by P1.4); a *semantic*
//! loss that would change the request's meaning yields `IngressError::Unsupported`
//! and the request is rejected. Both are values threaded to the caller — there is
//! no path that swallows either.

use serde::{Deserialize, Serialize};

/// A non-fatal translation notice: a feature was dropped or downgraded but the
/// request's meaning is preserved. P1.4 surfaces these (e.g. `x-oximy-warnings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// Machine-stable code, e.g. "param.dropped", "tool_choice.downgraded".
    pub code: String,
    /// Human-readable detail, e.g. "logit_bias is not supported by Anthropic".
    pub message: String,
}

impl Warning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Warning {
            code: code.into(),
            message: message.into(),
        }
    }

    /// A parameter present in the ingress request that this dialect/provider drops.
    pub fn dropped_param(name: &str, reason: &str) -> Self {
        Warning::new("param.dropped", format!("`{name}` was dropped: {reason}"))
    }
}

/// Accumulated warnings + the value they annotate. Returned from every ingress
/// parse so the caller decides how to surface them (never discarded here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated<T> {
    pub value: T,
    pub warnings: Vec<Warning>,
}

impl<T> Translated<T> {
    pub fn new(value: T) -> Self {
        Translated {
            value,
            warnings: Vec::new(),
        }
    }

    pub fn with_warning(mut self, w: Warning) -> Self {
        self.warnings.push(w);
        self
    }

    /// Map the inner value, preserving warnings.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Translated<U> {
        Translated {
            value: f(self.value),
            warnings: self.warnings,
        }
    }
}

/// Errors raised while translating a client request INTO the unified shape.
/// `Unsupported` is the typed no-silent-degradation seam (mirrors
/// `ProviderError::Unsupported` on the egress side).
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum IngressError {
    #[error("malformed request: {0}")]
    Malformed(String),
    #[error("request feature unsupported by this gateway: {feature}")]
    Unsupported { feature: String },
    #[error("missing required field: {0}")]
    MissingField(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_param_warning_is_coded() {
        let w = Warning::dropped_param("logit_bias", "Anthropic has no equivalent");
        assert_eq!(w.code, "param.dropped");
        assert!(w.message.contains("logit_bias"));
    }

    #[test]
    fn translated_threads_warnings_through_map() {
        let t = Translated::new(1u8)
            .with_warning(Warning::new("a", "first"))
            .map(|v| v + 1);
        assert_eq!(t.value, 2);
        assert_eq!(t.warnings.len(), 1);
        assert_eq!(t.warnings[0].code, "a");
    }

    #[test]
    fn unsupported_is_distinct_from_malformed() {
        let u = IngressError::Unsupported {
            feature: "audio input".into(),
        };
        let m = IngressError::Malformed("bad json".into());
        assert_ne!(u, m);
        assert!(u.to_string().contains("audio input"));
    }
}
