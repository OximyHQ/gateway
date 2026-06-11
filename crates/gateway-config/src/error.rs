//! Config engine errors. Distinct variants give the CLI semantic exit codes
//! (design §7: AXI-grade CLI) — e.g. validation vs interpolation vs apply.

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config is invalid: {detail}")]
    Validation { detail: String },
    #[error("interpolation error: {detail}")]
    Interpolation { detail: String },
    #[error("config io error: {detail}")]
    Io { detail: String },
    #[error("config parse error: {detail}")]
    Parse { detail: String },
    #[error("apply failed: {detail}")]
    Apply { detail: String },
    #[error("crypto error: {detail}")]
    Crypto { detail: String },
    #[error("storage error: {detail}")]
    Storage { detail: String },
}
