//! Tool/function definitions (what the model MAY call) and tool calls (what it
//! DID call). `ToolDef.parameters` is a raw JSON-Schema `Value` carried verbatim
//! across dialects. `ToolCall.arguments` is the model-produced argument JSON as a
//! STRING (providers emit it incrementally as a string; we keep it unparsed here
//! and let P1.3 own delta aggregation + schema validation).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A function the model is allowed to call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the function's arguments, carried verbatim.
    pub parameters: Value,
}

/// How the caller constrains tool selection for one request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether/which tool to call (provider default).
    #[default]
    Auto,
    /// Model must not call a tool.
    None,
    /// Model must call at least one tool.
    Required,
    /// Model must call exactly this tool.
    Function { name: String },
}

/// A concrete tool invocation the model emitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call id (echoed back on the matching tool result).
    pub id: String,
    pub name: String,
    /// Raw arguments JSON as a string (NOT yet parsed — see module note).
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_def_carries_schema_verbatim() {
        let schema = json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
        });
        let t = ToolDef {
            name: "get_weather".into(),
            description: Some("Look up weather".into()),
            parameters: schema.clone(),
        };
        assert_eq!(t.parameters, schema);
        let back: ToolDef = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn tool_choice_default_is_auto() {
        assert_eq!(ToolChoice::default(), ToolChoice::Auto);
    }

    #[test]
    fn tool_choice_function_roundtrips() {
        let c = ToolChoice::Function {
            name: "get_weather".into(),
        };
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["mode"], "function");
        assert_eq!(j["name"], "get_weather");
        let back: ToolChoice = serde_json::from_value(j).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn tool_call_keeps_arguments_as_string() {
        let c = ToolCall {
            id: "call_1".into(),
            name: "get_weather".into(),
            arguments: "{\"city\":\"SF\"}".into(),
        };
        assert_eq!(c.arguments, "{\"city\":\"SF\"}");
    }
}
