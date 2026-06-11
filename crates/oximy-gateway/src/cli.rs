//! The CLI surface. `up` is the zero-config command. Data-dir resolution:
//! `--dir` flag > `$OXIMY_GATEWAY_DIR` > the platform data dir
//! (`~/.local/share/oximy-gateway` on Linux, `~/Library/Application Support/...`
//! on macOS). The SQLite-style state file lives at `<dir>/gateway.json`.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "oximy-gateway",
    version,
    about = "Unified LLM + MCP gateway — oximy-gateway up to boot"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Boot the gateway and open the dashboard (zero-config on first run).
    ///
    /// On first run: creates the state directory, generates an admin key,
    /// prints it once, and starts the server. On subsequent runs: loads the
    /// existing state and starts normally.
    Up(UpArgs),
    /// Print the version.
    Version,
    /// Manage virtual API keys.
    Keys(KeysArgs),
}

#[derive(Debug, clap::Args, Clone)]
pub struct UpArgs {
    /// Port to bind the server.
    #[arg(long, env = "OXIMY_GATEWAY_PORT", default_value_t = 8080)]
    pub port: u16,
    /// Host/interface to bind (default 127.0.0.1 — local only).
    #[arg(long, env = "OXIMY_GATEWAY_HOST", default_value = "127.0.0.1")]
    pub host: String,
    /// Data directory (overrides $OXIMY_GATEWAY_DIR and the platform default).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// Do not open the dashboard in a browser (useful on servers/CI).
    #[arg(long)]
    pub no_open: bool,
    /// Confirm an admin key exists (never re-reveals the secret).
    #[arg(long)]
    pub print_key: bool,
}

/// `oximy-gateway keys <subcommand>` — manage virtual API keys offline (without
/// a running server). All operations work against the same state file the server
/// uses, so a key minted here is valid against a running server using the same
/// data dir.
#[derive(Debug, clap::Args)]
pub struct KeysArgs {
    /// Data directory (must match the running server's --dir).
    #[arg(long)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub subcommand: KeysCommand,
}

#[derive(Debug, Subcommand)]
pub enum KeysCommand {
    /// Mint a new virtual key. The secret is printed once and NOT stored.
    Create {
        /// Human-readable name (used as part of the key id).
        #[arg(long)]
        name: Option<String>,
        /// Spending budget in USD (e.g. 5.0). Omit for unlimited.
        #[arg(long)]
        budget_usd: Option<f64>,
        /// Comma-separated model allowlist (e.g. gpt-4o,claude-3-5-sonnet-20241022).
        /// Omit to allow all models.
        #[arg(long)]
        models: Option<String>,
    },
    /// List all keys in the store (ids, prefixes, budget, revoked status).
    List,
    /// Revoke a key by id. Revoked keys are rejected by the server on next restart.
    Revoke {
        /// The key id to revoke (from `keys list`).
        id: String,
    },
}

/// Resolve the data directory: `--dir` > `$OXIMY_GATEWAY_DIR` > platform default.
pub fn resolve_data_dir(flag: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("OXIMY_GATEWAY_DIR")
        && !env.is_empty()
    {
        return Ok(PathBuf::from(env));
    }
    let proj = directories::ProjectDirs::from("com", "oximy", "oximy-gateway")
        .ok_or_else(|| anyhow::anyhow!("could not resolve a platform data directory"))?;
    Ok(proj.data_dir().to_path_buf())
}

/// The state file path inside a data dir.
pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_dir_flag_wins() {
        let p = PathBuf::from("/tmp/explicit");
        let resolved = resolve_data_dir(Some(&p)).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn state_path_is_under_data_dir() {
        let dir = PathBuf::from("/var/lib/oximy");
        assert_eq!(
            state_path(&dir),
            PathBuf::from("/var/lib/oximy/gateway.json")
        );
    }

    #[test]
    fn up_args_default_port_and_host() {
        let cli = Cli::parse_from(["oximy-gateway", "up"]);
        match cli.command {
            Command::Up(args) => {
                assert_eq!(args.port, 8080);
                assert_eq!(args.host, "127.0.0.1");
                assert!(!args.no_open);
            }
            _ => panic!("expected up"),
        }
    }

    #[test]
    fn up_flags_parse() {
        let cli = Cli::parse_from([
            "oximy-gateway",
            "up",
            "--port",
            "9090",
            "--host",
            "0.0.0.0",
            "--no-open",
        ]);
        match cli.command {
            Command::Up(args) => {
                assert_eq!(args.port, 9090);
                assert_eq!(args.host, "0.0.0.0");
                assert!(args.no_open);
            }
            _ => panic!("expected up"),
        }
    }

    #[test]
    fn keys_create_parses() {
        let cli = Cli::parse_from([
            "oximy-gateway",
            "keys",
            "create",
            "--name",
            "test",
            "--budget-usd",
            "5.0",
            "--models",
            "gpt-4o,claude-3-5-sonnet-20241022",
        ]);
        match cli.command {
            Command::Keys(args) => match args.subcommand {
                KeysCommand::Create {
                    name,
                    budget_usd,
                    models,
                } => {
                    assert_eq!(name.as_deref(), Some("test"));
                    assert!((budget_usd.unwrap() - 5.0).abs() < 1e-9);
                    assert_eq!(models.as_deref(), Some("gpt-4o,claude-3-5-sonnet-20241022"));
                }
                _ => panic!("expected create"),
            },
            _ => panic!("expected keys"),
        }
    }

    #[test]
    fn keys_list_parses() {
        let cli = Cli::parse_from(["oximy-gateway", "keys", "list"]);
        assert!(matches!(
            cli.command,
            Command::Keys(KeysArgs {
                subcommand: KeysCommand::List,
                ..
            })
        ));
    }

    #[test]
    fn keys_revoke_parses() {
        let cli = Cli::parse_from(["oximy-gateway", "keys", "revoke", "key_admin_123"]);
        match cli.command {
            Command::Keys(args) => match args.subcommand {
                KeysCommand::Revoke { id } => assert_eq!(id, "key_admin_123"),
                _ => panic!("expected revoke"),
            },
            _ => panic!("expected keys"),
        }
    }
}
