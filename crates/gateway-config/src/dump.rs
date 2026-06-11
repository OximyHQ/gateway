//! `dump`: the inverse projection — read live store state into a `Config` so an
//! operator can capture running state into Git (decK round-trip). Secrets are
//! NEVER dumped in plaintext: a provider with a stored secret emits a
//! `${PROVIDER_ID_API_KEY}` ref placeholder, preserving the secrets-at-rest +
//! secrets-never-in-config-file invariants.

use crate::error::ConfigError;
use crate::model::{Config, KeyConfig, ProviderConfig};
use crate::store::ConfigStore;

const MICROS_PER_DOLLAR: f64 = 1_000_000.0;

fn micros_to_dollars(micros: i64) -> f64 {
    micros as f64 / MICROS_PER_DOLLAR
}

/// Read all durable state into a `Config`. The result, re-applied, is a no-op.
pub async fn dump(store: &dyn ConfigStore) -> Result<Config, ConfigError> {
    let mut config = Config::default();

    for p in store.load_providers().await? {
        let api_key = p
            .sealed_api_key
            .as_ref()
            .map(|_| format!("${{{}_API_KEY}}", p.id.to_uppercase().replace('-', "_")));
        config.providers.push(ProviderConfig {
            id: p.id,
            kind: p.kind,
            base_url: p.base_url,
            api_key,
        });
    }

    for k in store.load_keys().await? {
        if k.revoked {
            continue; // revoked keys are not part of desired state
        }
        config.keys.push(KeyConfig {
            id: k.id,
            max_budget_usd: k.max_budget_micros.map(micros_to_dollars),
            rpm: k.rpm,
            tpm: k.tpm,
            max_parallel: k.max_parallel,
            model_allowlist: k.model_allowlist,
        });
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::apply;
    use crate::crypto::MasterKey;
    use crate::model::ProviderConfig;
    use crate::store::SqliteConfigStore;

    fn mk() -> MasterKey {
        MasterKey::from_bytes([6u8; 32])
    }

    async fn store() -> SqliteConfigStore {
        let s = SqliteConfigStore::connect(":memory:").await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn dump_emits_env_ref_not_plaintext_secret() {
        let s = store().await;
        let mut desired = Config::default();
        desired.providers.push(ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            base_url: None,
            api_key: Some("sk-live".into()),
        });
        apply(&desired, &Config::default(), &s, &mk())
            .await
            .unwrap();

        let dumped = dump(&s).await.unwrap();
        assert_eq!(
            dumped.providers[0].api_key.as_deref(),
            Some("${OPENAI_API_KEY}")
        );
        // The plaintext secret never appears anywhere in the dumped JSON.
        let json = serde_json::to_string(&dumped).unwrap();
        assert!(!json.contains("sk-live"));
    }

    #[tokio::test]
    async fn dump_then_apply_is_a_noop() {
        let s = store().await;
        let mut desired = Config::default();
        desired.keys.push(KeyConfig {
            id: "k1".into(),
            max_budget_usd: Some(7.5),
            rpm: Some(60),
            tpm: None,
            max_parallel: None,
            model_allowlist: None,
        });
        apply(&desired, &Config::default(), &s, &mk())
            .await
            .unwrap();

        let dumped = dump(&s).await.unwrap();
        // Re-applying the dump against itself yields an empty plan.
        let plan = apply(&dumped, &dumped.clone(), &s, &mk()).await.unwrap();
        assert!(plan.is_empty());
        assert_eq!(dumped.keys[0].max_budget_usd, Some(7.5));
    }

    #[tokio::test]
    async fn dump_omits_revoked_keys() {
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
        apply(&live, &Config::default(), &s, &mk()).await.unwrap();
        apply(&Config::default(), &live, &s, &mk()).await.unwrap(); // revoke it

        let dumped = dump(&s).await.unwrap();
        assert!(dumped.keys.is_empty());
    }
}
