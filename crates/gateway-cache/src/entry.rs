//! What we actually store. Two body shapes share one envelope:
//!   - `Unary`  → a single `ChatResponse` (the non-streaming path).
//!   - `Stream` → the FULLY-MATERIALIZED ordered list of `StreamDelta`s. A stream
//!     is only ever stored once its terminal delta (the one carrying usage) has
//!     arrived — a partial/aborted stream is NEVER cached (invariant, design §2).
//!
//! The envelope records the cached `TokenUsage` (so a HIT reports the real cached
//! token counts) and the `cost` that was originally billed (for $-saved
//! analytics). A HIT itself bills $0 — we re-serve, we don't re-charge.

use gateway_llm::{ChatResponse, StreamDelta};
use gateway_spine::{TokenUsage, Usd};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CachedBody {
    Unary(ChatResponse),
    Stream(Vec<StreamDelta>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResponse {
    pub body: CachedBody,
    /// The usage reported by the original upstream call (re-reported on HIT).
    pub usage: TokenUsage,
    /// The USD cost the original call billed — used only for $-saved analytics.
    pub original_cost: Usd,
    /// Unix epoch millis when this entry was written.
    pub stored_at_ms: i64,
    /// Absolute expiry in Unix epoch millis. `None` = no expiry (layer default applies upstream).
    pub expires_at_ms: Option<i64>,
}

impl CachedResponse {
    /// Age in milliseconds at `now_ms` (saturating at 0 for clock skew).
    pub fn age_ms(&self, now_ms: i64) -> i64 {
        (now_ms - self.stored_at_ms).max(0)
    }

    /// Whether this entry is expired at `now_ms`.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        match self.expires_at_ms {
            Some(exp) => now_ms >= exp,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::FinishReason;

    fn unary_entry() -> CachedResponse {
        let resp = ChatResponse {
            model: "gpt-4o".into(),
            content: vec![],
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            provider_response_id: None,
        };
        CachedResponse {
            body: CachedBody::Unary(resp),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            original_cost: Usd::from_micros(7_500),
            stored_at_ms: 1_000,
            expires_at_ms: Some(61_000),
        }
    }

    #[test]
    fn age_is_now_minus_stored() {
        let e = unary_entry();
        assert_eq!(e.age_ms(1_500), 500);
    }

    #[test]
    fn age_saturates_on_clock_skew() {
        let e = unary_entry();
        assert_eq!(e.age_ms(900), 0);
    }

    #[test]
    fn expiry_respects_absolute_ms() {
        let e = unary_entry();
        assert!(!e.is_expired(60_999));
        assert!(e.is_expired(61_000));
    }

    #[test]
    fn no_expiry_never_expires() {
        let mut e = unary_entry();
        e.expires_at_ms = None;
        assert!(!e.is_expired(i64::MAX));
    }

    #[test]
    fn serde_roundtrips_both_bodies() {
        let unary = unary_entry();
        let s = serde_json::to_string(&unary).unwrap();
        let back: CachedResponse = serde_json::from_str(&s).unwrap();
        assert!(matches!(back.body, CachedBody::Unary(_)));

        let stream = CachedResponse {
            body: CachedBody::Stream(vec![
                StreamDelta::text("Hel"),
                StreamDelta::text("lo"),
                StreamDelta::finish(
                    FinishReason::Stop,
                    TokenUsage {
                        input_tokens: 10,
                        output_tokens: 2,
                        ..Default::default()
                    },
                ),
            ]),
            ..unary_entry()
        };
        let s = serde_json::to_string(&stream).unwrap();
        let back: CachedResponse = serde_json::from_str(&s).unwrap();
        match back.body {
            CachedBody::Stream(deltas) => assert_eq!(deltas.len(), 3),
            _ => panic!("expected stream body"),
        }
    }
}
