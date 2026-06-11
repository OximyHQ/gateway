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
            limits: RateLimits::default(),
            model_allowlist: None,
            expires_at: p.expires_at,
            revoked: p.revoked,
            parent_id: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StateFileData {
    pub keys: HashMap<String, PersistedKey>,
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
