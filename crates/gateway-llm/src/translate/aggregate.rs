//! Tool-call delta aggregation. A streamed turn emits `ToolCallDelta`s carrying an
//! `index` (which parallel call), an `id`/`name` (on the first fragment for that
//! index), then successive `arguments_delta` string fragments. This aggregator
//! folds them per-index into whole `ToolCall`s whose `arguments` is the
//! byte-concatenation of the fragments — the single point where fragmented JSON is
//! stitched. It also folds whole (non-fragmented) calls, so it works for providers
//! that emit complete calls in one delta. Ordering by index is stable; first-seen
//! `id`/`name` win (later fragments only carry args).

use std::collections::BTreeMap;

use crate::stream::{StreamDelta, ToolCallDelta};
use crate::toolcall::ToolCall;

#[derive(Debug, Default, Clone)]
struct Partial {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// Stateful accumulator. Feed every `StreamDelta`; call `finish()` once the stream
/// terminates to get the completed tool calls in stable index order.
#[derive(Debug, Default)]
pub struct ToolCallAggregator {
    by_index: BTreeMap<i64, Partial>,
}

impl ToolCallAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one delta's tool-call fragments. Text/finish/usage are ignored here —
    /// the caller handles those; this owns ONLY tool-call reassembly.
    pub fn push_delta(&mut self, delta: &StreamDelta) {
        for frag in &delta.tool_call_deltas {
            self.push_fragment(frag);
        }
    }

    fn push_fragment(&mut self, frag: &ToolCallDelta) {
        let entry = self.by_index.entry(frag.index).or_default();
        if entry.id.is_none()
            && let Some(id) = &frag.id
        {
            entry.id = Some(id.clone());
        }
        if entry.name.is_none()
            && let Some(name) = &frag.name
        {
            entry.name = Some(name.clone());
        }
        if let Some(args) = &frag.arguments_delta {
            entry.arguments.push_str(args);
        }
    }

    /// True if any tool-call fragment has been seen.
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }

    /// Finalize into completed calls, in ascending index order. A call with no id
    /// is given an empty id (provider didn't supply one); a call with no name is
    /// dropped (a nameless call is not invocable — better to omit than fabricate).
    pub fn finish(self) -> Vec<ToolCall> {
        self.by_index
            .into_values()
            .filter_map(|p| {
                let name = p.name?;
                Some(ToolCall {
                    id: p.id.unwrap_or_default(),
                    name,
                    arguments: if p.arguments.is_empty() {
                        "{}".to_string()
                    } else {
                        p.arguments
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(index: i64, id: Option<&str>, name: Option<&str>, args: Option<&str>) -> ToolCallDelta {
        ToolCallDelta {
            index,
            id: id.map(Into::into),
            name: name.map(Into::into),
            arguments_delta: args.map(Into::into),
        }
    }

    fn delta(frags: Vec<ToolCallDelta>) -> StreamDelta {
        StreamDelta {
            tool_call_deltas: frags,
            ..Default::default()
        }
    }

    #[test]
    fn stitches_fragmented_arguments_byte_exact() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(
            0,
            Some("call_1"),
            Some("get_weather"),
            Some("{\"ci"),
        )]));
        agg.push_delta(&delta(vec![frag(0, None, None, Some("ty\":\""))]));
        agg.push_delta(&delta(vec![frag(0, None, None, Some("SF\"}"))]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].arguments, "{\"city\":\"SF\"}");
    }

    #[test]
    fn handles_parallel_calls_by_index_in_order() {
        let mut agg = ToolCallAggregator::new();
        // Interleaved fragments for two parallel calls.
        agg.push_delta(&delta(vec![
            frag(0, Some("c0"), Some("f0"), Some("{\"a\":")),
            frag(1, Some("c1"), Some("f1"), Some("{\"b\":")),
        ]));
        agg.push_delta(&delta(vec![
            frag(1, None, None, Some("2}")),
            frag(0, None, None, Some("1}")),
        ]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "f0"); // index 0 first
        assert_eq!(calls[0].arguments, "{\"a\":1}");
        assert_eq!(calls[1].name, "f1");
        assert_eq!(calls[1].arguments, "{\"b\":2}");
    }

    #[test]
    fn whole_call_in_one_delta_works() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(
            0,
            Some("c"),
            Some("f"),
            Some("{\"x\":1}"),
        )]));
        let calls = agg.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments, "{\"x\":1}");
    }

    #[test]
    fn empty_arguments_default_to_object() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("c"), Some("f"), None)]));
        let calls = agg.finish();
        assert_eq!(calls[0].arguments, "{}");
    }

    #[test]
    fn nameless_fragment_is_dropped_not_fabricated() {
        let mut agg = ToolCallAggregator::new();
        agg.push_delta(&delta(vec![frag(0, Some("c"), None, Some("{\"x\":1}"))]));
        assert!(agg.finish().is_empty());
    }
}
