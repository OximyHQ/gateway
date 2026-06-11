//! The authenticated MCP gateway endpoint: `POST /mcp`.
//!
//! Speaks JSON-RPC 2.0 to the [`gateway_mcp::Federation`] held in [`AppState`].
//! Bearer auth is enforced exactly like `/v1/*` (an unauthenticated request gets
//! 401 before any body parsing). Supported methods:
//!   - `initialize` — the gateway's own server handshake.
//!   - `tools/list` — federated tools, ACL-filtered by the caller's key.
//!   - `tools/call` — dispatched to the owning upstream server; audited on the
//!     spine via the federation's shared `AuditSink`.
//!   - `notifications/*` — accepted (202, no body) per the JSON-RPC notification
//!     contract (no response is sent for a frame without an `id`).
//!
//! Tool-call dollar-metering against the budget ledger is deferred — see the
//! TODO in [`dispatch`].

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use gateway_mcp::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, RequestId, RpcFrame};
use gateway_spine::{Clock, VirtualKey};

use crate::auth::authenticate;
use crate::state::AppState;

/// `POST /mcp` — the JSON-RPC 2.0 MCP gateway. Authenticated by bearer token.
pub async fn mcp_handler<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    raw: axum::body::Bytes,
) -> Response {
    // Auth-by-default: 401 before we look at the body at all.
    let key = match authenticate(
        state.keys.as_ref(),
        state.clock.as_ref(),
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
    ) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };

    // Parse the JSON envelope. A malformed body is a JSON-RPC parse error (200
    // carrying an error object with a null id — the spec has no request id yet).
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => {
            return json_rpc(JsonRpcResponse::error(
                RequestId::Null,
                JsonRpcError::parse_error(),
            ));
        }
    };

    match RpcFrame::from_value(value) {
        Ok(RpcFrame::Request(req)) => json_rpc(dispatch(&state, &key, req).await),
        // Notifications carry no id and expect no response — ack with 202.
        Ok(RpcFrame::Notification(_)) => StatusCode::ACCEPTED.into_response(),
        // A response frame to our server endpoint is a protocol misuse.
        Ok(RpcFrame::Response(_)) => json_rpc(JsonRpcResponse::error(
            RequestId::Null,
            JsonRpcError::invalid_request(),
        )),
        Err(err) => json_rpc(JsonRpcResponse::error(RequestId::Null, err)),
    }
}

/// Dispatch one JSON-RPC request method against the federation.
async fn dispatch<C: Clock + 'static>(
    state: &AppState<C>,
    key: &VirtualKey,
    req: JsonRpcRequest,
) -> JsonRpcResponse {
    let id = req.id.clone();
    match req.method.as_str() {
        "initialize" => {
            let result = gateway_mcp::InitializeResult::gateway_response();
            match serde_json::to_value(&result) {
                Ok(v) => JsonRpcResponse::success(id, v),
                Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal(e.to_string())),
            }
        }
        "tools/list" => {
            // ACL-filter by the caller's key id.
            let result = state.federation.list_tools(Some(&key.id)).await;
            match serde_json::to_value(&result) {
                Ok(v) => JsonRpcResponse::success(id, v),
                Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal(e.to_string())),
            }
        }
        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let name = match params.get("name").and_then(Value::as_str) {
                Some(n) => n.to_string(),
                None => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params("tools/call: missing 'name'"),
                    );
                }
            };
            let arguments = params.get("arguments").cloned();

            // TODO(metering): meter the tool call against the caller's budget
            // ledger once a per-tool USD price model exists. The audit event is
            // already emitted by Federation::call_tool on the shared spine.
            match state
                .federation
                .call_tool(&name, arguments, Some(&key.id))
                .await
            {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => JsonRpcResponse::success(id, v),
                    Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal(e.to_string())),
                },
                Err(e) => JsonRpcResponse::error(id, e.to_jsonrpc_error()),
            }
        }
        other => JsonRpcResponse::error(id, JsonRpcError::method_not_found(other)),
    }
}

