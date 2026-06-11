//! Structured-output translation. A unified `ResponseFormat` is compiled into a
//! per-provider `StructuredOutputPlan`. OpenAI/Gemini support `json_schema`
//! natively; Anthropic has no response-format field, so we EMULATE it with a
//! single forced tool call whose input schema IS the requested schema — the
//! model's structured answer arrives as that tool call's `arguments`, which we
//! unwrap back into content. `Text`/`JsonObject` map to native json-mode or pass
//! through. A provider that supports neither yields `Unsupported` (no silent
//! degradation). The plan is data only; transports/serializers consume it.

use serde_json::Value;

use crate::req::ResponseFormat;
use crate::toolcall::{ToolChoice, ToolDef};
use crate::translate::warn::IngressError;

/// The sentinel tool name used for Anthropic forced-tool structured-output
/// emulation. The response unwrapper keys off this exact name.
pub const EMULATION_TOOL_NAME: &str = "__oximy_structured_output";

/// Which provider family a plan is being compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFamily {
    OpenAi,
    Anthropic,
    Gemini,
}

/// How a transport should request structured output for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredOutputPlan {
    /// No structured-output constraint.
    None,
    /// Provider's native json-object mode (no schema).
    NativeJsonObject,
    /// Provider's native json-schema mode; carry the schema verbatim.
    NativeJsonSchema {
        name: String,
        schema: Value,
        strict: bool,
    },
    /// Emulate via a single forced tool call; unwrap the result from its args.
    ForcedToolEmulation {
        tool: ToolDef,
        tool_choice: ToolChoice,
    },
}

impl StructuredOutputPlan {
    /// Compile a response-format request for a provider family.
    pub fn compile(
        format: Option<&ResponseFormat>,
        family: ProviderFamily,
    ) -> Result<StructuredOutputPlan, IngressError> {
        match format {
            None | Some(ResponseFormat::Text) => Ok(StructuredOutputPlan::None),
            Some(ResponseFormat::JsonObject) => Ok(StructuredOutputPlan::NativeJsonObject),
            Some(ResponseFormat::JsonSchema {
                name,
                schema,
                strict,
            }) => match family {
                ProviderFamily::OpenAi | ProviderFamily::Gemini => {
                    Ok(StructuredOutputPlan::NativeJsonSchema {
                        name: name.clone(),
                        schema: schema.clone(),
                        strict: *strict,
                    })
                }
                ProviderFamily::Anthropic => Ok(StructuredOutputPlan::ForcedToolEmulation {
                    tool: ToolDef {
                        name: EMULATION_TOOL_NAME.to_string(),
                        description: Some(format!(
                            "Respond ONLY by calling this function to produce `{name}`."
                        )),
                        parameters: schema.clone(),
                    },
                    tool_choice: ToolChoice::Function {
                        name: EMULATION_TOOL_NAME.to_string(),
                    },
                }),
            },
        }
    }

    /// For an emulated plan: given a finished tool call's name+args, unwrap the
    /// structured payload back to the content string. Returns `None` if this call
    /// is not the emulation tool (so a normal tool call passes through untouched).
    pub fn unwrap_emulated<'a>(call_name: &str, arguments: &'a str) -> Option<&'a str> {
        if call_name == EMULATION_TOOL_NAME {
            Some(arguments)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    #[test]
    fn text_and_none_compile_to_none() {
        assert_eq!(
            StructuredOutputPlan::compile(None, ProviderFamily::OpenAi).unwrap(),
            StructuredOutputPlan::None
        );
        assert_eq!(
            StructuredOutputPlan::compile(Some(&ResponseFormat::Text), ProviderFamily::Anthropic)
                .unwrap(),
            StructuredOutputPlan::None
        );
    }

    #[test]
    fn openai_json_schema_is_native() {
        let fmt = ResponseFormat::JsonSchema {
            name: "out".into(),
            schema: schema(),
            strict: true,
        };
        let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::OpenAi).unwrap();
        assert!(matches!(
            plan,
            StructuredOutputPlan::NativeJsonSchema { strict: true, .. }
        ));
    }

    #[test]
    fn anthropic_json_schema_emulates_with_forced_tool() {
        let fmt = ResponseFormat::JsonSchema {
            name: "out".into(),
            schema: schema(),
            strict: true,
        };
        let plan = StructuredOutputPlan::compile(Some(&fmt), ProviderFamily::Anthropic).unwrap();
        match plan {
            StructuredOutputPlan::ForcedToolEmulation { tool, tool_choice } => {
                assert_eq!(tool.name, EMULATION_TOOL_NAME);
                assert_eq!(tool.parameters, schema());
                assert_eq!(
                    tool_choice,
                    ToolChoice::Function {
                        name: EMULATION_TOOL_NAME.into()
                    }
                );
            }
            other => panic!("expected ForcedToolEmulation, got {other:?}"),
        }
    }

    #[test]
    fn unwrap_emulated_only_matches_sentinel_tool() {
        assert_eq!(
            StructuredOutputPlan::unwrap_emulated(EMULATION_TOOL_NAME, "{\"x\":1}"),
            Some("{\"x\":1}")
        );
        assert_eq!(
            StructuredOutputPlan::unwrap_emulated("get_weather", "{}"),
            None
        );
    }
}
