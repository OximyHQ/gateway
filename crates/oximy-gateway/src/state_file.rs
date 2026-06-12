//! Minimal JSON state persistence for the gateway's key store. Stores only the
//! hash + metadata of virtual keys (never plaintext secrets). On first boot this
//! file doesn't exist; we create it after seeding the admin key.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use gateway_spine::{RateLimits, Usd, VirtualKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PersistedKey {
    pub id: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub max_budget_micros: Option<i64>,
    // Policy fields below are #[serde(default)] so older state files (which only
    // stored budget/expiry/revoked) still load — they just come back as "no
    // restriction", same as a freshly minted unrestricted key.
    #[serde(default)]
    pub model_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub rpm: Option<i64>,
    #[serde(default)]
    pub tpm: Option<i64>,
    #[serde(default)]
    pub max_parallel: Option<i64>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    pub expires_at: Option<i64>,
    pub revoked: bool,
}

impl From<&VirtualKey> for PersistedKey {
    fn from(k: &VirtualKey) -> Self {
        Self {
            id: k.id.clone(),
            token_hash: k.token_hash.clone(),
            token_prefix: k.token_prefix.clone(),
            max_budget_micros: k.max_budget.map(|u| u.micros()),
            model_allowlist: k.model_allowlist.clone(),
            rpm: k.limits.rpm,
            tpm: k.limits.tpm,
            max_parallel: k.limits.max_parallel,
            tool_allowlist: k.tool_allowlist.clone(),
            expires_at: k.expires_at,
            revoked: k.revoked,
        }
    }
}

impl From<&PersistedKey> for VirtualKey {
    fn from(p: &PersistedKey) -> Self {
        Self {
            id: p.id.clone(),
            token_hash: p.token_hash.clone(),
            token_prefix: p.token_prefix.clone(),
            max_budget: p.max_budget_micros.map(Usd::from_micros),
            limits: RateLimits {
                rpm: p.rpm,
                tpm: p.tpm,
                max_parallel: p.max_parallel,
            },
            model_allowlist: p.model_allowlist.clone(),
            tool_allowlist: p.tool_allowlist.clone(),
            expires_at: p.expires_at,
            revoked: p.revoked,
            parent_id: None,
        }
    }
}

/// A provider added at runtime via the admin API (`POST /v1/admin/providers`).
/// Stored verbatim so it can be re-registered as an OpenAI-compatible deployment
/// on the next boot. The api_key lives here only because the gateway must replay
/// it to the upstream; the file is the operator's responsibility to protect,
/// same as it already is for key hashes.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StoredProvider {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StateFileData {
    pub keys: HashMap<String, PersistedKey>,
    /// Runtime-added providers (admin API). `#[serde(default)]` so older state
    /// files without this field still load.
    #[serde(default)]
    pub providers: Vec<StoredProvider>,
}

/// In-memory view of the JSON state file, implementing `firstboot::KeyStore`.
/// Interior mutability via `Mutex` satisfies `&self` in the `KeyStore` trait.
pub struct StateFile {
    data: Mutex<StateFileData>,
}

impl StateFile {
    /// Load from a JSON file, or create a fresh empty state if the file doesn't exist.
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let text = std::fs::read_to_string(path)?;
            let data: StateFileData = serde_json::from_str(&text)?;
            Ok(Self {
                data: Mutex::new(data),
            })
        } else {
            Ok(Self {
                data: Mutex::new(StateFileData::default()),
            })
        }
    }

    /// Persist to disk. The directory must already exist.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let data = self.data.lock().unwrap();
        let text = serde_json::to_string_pretty(&*data)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Return all stored keys as `VirtualKey` values ready for the key store.
    pub fn load_keys(&self) -> Vec<VirtualKey> {
        self.data
            .lock()
            .unwrap()
            .keys
            .values()
            .map(VirtualKey::from)
            .collect()
    }

    /// Return all runtime-added providers for re-registration at boot.
    pub fn load_providers(&self) -> Vec<StoredProvider> {
        self.data.lock().unwrap().providers.clone()
    }

    /// Add (or replace, by id) a runtime provider. Does not write to disk — the
    /// caller persists via [`StateFile::save`].
    pub fn insert_provider(&self, provider: StoredProvider) {
        let mut data = self.data.lock().unwrap();
        if let Some(existing) = data.providers.iter_mut().find(|p| p.id == provider.id) {
            *existing = provider;
        } else {
            data.providers.push(provider);
        }
    }

    /// Remove a runtime provider by id. Returns `true` if it was present. Does not
    /// write to disk — the caller persists via [`StateFile::save`].
    pub fn remove_provider(&self, id: &str) -> bool {
        let mut data = self.data.lock().unwrap();
        let before = data.providers.len();
        data.providers.retain(|p| p.id != id);
        data.providers.len() != before
    }
}

