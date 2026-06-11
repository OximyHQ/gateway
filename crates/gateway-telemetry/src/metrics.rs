//! Typed Prometheus registry. The batch writer folds each drained row into these
//! counters; the `/metrics` handler (Task 7) renders the OpenMetrics text. Cost
//! is exposed as a µUSD counter (integer-faithful — no float drift across a
//! scrape series). Per-label cardinality is intentionally limited to
//! key/model/status so a noisy tag set can't explode the series count.

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::row::RequestLogRow;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RequestLabels {
    pub key_id: String,
    pub model: String,
    pub status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct KeyModelLabels {
    pub key_id: String,
    pub model: String,
}

pub struct GatewayMetrics {
    registry: Registry,
    requests_total: Family<RequestLabels, Counter>,
    cost_micros_total: Family<KeyModelLabels, Counter>,
    input_tokens_total: Family<KeyModelLabels, Counter>,
    output_tokens_total: Family<KeyModelLabels, Counter>,
    dropped_rows_total: Counter,
    latency_ms: Family<KeyModelLabels, Histogram>,
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayMetrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        let requests_total = Family::<RequestLabels, Counter>::default();
        let cost_micros_total = Family::<KeyModelLabels, Counter>::default();
        let input_tokens_total = Family::<KeyModelLabels, Counter>::default();
        let output_tokens_total = Family::<KeyModelLabels, Counter>::default();
        let dropped_rows_total = Counter::default();
        let latency_ms = Family::<KeyModelLabels, Histogram>::new_with_constructor(|| {
            Histogram::new([5.0, 25.0, 100.0, 250.0, 1000.0, 5000.0, 30000.0].into_iter())
        });

        registry.register(
            "gateway_requests",
            "Total governed requests",
            requests_total.clone(),
        );
        registry.register(
            "gateway_cost_micros",
            "Total cost in µUSD",
            cost_micros_total.clone(),
        );
        registry.register(
            "gateway_input_tokens",
            "Total input tokens",
            input_tokens_total.clone(),
        );
        registry.register(
            "gateway_output_tokens",
            "Total output tokens",
            output_tokens_total.clone(),
        );
        registry.register(
            "gateway_dropped_rows",
            "Telemetry rows dropped due to a full channel",
            dropped_rows_total.clone(),
        );
        registry.register(
            "gateway_request_latency_ms",
            "Request latency (ms)",
            latency_ms.clone(),
        );

        Self {
            registry,
            requests_total,
            cost_micros_total,
            input_tokens_total,
            output_tokens_total,
            dropped_rows_total,
            latency_ms,
        }
    }

    /// Fold one drained row into the live counters.
    pub fn record(&self, row: &RequestLogRow) {
        let req = RequestLabels {
            key_id: row.key_id.clone(),
            model: row.model.clone(),
            status: row.status.to_string(),
        };
        let km = KeyModelLabels {
            key_id: row.key_id.clone(),
            model: row.model.clone(),
        };
        self.requests_total.get_or_create(&req).inc();
        // Cost is non-negative µUSD; a Counter only goes up — exact integer.
        self.cost_micros_total
            .get_or_create(&km)
            .inc_by(row.cost.micros().max(0) as u64);
        self.input_tokens_total
            .get_or_create(&km)
            .inc_by(row.usage.input_tokens.max(0) as u64);
        self.output_tokens_total
            .get_or_create(&km)
            .inc_by(row.usage.output_tokens.max(0) as u64);
        self.latency_ms
            .get_or_create(&km)
            .observe(row.latency_ms as f64);
    }

    pub fn record_dropped(&self) {
        self.dropped_rows_total.inc();
    }

    /// Render the OpenMetrics text body for `/metrics`.
    pub fn render(&self) -> String {
        let mut buf = String::new();
        encode(&mut buf, &self.registry).expect("encode never fails into a String");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{CacheStatus, CaptureMode, RequestKind};
    use gateway_spine::{TokenUsage, Usd};

    fn row(key: &str, model: &str, cost: i64, status: u16) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: model.into(),
            provider: "openai".into(),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            cost: Usd::from_micros(cost),
            latency_ms: 120,
            ttft_ms: Some(40),
            status,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: CaptureMode::Metadata,
            request_text: None,
            response_text: None,
        }
    }

    #[test]
    fn render_includes_registered_series() {
        let m = GatewayMetrics::new();
        m.record(&row("k1", "gpt-4o", 7_500, 200));
        let text = m.render();
        assert!(text.contains("gateway_requests_total"));
        assert!(text.contains("gateway_cost_micros_total"));
        assert!(text.contains("key_id=\"k1\""));
        assert!(text.contains("model=\"gpt-4o\""));
    }

    #[test]
    fn cost_counter_accumulates_exact_micros() {
        let m = GatewayMetrics::new();
        m.record(&row("k1", "gpt-4o", 7_500, 200));
        m.record(&row("k1", "gpt-4o", 2_500, 200));
        let text = m.render();
        // 7_500 + 2_500 = 10_000 µUSD on the same label set
        assert!(text.contains("gateway_cost_micros_total{key_id=\"k1\",model=\"gpt-4o\"} 10000"));
    }

    #[test]
    fn dropped_rows_counter_is_exposed() {
        let m = GatewayMetrics::new();
        m.record_dropped();
        m.record_dropped();
        let text = m.render();
        assert!(text.contains("gateway_dropped_rows_total 2"));
    }
}