/// Serialize a JSON-RPC response as a 200 `application/json` body.
fn json_rpc(resp: JsonRpcResponse) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize failure"}}"#
                .to_string()
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;
    use async_trait::async_trait;
    use axum::body::{Body, to_bytes};
    use gateway_mcp::{McpServer, McpTransport};
    use gateway_spine::{MockClock, RateLimits, Usd};
    use http::Request;
    use serde_json::json;
    use std::collections::HashSet;
    use tower::ServiceExt;

    /// An in-process MCP transport that serves two tools and echoes calls.
    struct TestTransport;

    #[async_trait]
    impl McpTransport for TestTransport {
        fn label(&self) -> &str {
            "test"
        }
        async fn call(
            &self,
            req: gateway_mcp::JsonRpcRequest,
        ) -> Result<gateway_mcp::JsonRpcResponse, gateway_mcp::McpError> {
            let result = match req.method.as_str() {
                "tools/list" => json!({
                    "tools": [
                        {"name":"echo","description":"echo back","inputSchema":{"type":"object"}},
                        {"name":"danger","description":"do danger","inputSchema":{"type":"object"}}
                    ]
                }),
                "tools/call" => {
                    let name = req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string();
                    json!({"content":[{"type":"text","text":format!("called:{name}")}]})
                }
                _ => json!({}),
            };
            Ok(gateway_mcp::JsonRpcResponse::success(req.id, result))
        }
    }

    async fn test_state() -> Arc<AppState<MockClock>> {
        test_state_with_audit().await.0
    }

    /// Build a test state and return it alongside the concrete `MemoryAudit`
    /// handle (the federation shares this same sink) so tests can assert events.
    async fn test_state_with_audit() -> (Arc<AppState<MockClock>>, Arc<gateway_spine::MemoryAudit>)
    {
        let mut ks = StaticKeyStore::new();
        ks.insert(VirtualKey {
            id: "key_mcp".into(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        });
        let audit = Arc::new(gateway_spine::MemoryAudit::new());
        let store = Arc::new(
            gateway_store::Store::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        let state = Arc::new(AppState::with_parts(
            Arc::new(ks),
            Arc::new(MockClock::new(0)),
            crate::providers::ProviderRegistry::new(),
            Arc::new(crate::guard::empty_chain()),
            audit.clone() as Arc<dyn gateway_spine::AuditSink>,
            store,
        ));
        (state, audit)
    }

    async fn with_server(state: &Arc<AppState<MockClock>>) {
        state
            .federation
            .register_server(McpServer::new("srv", Arc::new(TestTransport)))
            .await;
        state.federation.refresh_server("srv").await.unwrap();
    }

    fn post_mcp(body: serde_json::Value, auth: Option<&str>) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json");
        if let Some(a) = auth {
            b = b.header("authorization", format!("Bearer {a}"));
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn mcp_without_auth_is_401() {
        let app = crate::server::router(test_state().await);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mcp_initialize_returns_server_info() {
        let state = test_state().await;
        let app = crate::server::router(state);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
                Some("sk-good"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["result"]["serverInfo"]["name"], "oximy-gateway");
    }

    #[tokio::test]
    async fn mcp_tools_list_returns_federated_tools() {
        let state = test_state().await;
        with_server(&state).await;
        let app = crate::server::router(state);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
                Some("sk-good"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let names: Vec<&str> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"srv__echo"));
        assert!(names.contains(&"srv__danger"));
    }

    #[tokio::test]
    async fn mcp_tools_list_is_acl_filtered() {
        let state = test_state().await;
        with_server(&state).await;
        // Restrict key_mcp to only the echo tool.
        {
            let mut acl = state.federation.acl_mut().await;
            acl.set("key_mcp", Some(HashSet::from(["srv__echo".to_string()])));
        }
        let app = crate::server::router(state);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
                Some("sk-good"),
            ))
            .await
            .unwrap();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "ACL should hide 'danger'");
        assert_eq!(tools[0]["name"], "srv__echo");
    }

    #[tokio::test]
    async fn mcp_tools_call_dispatches_and_audits() {
        let (state, audit) = test_state_with_audit().await;
        with_server(&state).await;
        let app = crate::server::router(state);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                       "params":{"name":"srv__echo","arguments":{"msg":"hi"}}}),
                Some("sk-good"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["result"]["content"][0]["text"], "called:echo");

        // The federation audited the call on the shared spine sink.
        let events = audit.events();
        let call = events
            .iter()
            .find(|e| e.action == "mcp.tools.call" && e.target == "srv__echo");
        assert!(call.is_some(), "tool call must be audited");
        assert_eq!(call.unwrap().actor, "key_mcp");
        assert_eq!(call.unwrap().outcome, "ok");
    }

    #[tokio::test]
    async fn mcp_unknown_method_is_method_not_found() {
        let state = test_state().await;
        let app = crate::server::router(state);
        let resp = app
            .oneshot(post_mcp(
                json!({"jsonrpc":"2.0","id":5,"method":"frobnicate"}),
                Some("sk-good"),
            ))
            .await
            .unwrap();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }
}
