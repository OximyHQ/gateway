//! Structured-output translation conformance. For an Anthropic-family request, a
//! `json_schema` response-format compiles to a forced single-tool call whose input
//! schema IS the requested schema; the model's tool-call answer unwraps back to the
//! structured payload. For OpenAI/Gemini families it stays native. No silent
//! degradation: an unsupported combination would surface as an error (none here).

use gateway_llm::req::ResponseFormat;
use gateway_llm::translate::structured::{
    EMULATION_TOOL_NAME, ProviderFamily, StructuredOutputPlan,
};
use serde_json::json;

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": { "answer": { "type": "string" }, "confidence": { "type": "number" } },
        "required": ["answer"]
    })
}

#[test]
fn anthropic_structured_output_emulates_and_unwraps() {
    let fmt = ResponseFormat::JsonSchema {
        name: "result".into(),
        schema: schema(),
        strict: true,
    };
    let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::Anthropic).unwrap();

    // Compiles to a forced tool whose parameters are the requested schema.
    let tool = match &plan {
        StructuredOutputPlan::ForcedToolEmulation { tool, .. } => tool,
        other => panic!("expected emulation, got {other:?}"),
    };
    assert_eq!(tool.name, EMULATION_TOOL_NAME);
    assert_eq!(tool.parameters, schema());

    // The model answers by calling the emulation tool; we unwrap its args.
    let model_args = "{\"answer\":\"42\",\"confidence\":0.9}";
    let unwrapped = StructuredOutputPlan::unwrap_emulated(EMULATION_TOOL_NAME, model_args)
        .expect("emulation tool call unwraps to structured content");
    let parsed: serde_json::Value = serde_json::from_str(unwrapped).unwrap();
    assert_eq!(parsed["answer"], "42");

    // A normal (non-emulation) tool call passes through untouched.
    assert!(StructuredOutputPlan::unwrap_emulated("some_other_tool", "{}").is_none());
}

#[test]
fn openai_and_gemini_structured_output_stay_native() {
    let fmt = ResponseFormat::JsonSchema {
        name: "r".into(),
        schema: schema(),
        strict: false,
    };
    for family in [ProviderFamily::OpenAi, ProviderFamily::Gemini] {
        let plan = StructuredOutputPlan::compile(Some(&fmt), family).unwrap();
        assert!(
            matches!(plan, StructuredOutputPlan::NativeJsonSchema { .. }),
            "{family:?}"
        );
    }
}