impl crate::firstboot::KeyStore for StateFile {
    fn insert_key(&self, key: &VirtualKey) -> anyhow::Result<()> {
        let persisted = PersistedKey::from(key);
        self.data
            .lock()
            .unwrap()
            .keys
            .insert(key.id.clone(), persisted);
        Ok(())
    }

    fn key_count(&self) -> anyhow::Result<usize> {
        Ok(self.data.lock().unwrap().keys.len())
    }
}

impl StateFile {
    /// Mark a key as revoked by id. Returns an error if the id is not found.
    pub fn revoke_key(&self, id: &str) -> anyhow::Result<()> {
        let mut data = self.data.lock().unwrap();
        let key = data
            .keys
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("key '{id}' not found"))?;
        key.revoked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firstboot::KeyStore;

    fn rich_key() -> VirtualKey {
        VirtualKey {
            id: "key_scoped".into(),
            token_hash: "deadbeef".into(),
            token_prefix: "ogw_scoped".into(),
            max_budget: Some(Usd::from_dollars_f64(5.0)),
            limits: RateLimits {
                rpm: Some(60),
                tpm: Some(100_000),
                max_parallel: None,
            },
            model_allowlist: Some(vec!["openai/gpt-4o-mini".into()]),
            tool_allowlist: Some(vec!["everything__echo".into()]),
            expires_at: Some(123),
            revoked: false,
            parent_id: None,
        }
    }

    #[test]
    fn key_policy_survives_save_and_reload() {
        // Simulate a restart: persist the state file, then load it fresh from disk
        // exactly as boot does. The scoped policy must come back intact — this is
        // the regression guard for "allowlists silently reset to open on restart".
        let path =
            std::env::temp_dir().join(format!("oximy-gw-restart-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let sf = StateFile::load_or_create(&path).unwrap();
        KeyStore::insert_key(&sf, &rich_key()).unwrap();
        sf.save(&path).unwrap();

        let reloaded = StateFile::load_or_create(&path).unwrap();
        let keys = reloaded.load_keys();
        let _ = std::fs::remove_file(&path);

        assert_eq!(keys.len(), 1);
        let k = &keys[0];
        assert_eq!(
            k.model_allowlist.as_deref(),
            Some(&["openai/gpt-4o-mini".to_string()][..]),
            "model allowlist must survive restart"
        );
        assert_eq!(
            k.tool_allowlist.as_deref(),
            Some(&["everything__echo".to_string()][..]),
            "tool allowlist must survive restart"
        );
        assert_eq!(k.limits.rpm, Some(60));
        assert_eq!(k.limits.tpm, Some(100_000));
        assert_eq!(k.max_budget, Some(Usd::from_dollars_f64(5.0)));
        assert_eq!(k.expires_at, Some(123));
    }

    #[test]
    fn legacy_state_file_without_policy_fields_loads() {
        // Pre-fix state files lacked the policy fields; they must still load via
        // serde defaults and come back unrestricted (not crash).
        let p: PersistedKey = serde_json::from_str(
            r#"{"id":"k","token_hash":"h","token_prefix":"ogw_k","max_budget_micros":1000,"expires_at":null,"revoked":false}"#,
        )
        .unwrap();
        let k = VirtualKey::from(&p);
        assert!(k.model_allowlist.is_none());
        assert!(k.tool_allowlist.is_none());
        assert!(k.limits.rpm.is_none());
        assert_eq!(k.max_budget, Some(Usd::from_micros(1000)));
    }
}
