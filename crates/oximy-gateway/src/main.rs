//! # Oximy Gateway
//!
//! The unified, fastest, open-source LLM + MCP gateway. Single static binary,
//! embedded dashboard, agent-first control plane (CLI + admin-MCP + config-as-code).
//!
//! `oximy-gateway up` boots the gateway and opens the dashboard.
//!
//! See `docs/2026-06-10-oximy-gateway-design.md`. Status: **scaffold** — the
//! command surface below is a skeleton; behavior lands per the Phase plans.

#![forbid(unsafe_code)]

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");

    match cmd {
        "up" | "serve" => {
            tracing::info!(
                version = VERSION,
                "oximy-gateway: scaffold — `up` not yet implemented"
            );
            eprintln!(
                "oximy-gateway {VERSION}: scaffold. `up` will boot the gateway + dashboard once Phase 1 lands."
            );
            ExitCode::SUCCESS
        }
        "version" | "--version" | "-V" => {
            println!("oximy-gateway {VERSION}");
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print_help();
            // Semantic exit code: usage error.
            ExitCode::from(64)
        }
    }
}

fn print_help() {
    println!(
        "oximy-gateway {VERSION} — unified LLM + MCP gateway\n\
         \n\
         USAGE:\n\
         \toximy-gateway <command> [--json]\n\
         \n\
         COMMANDS (scaffold):\n\
         \tup            Boot the gateway and open the dashboard\n\
         \tversion       Print the version\n\
         \thelp          Show this help\n\
         \n\
         Planned: keys, budgets, config (dump/diff/apply), mcp, providers, logs.\n\
         Docs: https://github.com/oximyhq/gateway"
    );
}
