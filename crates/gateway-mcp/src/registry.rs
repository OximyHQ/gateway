//! In-memory `ToolRegistry` — the single source of truth for tools across all
//! registered upstream servers.
//!
//! Each entry carries:
//!   - the original [`Tool`] definition
//!   - the owning `server_name`
//!   - the **namespaced** name (`server__tool`)
//!   - a description-pin hash for rug-pull detection
//!   - a `healthy` flag updated by the federation on heartbeat/refresh
//!   - `last_seen_ms` — the epoch-ms timestamp of the last successful list

use std::collections::HashMap;

use tracing::warn;

use crate::{error::McpError, hash::description_hash, types::Tool};

// ─── Entry ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ToolEntry {
    /// The tool definition as returned by the upstream server (name is the
    /// original, not the namespaced form).
    pub tool: Tool,
    /// The server that owns this tool.
    pub server_name: String,
    /// `server__tool` (double-underscore convention, same as ContextForge).
    pub namespaced_name: String,
    /// SHA-256 hex of description + input_schema at registration time.
    pub description_hash: String,
    /// Whether the owning server was healthy at last check.
    pub healthy: bool,
    /// Epoch millis of the last successful `tools/list` that returned this tool.
    pub last_seen_ms: i64,
}

// ─── Registry ────────────────────────────────────────────────────────────────

/// All tools known to the gateway, keyed by their **namespaced** name.
#[derive(Default)]
pub struct ToolRegistry {
    /// key = namespaced_name
    entries: HashMap<String, ToolEntry>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update tools from a server.
    ///
    /// For each tool:
    ///   - if new → insert
    ///   - if existing and hash matches → update `last_seen_ms`
    ///   - if existing and hash differs → return `Err(DescriptionHashChanged)`
    ///     (caller decides whether to abort or force-update)
    ///
    /// Returns the list of `(namespaced_name, changed)` pairs where `changed`
    /// is `true` when the hash was new/updated.
    pub fn upsert_tools(
        &mut self,
        server_name: &str,
        tools: Vec<Tool>,
        now_ms: i64,
    ) -> Result<Vec<(String, bool)>, McpError> {
        let mut results = Vec::new();
        for tool in tools {
            let hash =
                description_hash(&tool.name, tool.description.as_deref(), &tool.input_schema);
            let ns = namespace(server_name, &tool.name);

            if let Some(existing) = self.entries.get_mut(&ns) {
                if existing.description_hash != hash {
                    warn!(
                        server = server_name,
                        tool = %tool.name,
                        old_hash = %existing.description_hash,
                        new_hash = %hash,
                        "tool description hash changed — possible rug-pull"
                    );
                    return Err(McpError::DescriptionHashChanged {
                        tool: ns,
                        expected: existing.description_hash.clone(),
                        got: hash,
                    });
                }
                existing.last_seen_ms = now_ms;
                existing.healthy = true;
                results.push((existing.namespaced_name.clone(), false));
            } else {
                let entry = ToolEntry {
                    description_hash: hash,
                    namespaced_name: ns.clone(),
                    tool,
                    server_name: server_name.to_string(),
                    healthy: true,
                    last_seen_ms: now_ms,
                };
                self.entries.insert(ns.clone(), entry);
                results.push((ns, true));
            }
        }
        Ok(results)
    }

    /// Force-update a tool's hash without raising an error.
    /// Used when the operator explicitly acknowledges a tool change.
    pub fn force_update(&mut self, server_name: &str, tools: Vec<Tool>, now_ms: i64) {
        for tool in tools {
            let hash =
                description_hash(&tool.name, tool.description.as_deref(), &tool.input_schema);
            let ns = namespace(server_name, &tool.name);
            let entry = ToolEntry {
                description_hash: hash,
                namespaced_name: ns.clone(),
                tool,
                server_name: server_name.to_string(),
                healthy: true,
                last_seen_ms: now_ms,
            };
            self.entries.insert(ns, entry);
        }
    }

    /// Mark all tools belonging to `server_name` as unhealthy.
    pub fn mark_server_unhealthy(&mut self, server_name: &str) {
        for entry in self.entries.values_mut() {
            if entry.server_name == server_name {
                entry.healthy = false;
            }
        }
    }

