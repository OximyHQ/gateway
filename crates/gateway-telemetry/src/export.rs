//! Telemetry export adapters. DEFAULT IS OFF: a fresh gateway emits nothing to
//! any external system (no OTLP, no Oximy/ClickHouse) — the standalone posture
//! (design open-question #4). When enabled, the batch writer additionally hands
//! each drained row to the configured `Exporter`. The Oximy/ClickHouse adapter
//! is the first-party substrate seam (design §8.9) and is NEVER the default.

use crate::otel::{SpanAttr, span_attrs};
use crate::row::RequestLogRow;

/// Where, if anywhere, telemetry is exported. `Disabled` is the default.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum ExportConfig {
    /// No external export. The shipped default.
    #[default]
    Disabled,
    /// Standard OTLP endpoint for the GenAI spans.
    Otlp { endpoint: String },
    /// Oximy's OTEL/ClickHouse substrate (opt-in only).
    OximyClickHouse { endpoint: String, tenant_id: String },
}

impl ExportConfig {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, ExportConfig::Disabled)
    }
}

/// The export seam. Implementations receive each drained row (post capture
/// enforcement). `Disabled` builds a no-op exporter.
pub trait Exporter: Send + Sync {
    fn export(&self, row: &RequestLogRow);
}

/// No-op exporter — what `ExportConfig::Disabled` resolves to.
#[derive(Debug, Default)]
pub struct NoopExporter;

impl Exporter for NoopExporter {
    fn export(&self, _row: &RequestLogRow) {}
}

/// Build the exporter for a config. `Disabled` → `NoopExporter`. The live OTLP /
/// ClickHouse exporters are constructed here (feature-gated); when the `otel`
/// feature is off, both enabled variants fall back to noop with a warning so the
/// binary still runs.
pub fn build_exporter(config: &ExportConfig) -> Box<dyn Exporter> {
    match config {
        ExportConfig::Disabled => Box::new(NoopExporter),
        ExportConfig::Otlp { endpoint } => build_otlp(endpoint),
        ExportConfig::OximyClickHouse {
            endpoint,
            tenant_id,
        } => build_clickhouse(endpoint, tenant_id),
    }
}

#[cfg(feature = "otel")]
fn build_otlp(endpoint: &str) -> Box<dyn Exporter> {
    // The real pipeline (opentelemetry-otlp) is initialized in P1.8 boot; here we
    // keep a thin adapter that maps each row to GenAI attrs and forwards. The
    // span-pipeline handle is stored in the concrete exporter at boot.
    tracing::info!(endpoint, "OTLP export enabled");
    Box::new(OtlpExporter {
        _endpoint: endpoint.to_string(),
    })
}

#[cfg(not(feature = "otel"))]
fn build_otlp(endpoint: &str) -> Box<dyn Exporter> {
    tracing::warn!(
        endpoint,
        "OTLP export requested but `otel` feature is off; using noop"
    );
    Box::new(NoopExporter)
}

fn build_clickhouse(endpoint: &str, tenant_id: &str) -> Box<dyn Exporter> {
    tracing::info!(endpoint, tenant_id, "Oximy/ClickHouse export enabled");
    Box::new(ClickHouseExporter {
        _endpoint: endpoint.to_string(),
        _tenant_id: tenant_id.to_string(),
    })
}

#[cfg(feature = "otel")]
struct OtlpExporter {
    _endpoint: String,
}

#[cfg(feature = "otel")]
impl Exporter for OtlpExporter {
    fn export(&self, row: &RequestLogRow) {
        // Map to GenAI attrs; the concrete span pipeline (P1.8) emits them.
        let _attrs: Vec<SpanAttr> = span_attrs(row);
    }
}

struct ClickHouseExporter {
    _endpoint: String,
    _tenant_id: String,
}

impl Exporter for ClickHouseExporter {
    fn export(&self, row: &RequestLogRow) {
        // Batched insert into the Oximy substrate (P4 hardens this); attr shape
        // mirrors the GenAI span set for cross-system consistency.
        let _attrs: Vec<SpanAttr> = span_attrs(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        assert_eq!(ExportConfig::default(), ExportConfig::Disabled);
        assert!(!ExportConfig::default().is_enabled());
    }

    #[test]
    fn disabled_builds_noop_that_does_nothing() {
        let exporter = build_exporter(&ExportConfig::Disabled);
        // Calling export on a noop must be safe and a no-op (no panic).
        let row = sample_row();
        exporter.export(&row);
    }

    #[test]
    fn enabled_configs_report_enabled() {
        assert!(
            ExportConfig::Otlp {
                endpoint: "http://localhost:4317".into()
            }
            .is_enabled()
        );
        assert!(
            ExportConfig::OximyClickHouse {
                endpoint: "https://ch.oximy.com".into(),
                tenant_id: "t1".into(),
            }
            .is_enabled()
        );
    }

    #[test]
    fn config_roundtrips_json_with_target_tag() {
        let c = ExportConfig::OximyClickHouse {
            endpoint: "https://ch.oximy.com".into(),
            tenant_id: "t1".into(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"target\":\"oximy_click_house\""));
        let back: ExportConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    fn sample_row() -> RequestLogRow {
        use crate::row::{CacheStatus, CaptureMode, RequestKind};
        use gateway_spine::{TokenUsage, Usd};
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: "k1".into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage::default(),
            cost: Usd::ZERO,
            latency_ms: 1,
            ttft_ms: None,
            status: 200,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: CaptureMode::Metadata,
            request_text: None,
            response_text: None,
        }
    }
}
