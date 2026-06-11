#![forbid(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(String),
    #[error("not found")]
    NotFound,
    #[error(
        "budget exceeded: budget={budget_micros}, spent={spent_micros}, reserved={reserved_micros}, requested={requested_micros}"
    )]
    BudgetExceeded {
        budget_micros: i64,
        spent_micros: i64,
        reserved_micros: i64,
        requested_micros: i64,
    },
    #[error("migration error: {0}")]
    Migration(String),
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => StoreError::NotFound,
            e => StoreError::Db(e.to_string()),
        }
    }
}

impl From<StoreError> for gateway_spine::SpineError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::BudgetExceeded {
                budget_micros,
                spent_micros,
                reserved_micros,
                requested_micros,
            } => gateway_spine::SpineError::BudgetExceeded {
                key_id: String::new(),
                would_spend_micros: spent_micros + reserved_micros + requested_micros,
                budget_micros,
            },
            StoreError::NotFound => gateway_spine::SpineError::NoSuchKey {
                key_id: String::new(),
            },
            StoreError::Db(msg) | StoreError::Migration(msg) => {
                // Map to a budget-exceeded with a descriptive key_id as best we can
                // since SpineError has no generic Internal variant
                gateway_spine::SpineError::NoSuchKey { key_id: msg }
            }
        }
    }
}
