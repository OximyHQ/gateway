//! `apply`: execute a `Diff` against the durable `ConfigStore` (the same store the
//! API mutates) so UI = API = CLI = Git really are one engine. Translates config
//! rows into storage entities (USD budgets → µUSD), sealing provider secrets with
//! the `MasterKey` so plaintext never lands in the store. Creates/updates are
//! upserts; key deletes revoke (never hard-delete spend history — cost-correctness).

use crate::crypto::MasterKey;
use crate::diff::{Change, Diff, diff};
use crate::error::ConfigError;
use crate::model::{Config, KeyConfig, ProviderConfig};
use crate::store::{ConfigStore, StoredKey, StoredProvider};

const MICROS_PER_DOLLAR: f64 = 1_000_000.0;

fn budget_to_micros(budget_usd: Option<f64>) -> Option<i64> {
    budget_usd.map(|d| (d * MICROS_PER_DOLLAR).round() as i64)
}

fn provider_to_stored(p: &ProviderConfig, mk: &MasterKey) -> Result<StoredProvider, ConfigError> {
    let sealed_api_key = match &p.api_key {
        Some(secret) => Some(mk.seal(secret)?),
        None => None,
    };
    Ok(StoredProvider {
        id: p.id.clone(),
        kind: p.kind.clone(),
        base_url: p.base_url.clone(),
        sealed_api_key,
    })
}

fn key_to_stored(k: &KeyConfig) -> StoredKey {
    StoredKey {
        id: k.id.clone(),
        max_budget_micros: budget_to_micros(k.max_budget_usd),
        rpm: k.rpm,
        tpm: k.tpm,
        max_parallel: k.max_parallel,
        model_allowlist: k.model_allowlist.clone(),
        revoked: false,
    }
}

/// Compute the diff and execute it. Returns the diff that was applied (so the CLI
/// can print the plan it ran). Idempotent: applying an already-matching config is
/// a no-op.
pub async fn apply(
    desired: &Config,
    live: &Config,
    store: &dyn ConfigStore,
    mk: &MasterKey,
) -> Result<Diff, ConfigError> {
    let plan = diff(desired, live);
    for change in &plan.changes {
        match change {
            Change::CreateProvider(id) | Change::UpdateProvider(id) => {
                let p = desired
                    .providers
                    .iter()
                    .find(|p| &p.id == id)
                    .expect("id in desired");
                store.upsert_provider(&provider_to_stored(p, mk)?).await?;
            }
            Change::CreateKey(id) | Change::UpdateKey(id) => {
                let k = desired
                    .keys
                    .iter()
                    .find(|k| &k.id == id)
                    .expect("id in desired");
                store.upsert_key(&key_to_stored(k)).await?;
            }
            Change::DeleteKey(id) => {
                // Never hard-delete: revoke so spend history survives.
                store.revoke_key(id).await?;
            }
            // Provider deletes + route create/update/delete: routes land with the
            // router (P1.4/later); provider delete is revoke-by-omission in future.
            Change::DeleteProvider(_)
            | Change::CreateRoute(_)
            | Change::UpdateRoute(_)
            | Change::DeleteRoute(_) => {}
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderConfig;
    use crate::store::SqliteConfigStore;

    fn mk() -> MasterKey {
        MasterKey::from_bytes([5u8; 32])
    }

    async fn store() -> SqliteConfigStore {
        let s = SqliteConfigStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn apply_creates_key_with_correct_budget() {
        let s = store().await;
        let mut desired = Config::default();
        desired.keys.push(KeyConfig {
            id: "k1".into(),
            max_budget_usd: Some(12.50),
            rpm: Some(100),
            tpm: None,
            max_parallel: None,
            model_allowlist: Some(vec!["gpt-4o".into()]),
        });
        let plan = apply(&desired, &Config::default(), &s, &mk())
            .await
            .unwrap();
        assert_eq!(plan.changes, vec![Change::CreateKey("k1".into())]);

        let keys = s.load_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].max_budget_micros, Some(12_500_000));
        assert_eq!(keys[0].rpm, Some(100));
    }

    #[tokio::test]
    async fn apply_seals_provider_secret_never_plaintext() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: Some("sk-live-openai".into()),
        });
        apply(&desired, &Config::default(), &s, &mk())
            .await
            .unwrap();

        let providers = s.load_providers().await.unwrap();
        let sealed = providers[0].sealed_api_key.as_ref().unwrap();
        // Stored value is ciphertext, not the plaintext secret.
        assert_ne!(sealed, "sk-live-openai");
        // ...but the master key opens it back.
        assert_eq!(mk().open(sealed).unwrap(), "sk-live-openai");
    }

    #[tokio::test]
    async fn apply_delete_revokes_not_destroys() {
        let s = store().await;
        let mut live = Config::default();
        live.keys.push(KeyConfig {
            id: "gone".into(),
            max_budget_usd: Some(1.0),
            rpm: None,
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        });
        // Seed the store so the key exists to revoke.
        apply(&live, &Config::default(), &s, &mk()).await.unwrap();

        // Desired drops the key → plan is a delete → store revokes it.
        let plan = apply(&Config::default(), &live, &s, &mk()).await.unwrap();
        assert_eq!(plan.changes, vec![Change::DeleteKey("gone".into())]);
        assert!(s.load_keys().await.unwrap()[0].revoked);
    }

    #[tokio::test]
    async fn apply_is_idempotent() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: None,
        });
        apply(&desired, &Config::default(), &s, &mk())
            .await
            .unwrap();
        // Second apply against the now-matching live state is a no-op plan.
        let plan = apply(&desired, &desired.clone(), &s, &mk()).await.unwrap();
        assert!(plan.is_empty());
    }
}