    /// Remove all tools belonging to `server_name`.
    pub fn remove_server(&mut self, server_name: &str) {
        self.entries.retain(|_, v| v.server_name != server_name);
    }

    /// All entries (healthy and unhealthy).
    pub fn all_entries(&self) -> Vec<&ToolEntry> {
        self.entries.values().collect()
    }

    /// Healthy entries only.
    pub fn healthy_entries(&self) -> Vec<&ToolEntry> {
        self.entries.values().filter(|e| e.healthy).collect()
    }

    /// Look up a tool by its namespaced name.
    pub fn get(&self, namespaced: &str) -> Option<&ToolEntry> {
        self.entries.get(namespaced)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a namespaced tool name: `server__tool`.
pub fn namespace(server_name: &str, tool_name: &str) -> String {
    format!("{server_name}__{tool_name}")
}

/// Split `server__tool` back into `(server, tool)`.
/// Returns `None` if the name doesn't contain `__`.
pub fn split_namespace(namespaced: &str) -> Option<(&str, &str)> {
    namespaced.split_once("__")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn echo_tool() -> Tool {
        crate::types::Tool {
            name: "echo".into(),
            description: Some("echo text".into()),
            input_schema: json!({"type":"object","properties":{"msg":{"type":"string"}}}),
            output_schema: None,
            annotations: None,
        }
    }

    fn add_tool() -> Tool {
        crate::types::Tool {
            name: "add".into(),
            description: Some("add numbers".into()),
            input_schema: json!({"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}}}),
            output_schema: None,
            annotations: None,
        }
    }

    #[test]
    fn namespace_format() {
        assert_eq!(namespace("alpha", "echo"), "alpha__echo");
    }

    #[test]
    fn split_namespace_round_trips() {
        assert_eq!(split_namespace("alpha__echo"), Some(("alpha", "echo")));
        assert_eq!(split_namespace("no_dunder"), None);
    }

    #[test]
    fn upsert_new_tools() {
        let mut reg = ToolRegistry::new();
        let results = reg
            .upsert_tools("srv1", vec![echo_tool(), add_tool()], 1000)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, changed)| *changed));
        assert_eq!(reg.len(), 2);
        assert!(reg.get("srv1__echo").is_some());
        assert!(reg.get("srv1__add").is_some());
    }

    #[test]
    fn upsert_same_tool_updates_last_seen() {
        let mut reg = ToolRegistry::new();
        reg.upsert_tools("srv1", vec![echo_tool()], 1000).unwrap();
        let results = reg.upsert_tools("srv1", vec![echo_tool()], 2000).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].1, "should not be changed");
        assert_eq!(reg.get("srv1__echo").unwrap().last_seen_ms, 2000);
    }

    #[test]
    fn upsert_changed_description_errors() {
        let mut reg = ToolRegistry::new();
        reg.upsert_tools("srv1", vec![echo_tool()], 1000).unwrap();

        let mut changed = echo_tool();
        changed.description = Some("DIFFERENT description".into());
        let err = reg.upsert_tools("srv1", vec![changed], 2000).unwrap_err();
        assert!(matches!(err, McpError::DescriptionHashChanged { .. }));
    }

    #[test]
    fn force_update_overrides_hash() {
        let mut reg = ToolRegistry::new();
        reg.upsert_tools("srv1", vec![echo_tool()], 1000).unwrap();
        let mut changed = echo_tool();
        changed.description = Some("DIFFERENT".into());
        reg.force_update("srv1", vec![changed], 2000);
        assert_eq!(
            reg.get("srv1__echo").unwrap().tool.description.as_deref(),
            Some("DIFFERENT")
        );
    }

    #[test]
    fn mark_unhealthy_and_remove() {
        let mut reg = ToolRegistry::new();
        reg.upsert_tools("srv1", vec![echo_tool()], 1000).unwrap();
        reg.upsert_tools("srv2", vec![add_tool()], 1000).unwrap();

        reg.mark_server_unhealthy("srv1");
        assert!(!reg.get("srv1__echo").unwrap().healthy);
        assert!(reg.get("srv2__add").unwrap().healthy);

        let healthy: Vec<_> = reg.healthy_entries();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].server_name, "srv2");

        reg.remove_server("srv1");
        assert!(reg.get("srv1__echo").is_none());
        assert_eq!(reg.len(), 1);
    }
}
