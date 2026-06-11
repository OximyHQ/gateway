//! decK-style diff. Computes the typed delta between a desired `Config` and the
//! live one (projected from store state). `apply` (Task 13) executes exactly
//! these changes — diff is the single planning step so UI/CLI/Git all show the
//! same plan before mutating. Stable ordering so the plan is deterministic.

use crate::model::Config;

/// One planned change to a single entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    CreateProvider(String),
    UpdateProvider(String),
    DeleteProvider(String),
    CreateKey(String),
    UpdateKey(String),
    DeleteKey(String),
    CreateRoute(String),
    UpdateRoute(String),
    DeleteRoute(String),
}

/// The full plan. Empty == live already matches desired (idempotent apply).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Compute desired-vs-live. Entities present-in-desired/absent-in-live → Create;
/// present-in-both but changed → Update; absent-in-desired/present-in-live →
/// Delete. Order: providers, keys, routes; within each, creates/updates then
/// deletes — sorted by id for determinism.
pub fn diff(desired: &Config, live: &Config) -> Diff {
    let mut changes = Vec::new();

    diff_section(
        &desired
            .providers
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect::<Vec<_>>(),
        &live
            .providers
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect::<Vec<_>>(),
        &mut changes,
        Change::CreateProvider,
        Change::UpdateProvider,
        Change::DeleteProvider,
    );
    diff_section(
        &desired
            .keys
            .iter()
            .map(|k| (k.id.clone(), k.clone()))
            .collect::<Vec<_>>(),
        &live
            .keys
            .iter()
            .map(|k| (k.id.clone(), k.clone()))
            .collect::<Vec<_>>(),
        &mut changes,
        Change::CreateKey,
        Change::UpdateKey,
        Change::DeleteKey,
    );
    diff_section(
        &desired
            .routes
            .iter()
            .map(|r| (r.id.clone(), r.clone()))
            .collect::<Vec<_>>(),
        &live
            .routes
            .iter()
            .map(|r| (r.id.clone(), r.clone()))
            .collect::<Vec<_>>(),
        &mut changes,
        Change::CreateRoute,
        Change::UpdateRoute,
        Change::DeleteRoute,
    );

    Diff { changes }
}

fn diff_section<T: PartialEq + Clone>(
    desired: &[(String, T)],
    live: &[(String, T)],
    out: &mut Vec<Change>,
    create: fn(String) -> Change,
    update: fn(String) -> Change,
    delete: fn(String) -> Change,
) {
    let mut creates_updates: Vec<Change> = Vec::new();
    let mut deletes: Vec<Change> = Vec::new();

    for (id, want) in desired {
        match live.iter().find(|(lid, _)| lid == id) {
            None => creates_updates.push(create(id.clone())),
            Some((_, have)) if have != want => creates_updates.push(update(id.clone())),
            Some(_) => {}
        }
    }
    for (id, _) in live {
        if !desired.iter().any(|(did, _)| did == id) {
            deletes.push(delete(id.clone()));
        }
    }

    creates_updates.sort_by_key(change_id);
    deletes.sort_by_key(change_id);
    out.extend(creates_updates);
    out.extend(deletes);
}

fn change_id(c: &Change) -> String {
    match c {
        Change::CreateProvider(s)
        | Change::UpdateProvider(s)
        | Change::DeleteProvider(s)
        | Change::CreateKey(s)
        | Change::UpdateKey(s)
        | Change::DeleteKey(s)
        | Change::CreateRoute(s)
        | Change::UpdateRoute(s)
        | Change::DeleteRoute(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyConfig, ProviderConfig};

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            kind: "openai".into(),
            base_url: None,
            api_key: None,
        }
    }

    fn key(id: &str, budget: f64) -> KeyConfig {
        KeyConfig {
            id: id.into(),
            max_budget_usd: Some(budget),
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        }
    }

    #[test]
    fn identical_configs_have_empty_diff() {
        let mut c = Config::default();
        c.providers.push(provider("openai"));
        assert!(diff(&c, &c.clone()).is_empty());
    }

    #[test]
    fn detects_create() {
        let live = Config::default();
        let mut desired = Config::default();
        desired.providers.push(provider("openai"));
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::CreateProvider("openai".into())]);
    }

    #[test]
    fn detects_update_on_changed_field() {
        let mut live = Config::default();
        live.keys.push(key("k1", 5.0));
        let mut desired = Config::default();
        desired.keys.push(key("k1", 9.0));
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::UpdateKey("k1".into())]);
    }

    #[test]
    fn detects_delete() {
        let mut live = Config::default();
        live.providers.push(provider("stale"));
        let desired = Config::default();
        let d = diff(&desired, &live);
        assert_eq!(d.changes, vec![Change::DeleteProvider("stale".into())]);
    }

    #[test]
    fn creates_and_updates_precede_deletes_and_are_sorted() {
        let mut live = Config::default();
        live.providers.push(provider("old"));
        let mut desired = Config::default();
        desired.providers.push(provider("bbb"));
        desired.providers.push(provider("aaa"));
        let d = diff(&desired, &live);
        assert_eq!(
            d.changes,
            vec![
                Change::CreateProvider("aaa".into()),
                Change::CreateProvider("bbb".into()),
                Change::DeleteProvider("old".into()),
            ]
        );
    }
}
