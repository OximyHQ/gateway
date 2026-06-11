//! One row per governed request. Populated by the HTTP lifecycle (P1.4) from the
//! committed call: the spine-priced `Usd` cost and the provider-reported
//! `TokenUsage` are copied in verbatim — never re-derived here. Content
//! (prompt/response text) is captured ONLY when the resolved capture mode is
//! `Full`; otherwise the text fields stay `None` (metadata-only, fail-safe).
//!
//! `kind` discriminates LLM rows from the MCP tool-call rows P2 will append to
//! the same store, so that extension is purely additive.

use gateway_spine::{TokenUsage, Usd};

/// Plane discriminant. P1.7 only emits `Llm`; P2 adds `McpTool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Llm,
    McpTool,
}

/// Resolved content-capture decision for one request. The three opt-out levels
/// (global config, per-key, per-request header) collapse to this: `Full` only
/// when EVERY level permits; any opt-out anywhere → `Metadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Prompt + response text stored.
    Full,
    /// Counts/cost/latency only; no text. The fail-safe default.
    Metadata,
}

impl CaptureMode {
    /// Fail-safe resolution: text is stored only if `global_enabled` AND
    /// `key_enabled` AND `request_enabled` are all true. Any opt-out wins.
    pub fn resolve(global_enabled: bool, key_enabled: bool, request_enabled: bool) -> Self {
        if global_enabled && key_enabled && request_enabled {
            CaptureMode::Full
        } else {
            CaptureMode::Metadata
        }
    }

    pub fn captures_text(self) -> bool {
        matches!(self, CaptureMode::Full)
    }
}

/// Cache outcome for the request (mirrors the `x-cache` response header values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Hit,
    Miss,
    /// Caching not applicable (e.g. streaming with caching disabled).
    Bypass,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequestLogRow {
    pub ts_ms: i64,
    pub kind: RequestKind,
    /// Virtual-key id that authorized the call.
    pub key_id: String,
    /// Optional attribution tags (P1.4 fills from key metadata / request).
    pub team_id: Option<String>,
    pub user_id: Option<String>,
    pub tags: Vec<String>,
    pub model: String,
    pub provider: String,
    pub usage: TokenUsage,
    /// Spine-priced cost, committed. µUSD, never re-derived with floats.
    pub cost: Usd,
    /// Total wall time of the request, ms.
    pub latency_ms: i64,
    /// Time-to-first-token for streamed responses; `None` for non-streamed.
    pub ttft_ms: Option<i64>,
    /// HTTP status returned to the client.
    pub status: u16,
    /// Which deployment/provider actually served it (the `x-served-by` value).
    pub served_by: String,
    /// Whether a fallback route fired (the `x-fallback` value).
    pub fallback_fired: bool,
    pub cache_status: CacheStatus,
    pub capture_mode: CaptureMode,
    /// Populated only when `capture_mode == Full`.
    pub request_text: Option<String>,
    pub response_text: Option<String>,
}

impl RequestLogRow {
    /// Strip text if the resolved mode is metadata-only. Idempotent; called
    /// defensively before the row enters the store so a mis-populated text field
    /// can never leak past an opt-out.
    pub fn enforce_capture(mut self) -> Self {
        if !self.capture_mode.captures_text() {
            self.request_text = None;
            self.response_text = None;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1_000,
            kind: RequestKind::Llm,
            key_id: "key_1".into(),
            team_id: Some("team_a".into()),
            user_id: Some("user_x".into()),
            tags: vec!["prod".into()],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            },
            cost: Usd::from_micros(7_500),
            latency_ms: 820,
            ttft_ms: Some(140),
            status: 200,
            served_by: "openai/gpt-4o".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("hello".into()),
            response_text: Some("hi there".into()),
        }
    }

    #[test]
    fn resolve_is_fail_safe() {
        assert_eq!(CaptureMode::resolve(true, true, true), CaptureMode::Full);
        assert_eq!(
            CaptureMode::resolve(false, true, true),
            CaptureMode::Metadata
        );
        assert_eq!(
            CaptureMode::resolve(true, false, true),
            CaptureMode::Metadata
        );
        assert_eq!(
            CaptureMode::resolve(true, true, false),
            CaptureMode::Metadata
        );
    }

    #[test]
    fn enforce_capture_strips_text_in_metadata_mode() {
        let r = row(CaptureMode::Metadata).enforce_capture();
        assert!(r.request_text.is_none());
        assert!(r.response_text.is_none());
    }

    #[test]
    fn enforce_capture_keeps_text_in_full_mode() {
        let r = row(CaptureMode::Full).enforce_capture();
        assert_eq!(r.request_text.as_deref(), Some("hello"));
        assert_eq!(r.response_text.as_deref(), Some("hi there"));
    }

    #[test]
    fn row_roundtrips_json() {
        let r = row(CaptureMode::Metadata).enforce_capture();
        let s = serde_json::to_string(&r).unwrap();
        let back: RequestLogRow = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
