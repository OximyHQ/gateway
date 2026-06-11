//! Per-key tool allowlists.
//!
//! Each virtual key can be restricted to a subset of namespaced tool names.
//! `None` = all tools allowed (the open default).

use std::collections::{HashMap, HashSet};

/// Policy store for tool allowlists.
///
/// Key = virtual key ID (opaque string).
/// Value = `None` (all tools allowed) or `Some(set of allowed namespaced names)`.
#[derive(Default)]
pub struct ToolAcl {
    policies: HashMap<String, Option<HashSet<String>>>,
}

impl ToolAcl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a key's allowlist.  Pass `None` to grant access to all tools.
    pub fn set(&mut self, key_id: impl Into<String>, allow: Option<HashSet<String>>) {
        self.policies.insert(key_id.into(), allow);
    }

    /// Allow a specific tool for a key (creates the entry if absent).
    pub fn allow_tool(&mut self, key_id: &str, namespaced: impl Into<String>) {
        self.policies
            .entry(key_id.to_string())
            .or_insert_with(|| Some(HashSet::new()))
            .get_or_insert_with(HashSet::new)
            .insert(namespaced.into());
    }

    /// Returns `true` if the key may call `namespaced_tool_name`.
    ///
    /// - If the key has no policy entry: allowed (open default).
    /// - If the key has `None`: allowed (explicit open).
    /// - If the key has `Some(set)`: allowed iff tool is in the set.
    pub fn is_allowed(&self, key_id: &str, namespaced_tool_name: &str) -> bool {
        match self.policies.get(key_id) {
            None => true,       // unknown key → open default
            Some(None) => true, // explicit open
            Some(Some(set)) => set.contains(namespaced_tool_name),
        }
    }

    /// Filter a list of namespaced tool names to only those allowed for `key_id`.
    pub fn filter_tools<'a>(
        &self,
        key_id: &str,
        tools: impl Iterator<Item = &'a str>,
    ) -> Vec<&'a str> {
        tools.filter(|t| self.is_allowed(key_id, t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_is_open() {
        let acl = ToolAcl::new();
        assert!(acl.is_allowed("k1", "srv__echo"));
    }

    #[test]
    fn explicit_open_allows_all() {
        let mut acl = ToolAcl::new();
        acl.set("k1", None);
        assert!(acl.is_allowed("k1", "srv__anything"));
    }

    #[test]
    fn allowlist_restricts() {
        let mut acl = ToolAcl::new();
        let mut allowed = HashSet::new();
        allowed.insert("srv__echo".to_string());
        acl.set("k1", Some(allowed));

        assert!(acl.is_allowed("k1", "srv__echo"));
        assert!(!acl.is_allowed("k1", "srv__danger"));
    }

    #[test]
    fn allow_tool_builds_set() {
        let mut acl = ToolAcl::new();
        acl.allow_tool("k1", "srv__tool_a");
        acl.allow_tool("k1", "srv__tool_b");

        assert!(acl.is_allowed("k1", "srv__tool_a"));
        assert!(acl.is_allowed("k1", "srv__tool_b"));
        assert!(!acl.is_allowed("k1", "srv__tool_c"));
    }

    #[test]
    fn filter_tools_respects_allowlist() {
        let mut acl = ToolAcl::new();
        acl.allow_tool("k1", "srv__echo");

        let tools = ["srv__echo", "srv__add", "srv__delete"];
        let visible = acl.filter_tools("k1", tools.iter().copied());
        assert_eq!(visible, vec!["srv__echo"]);
    }
}
