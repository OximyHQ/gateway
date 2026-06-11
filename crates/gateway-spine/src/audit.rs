//! Append-only audit trail. The spine records admin actions and request
//! outcomes through this seam; P1.7 swaps in a durable sink. The same stream
//! will carry MCP tool-call audit in P2 — one audit log across both planes.

use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub ts_ms: i64,
    /// Key id, or "admin" for control-plane actions.
    pub actor: String,
    /// e.g. "key.create", "request.complete", "request.denied".
    pub action: String,
    pub target: String,
    /// "ok" | "denied" | "error".
    pub outcome: String,
    pub detail: Option<String>,
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[derive(Default)]
pub struct MemoryAudit {
    events: Mutex<Vec<AuditEvent>>,
}

impl MemoryAudit {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().unwrap().clone()
    }
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for MemoryAudit {
    fn record(&self, event: AuditEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_in_order() {
        let a = MemoryAudit::new();
        assert!(a.is_empty());
        a.record(AuditEvent {
            ts_ms: 1,
            actor: "admin".into(),
            action: "key.create".into(),
            target: "key_1".into(),
            outcome: "ok".into(),
            detail: None,
        });
        a.record(AuditEvent {
            ts_ms: 2,
            actor: "key_1".into(),
            action: "request.denied".into(),
            target: "gpt-4o".into(),
            outcome: "denied".into(),
            detail: Some("budget exceeded".into()),
        });
        let ev = a.events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].action, "key.create");
        assert_eq!(ev[1].outcome, "denied");
    }

    #[test]
    fn works_through_trait_object() {
        let sink: Box<dyn AuditSink> = Box::new(MemoryAudit::new());
        sink.record(AuditEvent {
            ts_ms: 0,
            actor: "admin".into(),
            action: "boot".into(),
            target: "gateway".into(),
            outcome: "ok".into(),
            detail: None,
        });
    }
}
