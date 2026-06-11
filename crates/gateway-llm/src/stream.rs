//! The one internal streaming chunk. A provider SSE stream is mapped to a
//! sequence of `StreamDelta`s. Each delta carries ANY of: a text-content
//! fragment, a tool-call fragment, a finish reason, and/or usage. Usage usually
//! arrives only on the final delta — `usage: Option<TokenUsage>` makes that
//! explicit and (with the abort-safe decoder in Task 8/9) guarantees we NEVER
//! lose usage on aborted streams (invariant §2). Tool-call delta AGGREGATION
//! (stitching fragmented arguments) is DEFERRED to P1.3; here we faithfully
//! relay each fragment with its index.

use gateway_spine::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::resp::FinishReason;

/// A partial tool call as it streams in. `index` identifies which parallel call
/// this fragment belongs to; `id`/`name` appear on the first fragment, then
/// `arguments_delta` carries successive argument-string chunks. Reassembly is
/// P1.3's job — this type just preserves the pieces losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A chunk of the argument JSON string (concatenate across deltas to rebuild).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_delta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreamDelta {
    /// Incremental assistant text, if this chunk carried any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_delta: Option<String>,
    /// Incremental tool-call fragments, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_call_deltas: Vec<ToolCallDelta>,
    /// Set on the terminal chunk for this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Usage, typically only on the final chunk. NEVER dropped on abort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

impl StreamDelta {
    /// A pure text chunk.
    pub fn text(s: impl Into<String>) -> Self {
        StreamDelta {
            content_delta: Some(s.into()),
            ..Default::default()
        }
    }

    /// The terminal chunk carrying finish + usage.
    pub fn finish(reason: FinishReason, usage: TokenUsage) -> Self {
        StreamDelta {
            finish_reason: Some(reason),
            usage: Some(usage),
            ..Default::default()
        }
    }

    /// True if this delta carries no semantic payload (e.g. a keepalive).
    pub fn is_empty(&self) -> bool {
        self.content_delta.is_none()
            && self.tool_call_deltas.is_empty()
            && self.finish_reason.is_none()
            && self.usage.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_carries_only_content() {
        let d = StreamDelta::text("ab");
        assert_eq!(d.content_delta.as_deref(), Some("ab"));
        assert!(d.tool_call_deltas.is_empty());
        assert!(d.finish_reason.is_none());
        assert!(!d.is_empty());
    }

    #[test]
    fn finish_delta_carries_reason_and_usage() {
        let u = TokenUsage {
            input_tokens: 5,
            output_tokens: 3,
            ..Default::default()
        };
        let d = StreamDelta::finish(FinishReason::Stop, u);
        assert_eq!(d.finish_reason, Some(FinishReason::Stop));
        assert_eq!(d.usage.unwrap().total(), 8);
    }

    #[test]
    fn empty_delta_detected() {
        assert!(StreamDelta::default().is_empty());
    }

    #[test]
    fn tool_call_delta_roundtrips() {
        let d = StreamDelta {
            tool_call_deltas: vec![ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("get_weather".into()),
                arguments_delta: Some("{\"ci".into()),
            }],
            ..Default::default()
        };
        let back: StreamDelta = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}
