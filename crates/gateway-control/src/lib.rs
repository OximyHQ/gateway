//! # gateway-control
//!
//! The HTTP ingress + per-request governance lifecycle over the spine. Three
//! thin clients (REST API, admin-MCP, CLI) share one core; P1.4 ships the REST
//! data-plane (`/v1/*`) and the lifecycle that wires the spine (auth, budgets,
//! rate limits, audit) to the `gateway-llm` egress (streaming, idempotency).
//! Admin CRUD / admin-MCP / CLI land in P1.6 and P3.
//!
//! Part of [Oximy Gateway](https://github.com/oximyhq/gateway). See
//! `docs/2026-06-10-oximy-gateway-design.md` (§2 invariants, §6 lifecycle).

#![forbid(unsafe_code)]

pub mod auth;
pub mod error;
pub mod gateway;
pub mod guard;
pub mod keystore;
pub mod mcp;
pub mod providers;
pub mod route_exec;
pub mod server;
pub mod sse_out;
pub mod state;
pub mod wire;

pub use auth::{authenticate, parse_bearer};
pub use error::GatewayError;
pub use gateway::{Completed, CompletedStream, Gateway};
pub use guard::{AllowAll, GuardHook, GuardVerdict};
pub use keystore::{KeyStore, StaticKeyStore};
pub use providers::{Deployment, ProviderRegistry};
pub use route_exec::RegistryExecutor;
pub use server::{router, serve};
pub use sse_out::{delta_to_sse, done_event};
pub use state::AppState;
pub use wire::{WireChatRequest, WireChatResponse};
