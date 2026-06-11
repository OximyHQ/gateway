//! Content-capture policy resolution. Three independent opt-outs compose to a
//! single `CaptureMode` (Task 2). Precedence is AND-of-permits, so the most
//! restrictive level always wins — the gateway defaults to metadata-only and
//! only stores text when the operator, the key owner, AND the caller all opt in.
//!
//! The per-request signal comes from a header (P1.4 maps `x-oximy-log-content:
//! none` → opt-out). `RequestCapturePref::Default` means "no explicit request
//! preference" and inherits the global+key decision.

use crate::row::CaptureMode;

/// Global operator default for content logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalCapture {
    /// Operator allows content logging (subject to key + request opt-out).
    Enabled,
    /// Operator forbids content logging product-wide (hard floor).
    Disabled,
}

/// Per-request preference parsed from the request header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestCapturePref {
    /// No explicit request-level signal; inherit global + key.
    Default,
    /// Caller explicitly opted OUT for this request.
    OptOut,
}

#[derive(Debug, Clone, Copy)]
pub struct CapturePolicy {
    pub global: GlobalCapture,
    /// Per-key default (e.g. a key flagged "never log content").
    pub key_enabled: bool,
}

impl CapturePolicy {
    /// Resolve to the row's `CaptureMode`. Disabled global is a hard floor;
    /// otherwise content is captured only if the key permits AND the request did
    /// not opt out.
    pub fn resolve(&self, request: RequestCapturePref) -> CaptureMode {
        let global_enabled = matches!(self.global, GlobalCapture::Enabled);
        let request_enabled = matches!(request, RequestCapturePref::Default);
        CaptureMode::resolve(global_enabled, self.key_enabled, request_enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_disabled_is_a_hard_floor() {
        let p = CapturePolicy {
            global: GlobalCapture::Disabled,
            key_enabled: true,
        };
        assert_eq!(
            p.resolve(RequestCapturePref::Default),
            CaptureMode::Metadata
        );
    }

    #[test]
    fn key_opt_out_wins_even_if_global_enabled() {
        let p = CapturePolicy {
            global: GlobalCapture::Enabled,
            key_enabled: false,
        };
        assert_eq!(
            p.resolve(RequestCapturePref::Default),
            CaptureMode::Metadata
        );
    }

    #[test]
    fn request_opt_out_wins() {
        let p = CapturePolicy {
            global: GlobalCapture::Enabled,
            key_enabled: true,
        };
        assert_eq!(p.resolve(RequestCapturePref::OptOut), CaptureMode::Metadata);
    }

    #[test]
    fn full_only_when_all_three_permit() {
        let p = CapturePolicy {
            global: GlobalCapture::Enabled,
            key_enabled: true,
        };
        assert_eq!(p.resolve(RequestCapturePref::Default), CaptureMode::Full);
    }
}
