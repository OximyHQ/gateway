#![forbid(unsafe_code)]

mod error;
mod store;
mod types;

pub use error::StoreError;
pub use store::Store;
pub use types::{ReservationId, StoredKey};
