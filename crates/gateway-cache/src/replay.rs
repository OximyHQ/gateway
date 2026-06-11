//! Streaming-response replay (design §5). On a HIT for a streaming request the
//! server must re-emit the EXACT delta sequence that was originally streamed —
//! byte-faithful, terminal usage delta included — so a strict client cannot tell
//! a replay from a live stream. `replay_stream` adapts a cached `Vec<StreamDelta>`
//! into an owned iterator the HTTP layer (P1.4) drives to produce SSE frames.
//! It returns `None` for a unary cache body (the caller serves it non-streamed).

use gateway_llm::StreamDelta;

use crate::entry::{CachedBody, CachedResponse};

/// Yields the cached deltas in order. If the cached body is unary, returns `None`.
pub fn replay_stream(entry: &CachedResponse) -> Option<impl Iterator<Item = StreamDelta> + '_> {
    match &entry.body {
        CachedBody::Stream(deltas) => Some(deltas.iter().cloned()),
        CachedBody::Unary(_) => None,
    }
}

/// The unary counterpart: the single cached `ChatResponse`, or `None` if the body
/// was a stream (the caller must replay instead).
pub fn replay_unary(entry: &CachedResponse) -> Option<&gateway_llm::ChatResponse> {
    match &entry.body {
        CachedBody::Unary(resp) => Some(resp),
        CachedBody::Stream(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_llm::{ChatResponse, FinishReason};
    use gateway_spine::{TokenUsage, Usd};

    fn stream_entry() -> CachedResponse {
        CachedResponse {
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
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 2,
                ..Default::default()
            },
            original_cost: Usd::from_micros(100),
            stored_at_ms: 0,
            expires_at_ms: None,
        }
    }

    fn unary_entry() -> CachedResponse {
        CachedResponse {
            body: CachedBody::Unary(ChatResponse {
                model: "m".into(),
                content: vec![],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage::default(),
                provider_response_id: None,
            }),
            ..stream_entry()
        }
    }

    #[test]
    fn replays_deltas_in_order_with_terminal_usage() {
        let e = stream_entry();
        let deltas: Vec<StreamDelta> = replay_stream(&e).unwrap().collect();
        assert_eq!(deltas.len(), 3);
        // reconstructed text matches
        let text: String = deltas
            .iter()
            .filter_map(|d| d.content_delta.clone())
            .collect();
        assert_eq!(text, "Hello");
        // last delta carries finish + usage (never dropped on replay)
        let last = deltas.last().unwrap();
        assert!(last.finish_reason.is_some());
        assert_eq!(last.usage.unwrap().output_tokens, 2);
    }

    #[test]
    fn unary_body_has_no_stream_replay() {
        assert!(replay_stream(&unary_entry()).is_none());
        assert!(replay_unary(&unary_entry()).is_some());
    }

    #[test]
    fn stream_body_has_no_unary_replay() {
        assert!(replay_unary(&stream_entry()).is_none());
    }
}
