//! The `Federation` — the gateway's core MCP plane.
//!
//! Registers N upstream `McpServer`s, aggregates their tools with namespacing
//! (`server__tool`), dispatches `tools/call` to the owning server, enforces
//! per-key tool ACLs, and emits audit events on every call.
//!
//! # Design decisions
//! - Tools are lazily fetched on `refresh_server`; callers drive refresh.
//! - The federation holds a `RwLock<ToolRegistry>` for reads, `Mutex<HashMap>`
//!   for the server map (a server is rarely added mid-run).
//! - Audit events are emitted synchronously via the `AuditSink` trait.
//! - Tool-description changes hard-error by default; `force_update` override
//!   available for acknowledged operator changes.
//!
//! # Deferred
//! - OAuth 2.1 brokering
//! - Semantic tool discovery (`find_tool` / `call_tool`)
//! - Stateless RC session shim
//! - Inbound SSE server side

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info};

use gateway_spine::{AuditEvent, AuditSink, MemoryAudit};

use crate::{
    acl::ToolAcl,
    error::McpError,
    registry::ToolRegistry,
    server::McpServer,
    types::{Tool, ToolCallResult, ToolsListResult},
};

// ─── Federation ──────────────────────────────────────────────────────────────

pub struct Federation {
    servers: Mutex<HashMap<String, Arc<McpServer>>>,
    registry: RwLock<ToolRegistry>,
    acl: RwLock<ToolAcl>,
    audit: Arc<dyn AuditSink>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Federation {
    /// Create a federation with an injected audit sink.
    pub fn new(audit: Arc<dyn AuditSink>) -> Self {
        Self {
            servers: Mutex::new(HashMap::new()),
            registry: RwLock::new(ToolRegistry::new()),
            acl: RwLock::new(ToolAcl::new()),
            audit,
            clock: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64
            }),
        }
    }

    /// Create a federation with an in-memory audit sink (useful for tests).
    pub fn with_memory_audit() -> (Self, Arc<MemoryAudit>) {
        let sink = Arc::new(MemoryAudit::new());
        let fed = Self::new(sink.clone() as Arc<dyn AuditSink>);
        (fed, sink)
    }

    // ──── Server management ──────────────────────────────────────────────────

    /// Register an upstream server.  Does NOT fetch tools automatically;
    /// call `refresh_server` to populate the registry.
    pub async fn register_server(&self, server: McpServer) {
        let name = server.name.clone();
        let mut servers = self.servers.lock().await;
        info!(server = %name, "registered upstream MCP server");
        servers.insert(name, Arc::new(server));
    }

    /// Remove an upstream server and all its tools from the registry.
    pub async fn remove_server(&self, server_name: &str) {
        self.servers.lock().await.remove(server_name);
        self.registry.write().await.remove_server(server_name);
        info!(server = %server_name, "removed upstream MCP server");
    }

    /// List registered server names.
    pub async fn server_names(&self) -> Vec<String> {
        self.servers.lock().await.keys().cloned().collect()
    }

    // ──── Tool refresh ───────────────────────────────────────────────────────

    /// Fetch the tool list from `server_name` and upsert into the registry.
    /// Returns the list of namespaced tool names that were inserted/updated.
    pub async fn refresh_server(&self, server_name: &str) -> Result<Vec<String>, McpError> {
        let server = {
            let servers = self.servers.lock().await;
            servers
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.to_string()))?
        };

        let result = server.list_tools().await;
        match result {
            Err(e) => {
                error!(server = %server_name, error = %e, "tool refresh failed");
                self.registry
                    .write()
                    .await
                    .mark_server_unhealthy(server_name);
                Err(e)
            }
            Ok(list) => {
                let now = (self.clock)();
                let names = self
                    .registry
                    .write()
                    .await
                    .upsert_tools(server_name, list.tools, now)?
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect();
                Ok(names)
            }
        }
    }

    /// Like `refresh_server` but force-updates description hashes without
    /// error.  Use when the operator has acknowledged a tool change.
    pub async fn force_refresh_server(&self, server_name: &str) -> Result<(), McpError> {
        let server = {
            let servers = self.servers.lock().await;
            servers
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.to_string()))?
        };
        let list = server.list_tools().await?;
        let now = (self.clock)();
        self.registry
            .write()
            .await
            .force_update(server_name, list.tools, now);
        Ok(())
    }

    // ──── Tool listing ───────────────────────────────────────────────────────

    /// Return all healthy tools visible to `caller_key`.
    ///
    /// If `caller_key` is `None`, no ACL filtering is applied (internal use).
    pub async fn list_tools(&self, caller_key: Option<&str>) -> ToolsListResult {
        let reg = self.registry.read().await;
        let acl = self.acl.read().await;

        let tools: Vec<Tool> = reg
            .healthy_entries()
            .into_iter()
            .filter(|e| {
                caller_key
                    .map(|k| acl.is_allowed(k, &e.namespaced_name))
                    .unwrap_or(true)
            })
            .map(|e| {
                // Return the tool with its namespaced name so callers
                // know how to invoke it.
                let mut t = e.tool.clone();
                t.name = e.namespaced_name.clone();
                t
            })
            .collect();

        ToolsListResult {
            tools,
            next_cursor: None,
        }
    }

    // ──── Tool dispatch ──────────────────────────────────────────────────────

    /// Dispatch a `tools/call` for `namespaced_name` on behalf of `caller_key`.
    ///
    /// Steps:
    ///   1. Resolve entry by namespaced name → find owning server.
    ///   2. Check ACL.
    ///   3. Dispatch to the upstream server.
    ///   4. Emit audit event.
    pub async fn call_tool(
        &self,
        namespaced_name: &str,
        arguments: Option<serde_json::Value>,
        caller_key: Option<&str>,
    ) -> Result<ToolCallResult, McpError> {
        // 1. Resolve.
        let (server_name, original_tool_name) = {
            let reg = self.registry.read().await;
            let entry = reg
                .get(namespaced_name)
                .ok_or_else(|| McpError::UnknownTool(namespaced_name.to_string()))?;
            (entry.server_name.clone(), entry.tool.name.clone())
        };

        // 2. ACL check.
        if let Some(key) = caller_key {
            let acl = self.acl.read().await;
            if !acl.is_allowed(key, namespaced_name) {
                self.emit_audit(
                    caller_key.unwrap_or("anon"),
                    "mcp.tools.call",
                    namespaced_name,
                    "denied",
                    Some("tool not in allowlist"),
                );
                return Err(McpError::ToolNotAllowed(namespaced_name.to_string()));
            }
        }

        // 3. Dispatch.
        let server = {
            let servers = self.servers.lock().await;
            servers
                .get(&server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.clone()))?
        };

        debug!(
            server = %server_name,
            tool = %original_tool_name,
            namespaced = %namespaced_name,
            caller = ?caller_key,
            "dispatching tool call"
        );

        let result = server.call_tool(&original_tool_name, arguments).await;

        // 4. Audit.
        let outcome = match &result {
            Ok(r) if r.is_error() => "tool_error",
            Ok(_) => "ok",
            Err(_) => "error",
        };
        self.emit_audit(
            caller_key.unwrap_or("anon"),
            "mcp.tools.call",
            namespaced_name,
            outcome,
            result.as_ref().err().map(|e| e.to_string()).as_deref(),
        );

        result
    }

    // ──── ACL management ────────────────────────────────────────────────────

    pub async fn acl_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, ToolAcl> {
        self.acl.write().await
    }

    // ──── Internals ──────────────────────────────────────────────────────────

    fn emit_audit(
        &self,
        actor: &str,
        action: &str,
        target: &str,
        outcome: &str,
        detail: Option<&str>,
    ) {
        let now = (self.clock)();
        self.audit.record(AuditEvent {
            ts_ms: now,
            actor: actor.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            outcome: outcome.to_string(),
            detail: detail.map(String::from),
        });
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;
    use crate::types::{Tool, ToolCallResult, ToolContent};
    use serde_json::json;
    use std::sync::Arc;

    fn make_echo_tool() -> Tool {
        Tool {
            name: "echo".into(),
            description: Some("echo back".into()),
            input_schema: json!({"type":"object"}),
            output_schema: None,
            annotations: None,
        }
    }

    fn make_add_tool() -> Tool {
        Tool {
            name: "add".into(),
            description: Some("add numbers".into()),
            input_schema: json!({"type":"object"}),
            output_schema: None,
            annotations: None,
        }
    }

    fn tools_list_response(tools: &[Tool]) -> serde_json::Value {
        let tools_val = serde_json::to_value(tools).unwrap();
        json!({ "tools": tools_val })
    }

    fn tool_call_response(text: &str) -> serde_json::Value {
        let result = ToolCallResult::ok(vec![ToolContent::text(text)]);
        serde_json::to_value(&result).unwrap()
    }

    /// Build a mock transport that serves a list of tools and echoes calls.
    fn mock_server(name: &str, tools: Vec<Tool>) -> McpServer {
        let tools_clone = tools.clone();
        let transport = Arc::new(
            MockTransport::new(name)
                .on("initialize", |_| json!({"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"mock","version":"0"}}))
                .on("tools/list", move |_| tools_list_response(&tools_clone))
                .on("tools/call", |params| {
                    let tool_name = params
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    tool_call_response(&format!("called:{tool_name}"))
                }),
        );
        McpServer::new(name, transport)
    }

    #[tokio::test]
    async fn register_and_refresh_populates_registry() {
        let (fed, _audit) = Federation::with_memory_audit();
        fed.register_server(mock_server(
            "alpha",
            vec![make_echo_tool(), make_add_tool()],
        ))
        .await;

        let names = fed.refresh_server("alpha").await.unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"alpha__echo".to_string()));
        assert!(names.contains(&"alpha__add".to_string()));
    }

    #[tokio::test]
    async fn list_tools_returns_namespaced_names() {
        let (fed, _audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("alpha", vec![make_echo_tool()]))
            .await;
        fed.refresh_server("alpha").await.unwrap();

        let result = fed.list_tools(None).await;
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "alpha__echo");
    }

    #[tokio::test]
    async fn list_tools_from_two_servers() {
        let (fed, _audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool()]))
            .await;
        fed.register_server(mock_server("srv2", vec![make_add_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();
        fed.refresh_server("srv2").await.unwrap();

        let result = fed.list_tools(None).await;
        assert_eq!(result.tools.len(), 2);
        let names: Vec<_> = result.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"srv1__echo"));
        assert!(names.contains(&"srv2__add"));
    }

    #[tokio::test]
    async fn call_tool_dispatches_to_correct_server() {
        let (fed, audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool()]))
            .await;
        fed.register_server(mock_server("srv2", vec![make_add_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();
        fed.refresh_server("srv2").await.unwrap();

        // Call a tool on srv1.
        let result = fed
            .call_tool("srv1__echo", Some(json!({"msg":"hi"})), Some("key1"))
            .await
            .unwrap();
        assert!(!result.is_error());
        // The mock echoes "called:echo"
        assert!(
            matches!(&result.content[0], crate::types::ToolContent::Text { text } if text.contains("called:echo"))
        );

        // Audit event should have been recorded.
        let events = audit.events();
        assert!(!events.is_empty());
        let ev = events.last().unwrap();
        assert_eq!(ev.action, "mcp.tools.call");
        assert_eq!(ev.target, "srv1__echo");
        assert_eq!(ev.outcome, "ok");
        assert_eq!(ev.actor, "key1");
    }

    #[tokio::test]
    async fn allowlist_hides_tools_from_list() {
        let (fed, _audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool(), make_add_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();

        // Restrict key1 to only echo.
        {
            let mut acl = fed.acl_mut().await;
            acl.allow_tool("key1", "srv1__echo");
        }

        let result = fed.list_tools(Some("key1")).await;
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "srv1__echo");
    }

    #[tokio::test]
    async fn allowlist_blocks_call() {
        let (fed, audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool(), make_add_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();

        // Restrict key1 to only echo.
        {
            let mut acl = fed.acl_mut().await;
            acl.allow_tool("key1", "srv1__echo");
        }

        let err = fed
            .call_tool("srv1__add", None, Some("key1"))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::ToolNotAllowed(_)));

        // Denied event recorded.
        let events = audit.events();
        let denied = events.iter().find(|e| e.outcome == "denied");
        assert!(denied.is_some());
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let (fed, _audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();

        let err = fed
            .call_tool("srv1__nonexistent", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::UnknownTool(_)));
    }

    #[tokio::test]
    async fn dispatch_routing_correct_server_name() {
        // Two servers, each with a tool named "ping".
        let transport_a = Arc::new(
            MockTransport::new("srv_a")
                .on("initialize", |_| json!({}))
                .on("tools/list", |_| {
                    json!({"tools":[{"name":"ping","description":"from A","inputSchema":{"type":"object"}}]})
                })
                .on("tools/call", |_| {
                    let r = ToolCallResult::ok(vec![ToolContent::text("pong-A")]);
                    serde_json::to_value(&r).unwrap()
                }),
        );
        let transport_b = Arc::new(
            MockTransport::new("srv_b")
                .on("initialize", |_| json!({}))
                .on("tools/list", |_| {
                    json!({"tools":[{"name":"ping","description":"from B","inputSchema":{"type":"object"}}]})
                })
                .on("tools/call", |_| {
                    let r = ToolCallResult::ok(vec![ToolContent::text("pong-B")]);
                    serde_json::to_value(&r).unwrap()
                }),
        );

        let (fed, _) = Federation::with_memory_audit();
        fed.register_server(McpServer::new("srv_a", transport_a))
            .await;
        fed.register_server(McpServer::new("srv_b", transport_b))
            .await;
        fed.refresh_server("srv_a").await.unwrap();
        fed.refresh_server("srv_b").await.unwrap();

        let r_a = fed.call_tool("srv_a__ping", None, None).await.unwrap();
        assert!(matches!(&r_a.content[0], ToolContent::Text{text} if text == "pong-A"));

        let r_b = fed.call_tool("srv_b__ping", None, None).await.unwrap();
        assert!(matches!(&r_b.content[0], ToolContent::Text{text} if text == "pong-B"));
    }

    #[tokio::test]
    async fn description_hash_change_errors_on_refresh() {
        // First server returns v1 on the first call, v2 on the second.
        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let call_count_clone = call_count.clone();
        let transport = Arc::new(
            MockTransport::new("srv1")
                .on("initialize", |_| json!({}))
                .on("tools/list", move |_| {
                    let n = call_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if n == 0 {
                        tools_list_response(&[make_echo_tool()])
                    } else {
                        let mut t = make_echo_tool();
                        t.description = Some("DIFFERENT".into());
                        tools_list_response(&[t])
                    }
                }),
        );

        let (fed, _) = Federation::with_memory_audit();
        fed.register_server(McpServer::new("srv1", transport)).await;

        // First refresh succeeds.
        fed.refresh_server("srv1").await.unwrap();

        // Second refresh detects the description change.
        let err = fed.refresh_server("srv1").await.unwrap_err();
        assert!(matches!(err, McpError::DescriptionHashChanged { .. }));
    }

    #[tokio::test]
    async fn no_key_means_open_acl() {
        let (fed, _) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();

        // No caller_key → open
        let result = fed.call_tool("srv1__echo", None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn audit_records_denied_call() {
        let (fed, audit) = Federation::with_memory_audit();
        fed.register_server(mock_server("srv1", vec![make_echo_tool()]))
            .await;
        fed.refresh_server("srv1").await.unwrap();

        {
            let mut acl = fed.acl_mut().await;
            // key2 can see nothing
            acl.set("key2", Some(std::collections::HashSet::new()));
        }

        let _ = fed.call_tool("srv1__echo", None, Some("key2")).await;
        let events = audit.events();
        let denied = events
            .iter()
            .find(|e| e.actor == "key2" && e.outcome == "denied");
        assert!(
            denied.is_some(),
            "should have a denied audit event for key2"
        );
    }
}
