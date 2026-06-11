//! Egress provider transports — one module per provider API shape. Each owns its
//! wire structs PRIVATELY and exposes only the unified `Provider` impl.

pub mod anthropic;
pub mod gemini;
pub mod openai;
