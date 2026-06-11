//! JSON-RPC 2.0 types — the wire layer for MCP.
//!
//! The spec says: requests carry `id` + `method` + optional `params`;
//! notifications carry `method` + optional `params` but NO `id`;
//! responses carry `id` + `result` XOR `error`.
//!
//! All three share the `"jsonrpc": "2.0"` version discriminant.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Marker constant embedded in every JSON-RPC 2.0 frame.
pub const JSONRPC_VERSION: &str = "2.0";

// ─── id ─────────────────────────────────────────────────────────────────────

/// A JSON-RPC request id: `null`, number, or string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Null,
    Num(i64),
    Str(String),
}

// ─── Request ────────────────────────────────────────────────────────────────

/// An outgoing or incoming JSON-RPC 2.0 request (has an `id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            method: method.into(),
            params,
        }
    }
}

// ─── Notification ────────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 notification (no `id`; no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            method: method.into(),
            params,
        }
    }
}

// ─── Response ───────────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("JSON-RPC error {code}: {message}")]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    // Standard JSON-RPC error codes.
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    pub fn parse_error() -> Self {
        Self::new(Self::PARSE_ERROR, "Parse error")
    }
    pub fn invalid_request() -> Self {
        Self::new(Self::INVALID_REQUEST, "Invalid Request")
    }
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            Self::METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )
    }
    pub fn invalid_params(detail: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, detail)
    }
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(Self::INTERNAL_ERROR, detail)
    }
}

/// A JSON-RPC 2.0 response: either a `result` or an `error`, never both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: RequestId, err: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(err),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

// ─── Incoming frame discriminator ────────────────────────────────────────────

/// Parses a raw JSON value into one of the three JSON-RPC frame kinds.
#[derive(Debug, Clone)]
pub enum RpcFrame {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
}

impl RpcFrame {
    /// Attempt to classify a raw [`Value`].
    /// Returns `Err` only if the outer envelope is malformed (not 2.0, etc.).
    pub fn from_value(v: Value) -> Result<Self, JsonRpcError> {
        let version = v.get("jsonrpc").and_then(Value::as_str);
        if version != Some(JSONRPC_VERSION) {
            return Err(JsonRpcError::invalid_request());
        }
        // Has "result" or "error" → response.
        if v.get("result").is_some() || v.get("error").is_some() {
            let resp: JsonRpcResponse =
                serde_json::from_value(v).map_err(|_| JsonRpcError::invalid_request())?;
            return Ok(RpcFrame::Response(resp));
        }
        // Has "id" → request; no "id" → notification.
        if v.get("id").is_some() {
            let req: JsonRpcRequest =
                serde_json::from_value(v).map_err(|_| JsonRpcError::invalid_request())?;
            Ok(RpcFrame::Request(req))
        } else {
            let notif: JsonRpcNotification =
                serde_json::from_value(v).map_err(|_| JsonRpcError::invalid_request())?;
            Ok(RpcFrame::Notification(notif))
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trips() {
        let req = JsonRpcRequest::new(
            RequestId::Num(1),
            "tools/list",
            Some(json!({"cursor": null})),
        );
        let s = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.method, "tools/list");
        assert!(matches!(parsed.id, RequestId::Num(1)));
    }

    #[test]
    fn notification_has_no_id() {
        let n = JsonRpcNotification::new("notifications/initialized", None);
        let v = serde_json::to_value(&n).unwrap();
        assert!(v.get("id").is_none());
        assert_eq!(v["method"], "notifications/initialized");
    }

    #[test]
    fn response_success_round_trips() {
        let resp = JsonRpcResponse::success(RequestId::Str("abc".into()), json!({"tools": []}));
        assert!(resp.is_ok());
        let s = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&s).unwrap();
        assert!(matches!(parsed.id, RequestId::Str(_)));
    }

    #[test]
    fn response_error_serializes_cleanly() {
        let resp =
            JsonRpcResponse::error(RequestId::Num(2), JsonRpcError::method_not_found("foo/bar"));
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("result").is_none());
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn frame_classifier_works() {
        // Request.
        let v = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
        assert!(matches!(RpcFrame::from_value(v), Ok(RpcFrame::Request(_))));

        // Notification.
        let v = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(matches!(
            RpcFrame::from_value(v),
            Ok(RpcFrame::Notification(_))
        ));

        // Response (result).
        let v = json!({"jsonrpc":"2.0","id":1,"result":{}});
        assert!(matches!(RpcFrame::from_value(v), Ok(RpcFrame::Response(_))));

        // Response (error).
        let v = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}});
        assert!(matches!(RpcFrame::from_value(v), Ok(RpcFrame::Response(_))));

        // Bad version.
        let v = json!({"jsonrpc":"1.0","id":1,"method":"x"});
        assert!(RpcFrame::from_value(v).is_err());
    }
}
