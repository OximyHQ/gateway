//! MCP core message types — initialize, tools/list, tools/call.
//!
//! Covers the **2025-11-25** stable spec. Resources and prompts are stubbed
//! (they share the same JSON-RPC envelope but are deferred for now).

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Implementation info ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Implementation {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Implementation {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
        }
    }

    pub fn gateway() -> Self {
        Self::new("oximy-gateway", env!("CARGO_PKG_VERSION"))
    }
}

// ─── Capabilities ─────────────────────────────────────────────────────────────

/// Server capabilities advertised in `InitializeResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourcesCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Client capabilities sent in `InitializeRequest`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Value>,
}

// ─── Initialize ──────────────────────────────────────────────────────────────

/// Params for the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: Implementation,
}

/// Result returned by the server for `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl InitializeResult {
    /// Convenience constructor for the gateway's own response.
    pub fn gateway_response() -> Self {
        Self {
            protocol_version: crate::MCP_PROTOCOL_VERSION.into(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
                ..Default::default()
            },
            server_info: Implementation::gateway(),
            instructions: None,
        }
    }
}

// ─── Tool ────────────────────────────────────────────────────────────────────

/// A tool as advertised by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool's input (`object` type).
    pub input_schema: Value,
    /// Optional output schema (structured tool output, 2025-11-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// Hints: readOnlyHint, destructiveHint, idempotentHint, openWorldHint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

// ─── tools/list ──────────────────────────────────────────────────────────────

/// Params for `tools/list` (cursor is optional).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Result of `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsListResult {
    pub tools: Vec<Tool>,
    /// Present only when there are more pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

// ─── tools/call ──────────────────────────────────────────────────────────────

/// Params for `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// Content item inside a tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: Value },
}

impl ToolContent {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    pub content: Vec<ToolContent>,
    /// `true` if the tool itself reported an error (not a protocol error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Structured output matching `outputSchema`, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

impl ToolCallResult {
    pub fn ok(content: Vec<ToolContent>) -> Self {
        Self {
            content,
            is_error: None,
            structured_content: None,
        }
    }

    pub fn err_result(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(message)],
            is_error: Some(true),
            structured_content: None,
        }
    }

    pub fn is_error(&self) -> bool {
        self.is_error.unwrap_or(false)
    }
}

// ─── Stubs: resources / prompts ──────────────────────────────────────────────
//
// These are real message types in the spec.  We define minimal round-trip-safe
// structs now so the federation can relay them without panicking; full
// semantics are a later deliverable.

/// Minimal resource descriptor (stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Minimal prompt descriptor (stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool() -> Tool {
        Tool {
            name: "echo".into(),
            description: Some("Echoes text".into()),
            input_schema: json!({"type":"object","properties":{"text":{"type":"string"}}}),
            output_schema: None,
            annotations: None,
        }
    }

    #[test]
    fn tool_round_trips() {
        let t = sample_tool();
        let s = serde_json::to_string(&t).unwrap();
        let t2: Tool = serde_json::from_str(&s).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn initialize_result_round_trips() {
        let r = InitializeResult::gateway_response();
        let s = serde_json::to_string(&r).unwrap();
        let r2: InitializeResult = serde_json::from_str(&s).unwrap();
        assert_eq!(r.protocol_version, r2.protocol_version);
        assert_eq!(r.server_info.name, r2.server_info.name);
    }

    #[test]
    fn tool_call_result_error_flag() {
        let ok = ToolCallResult::ok(vec![ToolContent::text("hello")]);
        assert!(!ok.is_error());

        let err = ToolCallResult::err_result("boom");
        assert!(err.is_error());
        assert!(matches!(&err.content[0], ToolContent::Text { text } if text == "boom"));
    }

    #[test]
    fn tools_list_result_cursor_optional() {
        let r = ToolsListResult {
            tools: vec![sample_tool()],
            next_cursor: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("nextCursor").is_none()); // skip_serializing_if
    }
}
