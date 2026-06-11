//! OTel GenAI semantic-convention span emit. The attribute MAPPING (row →
//! `gen_ai.*` key/values) is always compiled and tested; only the OTLP exporter
//! pipeline init is behind the `otel` feature. We emit a span per request named
//! `gen_ai.{operation}` with the conventional model/provider/usage attributes;
//! content text is attached ONLY when the row's capture mode kept it (the same
//! fail-safe privacy floor as storage).

use crate::row::{CaptureMode, RequestLogRow};

/// One OTel attribute as a typed key/value. Kept dependency-light so the mapping
/// is testable without pulling the OTel SDK into the unit test.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanAttr {
    pub key: &'static str,
    pub value: AttrValue,
}

/// Build the GenAI-semconv attribute set for a request row. Stable keys per the
/// OpenTelemetry GenAI conventions (`gen_ai.system`, `gen_ai.request.model`,
/// `gen_ai.usage.input_tokens`, etc.) plus gateway-specific `oximy.*` extras.
pub fn span_attrs(row: &RequestLogRow) -> Vec<SpanAttr> {
    let mut attrs = vec![
        SpanAttr {
            key: "gen_ai.system",
            value: AttrValue::Str(row.provider.clone()),
        },
        SpanAttr {
            key: "gen_ai.request.model",
            value: AttrValue::Str(row.model.clone()),
        },
        SpanAttr {
            key: "gen_ai.usage.input_tokens",
            value: AttrValue::Int(row.usage.input_tokens),
        },
        SpanAttr {
            key: "gen_ai.usage.output_tokens",
            value: AttrValue::Int(row.usage.output_tokens),
        },
        SpanAttr {
            key: "oximy.cost_micros",
            value: AttrValue::Int(row.cost.micros()),
        },
        SpanAttr {
            key: "oximy.key_id",
            value: AttrValue::Str(row.key_id.clone()),
        },
        SpanAttr {
            key: "oximy.served_by",
            value: AttrValue::Str(row.served_by.clone()),
        },
        SpanAttr {
            key: "oximy.fallback_fired",
            value: AttrValue::Bool(row.fallback_fired),
        },
        SpanAttr {
            key: "http.response.status_code",
            value: AttrValue::Int(row.status as i64),
        },
    ];
    if let Some(ttft) = row.ttft_ms {
        attrs.push(SpanAttr {
            key: "oximy.ttft_ms",
            value: AttrValue::Int(ttft),
        });
    }
    // Content is attached only under the same privacy floor as storage.
    if row.capture_mode == CaptureMode::Full {
        if let Some(text) = &row.request_text {
            attrs.push(SpanAttr {
                key: "gen_ai.prompt",
                value: AttrValue::Str(text.clone()),
            });
        }
        if let Some(text) = &row.response_text {
            attrs.push(SpanAttr {
                key: "gen_ai.completion",
                value: AttrValue::Str(text.clone()),
            });
        }
    }
    attrs
}

/// The conventional span name for an LLM chat request.
pub fn span_name(row: &RequestLogRow) -> String {
    format!("gen_ai.chat {}", row.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{CacheStatus, RequestKind};
    use gateway_spine::{TokenUsage, Usd};

    fn row(mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: "k1".into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            cost: Usd::from_micros(7_500),
            latency_ms: 800,
            ttft_ms: Some(120),
            status: 200,
            served_by: "openai/gpt-4o".into(),
            fallback_fired: true,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("hello".into()),
            response_text: Some("hi".into()),
        }
    }

    fn find<'a>(attrs: &'a [SpanAttr], key: &str) -> Option<&'a AttrValue> {
        attrs.iter().find(|a| a.key == key).map(|a| &a.value)
    }

    #[test]
    fn maps_core_genai_attributes() {
        let attrs = span_attrs(&row(CaptureMode::Metadata));
        assert_eq!(
            find(&attrs, "gen_ai.system"),
            Some(&AttrValue::Str("openai".into()))
        );
        assert_eq!(
            find(&attrs, "gen_ai.request.model"),
            Some(&AttrValue::Str("gpt-4o".into()))
        );
        assert_eq!(
            find(&attrs, "gen_ai.usage.input_tokens"),
            Some(&AttrValue::Int(100))
        );
        assert_eq!(
            find(&attrs, "oximy.cost_micros"),
            Some(&AttrValue::Int(7_500))
        );
        assert_eq!(
            find(&attrs, "oximy.fallback_fired"),
            Some(&AttrValue::Bool(true))
        );
        assert_eq!(find(&attrs, "oximy.ttft_ms"), Some(&AttrValue::Int(120)));
    }

    #[test]
    fn metadata_mode_omits_content() {
        let attrs = span_attrs(&row(CaptureMode::Metadata));
        assert!(find(&attrs, "gen_ai.prompt").is_none());
        assert!(find(&attrs, "gen_ai.completion").is_none());
    }

    #[test]
    fn full_mode_includes_content() {
        let attrs = span_attrs(&row(CaptureMode::Full));
        assert_eq!(
            find(&attrs, "gen_ai.prompt"),
            Some(&AttrValue::Str("hello".into()))
        );
        assert_eq!(
            find(&attrs, "gen_ai.completion"),
            Some(&AttrValue::Str("hi".into()))
        );
    }

    #[test]
    fn span_name_includes_model() {
        assert_eq!(span_name(&row(CaptureMode::Metadata)), "gen_ai.chat gpt-4o");
    }
}
