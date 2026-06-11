//! The spine's error taxonomy. These map to HTTP statuses at the server layer
//! (P1.4): BudgetExceeded/RateLimited → 429, Key* → 401/403, ModelNotAllowed →
//! 403, UnknownModel → 400.

use crate::money::Usd;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RateDimension {
    Requests,
    Tokens,
    Parallel,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SpineError {
    #[error(
        "budget exceeded for key {key_id}: would spend {would_spend_micros} µUSD of {budget_micros} µUSD"
    )]
    BudgetExceeded {
        key_id: String,
        would_spend_micros: i64,
        budget_micros: i64,
    },
    #[error("rate limit exceeded for key {key_id}: {dimension:?}")]
    RateLimited {
        key_id: String,
        dimension: RateDimension,
    },
    #[error("key {key_id} is revoked")]
    KeyRevoked { key_id: String },
    #[error("key {key_id} has expired")]
    KeyExpired { key_id: String },
    #[error("model {model} is not allowed for key {key_id}")]
    ModelNotAllowed { key_id: String, model: String },
    #[error("unknown model: {model}")]
    UnknownModel { model: String },
    #[error("no such reservation")]
    NoSuchReservation,
    #[error("no such key: {key_id}")]
    NoSuchKey { key_id: String },
}

impl SpineError {
    pub fn budget_exceeded(key_id: &str, would_spend: Usd, budget: Usd) -> Self {
        SpineError::BudgetExceeded {
            key_id: key_id.to_string(),
            would_spend_micros: would_spend.micros(),
            budget_micros: budget.micros(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_exceeded_constructor_and_display() {
        let e = SpineError::budget_exceeded(
            "k1",
            Usd::from_micros(1_100_000),
            Usd::from_micros(1_000_000),
        );
        assert!(matches!(e, SpineError::BudgetExceeded { .. }));
        assert!(e.to_string().contains("1100000 µUSD of 1000000"));
    }

    #[test]
    fn rate_dimension_in_message() {
        let e = SpineError::RateLimited {
            key_id: "k".into(),
            dimension: RateDimension::Tokens,
        };
        assert!(e.to_string().contains("Tokens"));
    }
}
