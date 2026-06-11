#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub type ReservationId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredKey {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub budget_micros: Option<i64>,
    pub spent_micros: i64,
    pub rpm: Option<i64>,
    pub tpm: Option<i64>,
    pub max_parallel: Option<i64>,
    pub model_allowlist: Option<Vec<String>>,
    pub expires_at_ms: Option<i64>,
    pub revoked: bool,
    pub parent_id: Option<String>,
    pub created_at_ms: i64,
}
