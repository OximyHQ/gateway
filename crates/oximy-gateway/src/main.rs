//! # Oximy Gateway
//!
//! The unified, fastest, open-source LLM + MCP gateway. Single static binary,
//! embedded dashboard, agent-first control plane (CLI + admin-MCP + config-as-code).
//!
//! `oximy-gateway up` boots the gateway and opens the dashboard.
//!
//! See `docs/2026-06-10-oximy-gateway-design.md`.

#![forbid(unsafe_code)]

mod cli;
mod firstboot;
mod state_file;

use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;

use cli::{Cli, Command, KeysCommand, UpArgs};

/// Bundled model catalog in models.dev API snapshot format (5000+ models, 142 providers).
/// Embedded at compile time — no disk dependency for the base catalog.
const BUNDLED_MODELS_DEV: &str = include_str!("models-dev.json");

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("oximy-gateway {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Up(args) => match run_up(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("oximy-gateway up failed: {e:#}");
                ExitCode::from(70)
            }
        },
        Command::Keys(args) => match run_keys(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("oximy-gateway keys failed: {e:#}");
                ExitCode::from(70)
            }
        },
    }
}

/// Boot the gateway: resolve state dir → first-boot seed → register providers →
/// build AppState → start HTTP server → open browser.
fn run_up(args: UpArgs) -> anyhow::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(run_up_async(args))
}

async fn run_up_async(args: UpArgs) -> anyhow::Result<()> {
    use gateway_cache::build_registry_from_models_dev;
    use gateway_control::cache_handle::memory_cache_handle;
    use gateway_control::guard::default_chain;
    use gateway_control::keystore::{MutableKeyStore, PersistHook};
    use gateway_control::providers::{Deployment, ProviderRegistry};
    use gateway_control::state::AppState;
    use gateway_llm::Credentials;
    use gateway_llm::transports::openai::OpenAi;
    use gateway_spine::{MemoryAudit, SystemClock, Usd, VirtualKey};

    // ── 1. Resolve + create the state directory ───────────────────────────────
    let data_dir = cli::resolve_data_dir(args.dir.as_deref())?;
    std::fs::create_dir_all(&data_dir)?;
    let state_path = cli::state_path(&data_dir);
    tracing::info!(dir = %data_dir.display(), "data directory");

    // ── 2. Load or initialize the key store from the JSON state file ──────────
    let sf = Arc::new(state_file::StateFile::load_or_create(&state_path)?);

    // First boot: seed admin key, persist, print once.
    let clock = SystemClock;
    if let Some(minted) = firstboot::ensure_admin_key(sf.as_ref(), &clock)? {
        sf.save(&state_path)?;
        print_minted_key(&minted);
    } else if args.print_key {
        eprintln!(
            "An admin key already exists for this data dir ({}).\n\
             The secret is never recoverable; rotate it via the dashboard\n\
             or `oximy-gateway keys` CLI if lost.",
            data_dir.display()
        );
    }

    // ── 2b. Config file (light) ───────────────────────────────────────────────
    let config_path = data_dir.join("oximy-gateway.json");
    let config = load_or_seed_config(&config_path)?;

    // ── 3. Build the mutable, live key store with a file-persistence hook ─────
    // The hook is called after every `insert` / `revoke` to write the state file.
    struct FileHook {
        sf: Arc<state_file::StateFile>,
        path: std::path::PathBuf,
    }
    impl PersistHook for FileHook {
        fn persist(&self, keys: &[VirtualKey]) -> anyhow::Result<()> {
            // Re-populate the state file from the live key list.
            for k in keys {
                crate::firstboot::KeyStore::insert_key(self.sf.as_ref(), k)?;
            }
            self.sf.save(&self.path)
        }
    }

    let hook = Arc::new(FileHook {
        sf: Arc::clone(&sf),
        path: state_path.clone(),
    });
    let ks = Arc::new(MutableKeyStore::with_hook(hook));
    // Seed from the state file without calling the persist hook.
    ks.seed(sf.load_keys());

    // ── 4. Register LLM providers from env vars ───────────────────────────────
    let mut providers = ProviderRegistry::new();

    // OpenAI native
    if let Ok(api_key) = std::env::var("OPENAI_API_KEY")
        && !api_key.is_empty()
    {
        let mut creds = Credentials::new(api_key);
        if let Ok(base) = std::env::var("OPENAI_BASE_URL")
            && !base.is_empty()
        {
            creds = creds.with_base_url(base);
        }
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(OpenAi::new()),
                credentials: Arc::new(creds),
            },
        );
        tracing::info!("provider registered: openai");
    }

    // OpenRouter (OpenAI-compatible)
    if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY")
        && !api_key.is_empty()
    {
        providers.insert(
            "openrouter",
            Deployment {
                provider: Arc::new(OpenAi::new()),
                credentials: Arc::new(
                    Credentials::new(api_key).with_base_url("https://openrouter.ai/api"),
                ),
            },
        );
        tracing::info!("provider registered: openrouter (OpenAI-compatible)");
    }

    // Anthropic native
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY")
        && !api_key.is_empty()
    {
        use gateway_llm::transports::anthropic::Anthropic;
        providers.insert(
            "anthropic",
            Deployment {
                provider: Arc::new(Anthropic::new()),
                credentials: Arc::new(Credentials::new(api_key)),
            },
        );
        tracing::info!("provider registered: anthropic");
    }

    // Gemini native
    if let Ok(api_key) = std::env::var("GEMINI_API_KEY")
        && !api_key.is_empty()
    {
        use gateway_llm::transports::gemini::Gemini;
        providers.insert(
            "gemini",
            Deployment {
                provider: Arc::new(Gemini::new()),
                credentials: Arc::new(Credentials::new(api_key)),
            },
        );
        tracing::info!("provider registered: gemini");
    }

    // ── 4b. OpenAI-compatible provider presets ────────────────────────────────
    register_compat_provider(
        &mut providers,
        "GROQ_API_KEY",
        "groq",
        "https://api.groq.com/openai",
    );
    register_compat_provider(
        &mut providers,
        "TOGETHER_API_KEY",
        "together",
        "https://api.together.xyz",
    );
    register_compat_provider(
        &mut providers,
        "FIREWORKS_API_KEY",
        "fireworks",
        "https://api.fireworks.ai/inference",
    );
    register_compat_provider(
        &mut providers,
        "DEEPSEEK_API_KEY",
        "deepseek",
        "https://api.deepseek.com",
    );
    register_compat_provider(&mut providers, "XAI_API_KEY", "xai", "https://api.x.ai");
    register_compat_provider(
        &mut providers,
        "MISTRAL_API_KEY",
        "mistral",
        "https://api.mistral.ai",
    );
    register_compat_provider(
        &mut providers,
        "PERPLEXITY_API_KEY",
        "perplexity",
        "https://api.perplexity.ai",
    );
    register_compat_provider(
        &mut providers,
        "CEREBRAS_API_KEY",
        "cerebras",
        "https://api.cerebras.ai",
    );

    if providers.is_empty() {
        eprintln!(
            "  Warning: no provider API keys found. The server will start but\n\
             \x20          chat requests will fail until at least one key is set.\n\
             \x20          Supported env vars: OPENAI_API_KEY, ANTHROPIC_API_KEY,\n\
             \x20          GEMINI_API_KEY, OPENROUTER_API_KEY, GROQ_API_KEY,\n\
             \x20          TOGETHER_API_KEY, FIREWORKS_API_KEY, DEEPSEEK_API_KEY,\n\
             \x20          XAI_API_KEY, MISTRAL_API_KEY, PERPLEXITY_API_KEY, CEREBRAS_API_KEY."
        );
    }

    // ── 5. Spin up the telemetry writer ──────────────────────────────────────
    use gateway_telemetry::{
        DEFAULT_CHANNEL_CAPACITY, GatewayMetrics, MemorySpendStore, spawn as spawn_telemetry,
    };
    let metrics = Arc::new(GatewayMetrics::new());
    let spend_store = Arc::new(MemorySpendStore::new());
    let (telem_sink, _telem_writer) = spawn_telemetry(
        Arc::clone(&spend_store),
        Arc::clone(&metrics),
        DEFAULT_CHANNEL_CAPACITY,
    );

    // ── 6. Build the model registry from the bundled models.dev catalog ──────
    // User-override file: <data_dir>/models.json (flat-array format, merged over
    // the bundled catalog). Overrides win by id.
    let user_overrides_path = data_dir.join("models.json");
    let user_overrides_json: Option<String> = if user_overrides_path.exists() {
        match std::fs::read_to_string(&user_overrides_path) {
            Ok(s) => {
                tracing::info!(path = %user_overrides_path.display(), "loading user model overrides");
                Some(s)
            }
            Err(e) => {
                tracing::warn!(path = %user_overrides_path.display(), err = %e, "failed to read user model overrides; using bundled catalog only");
                None
            }
        }
    } else {
        None
    };

    let model_registry =
        build_registry_from_models_dev(BUNDLED_MODELS_DEV, user_overrides_json.as_deref())
            .map_err(|e| anyhow::anyhow!("failed to build model registry: {e}"))?;

    tracing::info!(
        models = model_registry.len(),
        "loaded models from models.dev catalog"
    );

    // ── 6a. Build AppState (with L1 cache pre-wired) ─────────────────────────
    let mut state_inner = AppState::with_parts_and_telemetry(
        ks,
        Arc::new(SystemClock),
        providers,
        Arc::new(default_chain()),
        Arc::new(MemoryAudit::new()),
        telem_sink,
        metrics,
        Arc::clone(&spend_store) as Arc<dyn gateway_telemetry::SpendStore>,
    );

    {
        let mut reg = state_inner.registry.write().unwrap();
        for entry in model_registry.all_entries() {
            reg.insert(entry);
        }
    }

    // ── 6b. Wire the L1 in-memory cache into AppState ────────────────────────
    state_inner.cache = Some(memory_cache_handle(SystemClock, 3600));

    let state = Arc::new(state_inner);

    // Set budget for all keys.
    for key in sf.load_keys() {
        state.ledger.set_budget(&key.id, key.max_budget, Usd::ZERO);
    }

    // ── 6c. Route overrides from OXIMY_ROUTES (JSON: model → Route) ──────────
    if let Ok(raw) = std::env::var("OXIMY_ROUTES")
        && !raw.trim().is_empty()
    {
        match serde_json::from_str::<std::collections::HashMap<String, gateway_route::Route>>(&raw)
        {
            Ok(routes) => {
                for (model, route) in routes {
                    tracing::info!(model = %model, targets = route.targets.len(), "route override installed");
                    state.set_route(model, route);
                }
            }
            Err(e) => {
                eprintln!("  Warning: OXIMY_ROUTES is not valid JSON ({e}); ignoring.");
            }
        }
    }

    // ── 6d. Route overrides from config file ─────────────────────────────────
    if let Some(cfg) = &config {
        apply_config(cfg, &state);
    }

    // ── 6e. Upstream MCP servers from OXIMY_MCP_SERVERS ──────────────────────
    if let Ok(raw) = std::env::var("OXIMY_MCP_SERVERS")
        && !raw.trim().is_empty()
    {
        register_mcp_servers(&state, &raw).await;
    }

    // ── 7. Build the app router: API + dashboard (mounted last) ───────────────
    let api = gateway_control::router(state);
    let app = api.merge(gateway_dash::dash_router());

    // ── 8. Bind + serve with graceful shutdown ────────────────────────────────
    let addr: std::net::SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    let host_display = display_host(&args.host);
    let base = format!("http://{host_display}:{}", bound.port());

    eprintln!(
        "\n  Oximy Gateway is running.\n\
         \n\
         \x20 Dashboard:  {base}/\n\
         \x20 API base:   {base}/v1\n\
         \x20 Health:     {base}/health\n\
         \x20 Models:     {base}/v1/models (auth required)\n"
    );
    tracing::info!(
        url = %format!("{base}/"),
        assets = gateway_dash::asset_count(),
        "gateway ready"
    );

    // Best-effort open the browser (ignore failure on headless servers).
    if !args.no_open && gateway_dash::index_present() {
        let _ = open::that(format!("{base}/"));
    }

    // Graceful shutdown on SIGINT / SIGTERM.
    let shutdown = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
            let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
            tokio::select! {
                _ = sigint.recv() => tracing::info!("SIGINT received — shutting down"),
                _ = sigterm.recv() => tracing::info!("SIGTERM received — shutting down"),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Ctrl-C received — shutting down");
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    tracing::info!("gateway stopped");
    Ok(())
}

/// Register an OpenAI-compatible provider when the given env key is set.
fn register_compat_provider(
    providers: &mut gateway_control::providers::ProviderRegistry,
    env_key: &str,
    provider_id: &'static str,
    base_url: &'static str,
) {
    use gateway_control::providers::Deployment;
    use gateway_llm::Credentials;
    use gateway_llm::transports::openai::OpenAi;

    if let Ok(api_key) = std::env::var(env_key)
        && !api_key.is_empty()
    {
        providers.insert(
            provider_id,
            Deployment {
                provider: Arc::new(OpenAi::new()),
                credentials: Arc::new(Credentials::new(api_key).with_base_url(base_url)),
            },
        );
        tracing::info!(
            provider_id,
            base_url,
            "provider registered (OpenAI-compatible)"
        );
    }
}

/// Light config file: load `oximy-gateway.json` from the data dir. On first boot
/// (no file exists) write a commented example. Config is additive — env still wins
/// for provider keys; config can add routes/model overrides.
fn load_or_seed_config(config_path: &std::path::Path) -> anyhow::Result<Option<FileConfig>> {
    if config_path.exists() {
        let text = std::fs::read_to_string(config_path)
            .map_err(|e| anyhow::anyhow!("reading config {}: {e}", config_path.display()))?;
        let cfg: FileConfig = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing config {}: {e}", config_path.display()))?;
        tracing::info!(path = %config_path.display(), "config file loaded");
        Ok(Some(cfg))
    } else {
        // Write an example config so the operator knows what's possible.
        let example = r#"{
  "_comment": "Oximy Gateway config — edit and restart to apply. All fields are optional.",
  "routes": {},
  "model_overrides": []
}
"#;
        if let Err(e) = std::fs::write(config_path, example) {
            tracing::warn!(path = %config_path.display(), err = %e, "could not write example config");
        } else {
            tracing::info!(path = %config_path.display(), "wrote example config");
        }
        Ok(None)
    }
}

/// Minimal config file schema — only what we act on here. Other fields (providers,
/// keys) are ignored; they are managed by env vars / the `keys` CLI.
#[derive(serde::Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    routes: std::collections::HashMap<String, FileRoute>,
    #[serde(default)]
    model_overrides: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct FileRoute {
    targets: Vec<FileRouteTarget>,
    #[serde(default = "default_strategy")]
    strategy: String,
}

fn default_strategy() -> String {
    "failover".into()
}

#[derive(serde::Deserialize)]
struct FileRouteTarget {
    provider_id: String,
    model: String,
}

/// Apply config-file routes and model overrides to an already-built AppState.
fn apply_config<C: gateway_spine::Clock + 'static>(
    cfg: &FileConfig,
    state: &gateway_control::state::AppState<C>,
) {
    use gateway_cache::build_registry_from_models_dev;

    for (model_id, file_route) in &cfg.routes {
        // Skip comment keys (keys starting with "_")
        if model_id.starts_with('_') {
            continue;
        }
        let targets: Vec<gateway_route::RouteTarget> = file_route
            .targets
            .iter()
            .map(|t| gateway_route::RouteTarget::new(&t.provider_id, &t.model))
            .collect();
        if targets.is_empty() {
            continue;
        }
        let strategy = match file_route.strategy.as_str() {
            "weighted" => gateway_route::Strategy::Weighted,
            "latency_aware" | "latency-aware" => gateway_route::Strategy::LatencyAware,
            _ => gateway_route::Strategy::Failover,
        };
        let route = gateway_route::Route::new(targets, strategy);
        tracing::info!(model = %model_id, strategy = %file_route.strategy, "config route installed");
        state.set_route(model_id.clone(), route);
    }

    if !cfg.model_overrides.is_empty() {
        let overrides_json = match serde_json::to_string(&cfg.model_overrides) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(err = %e, "could not serialize config model_overrides; skipping");
                return;
            }
        };
        // Re-build from the bundled catalog + config overrides and merge into state registry.
        match build_registry_from_models_dev(BUNDLED_MODELS_DEV, Some(&overrides_json)) {
            Ok(reg) => {
                let mut state_reg = state.registry.write().unwrap();
                for entry in reg.all_entries() {
                    state_reg.insert(entry);
                }
                tracing::info!(
                    count = cfg.model_overrides.len(),
                    "config model overrides applied"
                );
            }
            Err(e) => {
                tracing::warn!(err = %e, "config model_overrides parse error; skipping");
            }
        }
    }
}

/// Parse `OXIMY_MCP_SERVERS` (JSON array) and register + refresh each upstream
/// MCP server on the federation.
async fn register_mcp_servers<C: gateway_spine::Clock + 'static>(
    state: &gateway_control::state::AppState<C>,
    raw: &str,
) {
    use gateway_mcp::{HttpTransport, McpServer, StdioTransport};

    #[derive(serde::Deserialize)]
    struct McpServerCfg {
        name: String,
        url: Option<String>,
        command: Option<String>,
        #[serde(default)]
        args: Vec<String>,
    }

    let cfgs: Vec<McpServerCfg> = match serde_json::from_str(raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  Warning: OXIMY_MCP_SERVERS is not valid JSON ({e}); ignoring.");
            return;
        }
    };

    for cfg in cfgs {
        let server = if let Some(url) = cfg.url {
            McpServer::new(
                cfg.name.clone(),
                Arc::new(HttpTransport::new(cfg.name.clone(), url)),
            )
        } else if let Some(command) = cfg.command {
            let mut cmd = tokio::process::Command::new(&command);
            cmd.args(&cfg.args);
            match StdioTransport::spawn(cfg.name.clone(), &mut cmd).await {
                Ok(t) => McpServer::new(cfg.name.clone(), Arc::new(t)),
                Err(e) => {
                    eprintln!("  Warning: MCP server '{}' failed to spawn: {e}", cfg.name);
                    continue;
                }
            }
        } else {
            eprintln!(
                "  Warning: MCP server '{}' has neither 'url' nor 'command'; skipping.",
                cfg.name
            );
            continue;
        };

        state.federation.register_server(server).await;
        match state.federation.refresh_server(&cfg.name).await {
            Ok(names) => {
                tracing::info!(server = %cfg.name, tools = names.len(), "MCP server registered");
            }
            Err(e) => {
                eprintln!(
                    "  Warning: MCP server '{}' tool refresh failed: {e}",
                    cfg.name
                );
            }
        }
    }
}

// ── `keys` subcommand implementation ─────────────────────────────────────────

fn run_keys(args: cli::KeysArgs) -> anyhow::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(run_keys_async(args))
}

async fn run_keys_async(args: cli::KeysArgs) -> anyhow::Result<()> {
    use gateway_spine::{Clock, RateLimits, SystemClock, Usd, VirtualKey};

    let data_dir = cli::resolve_data_dir(args.dir.as_deref())?;
    std::fs::create_dir_all(&data_dir)?;
    let state_path = cli::state_path(&data_dir);
    let sf = state_file::StateFile::load_or_create(&state_path)?;

    match args.subcommand {
        KeysCommand::Create {
            name,
            budget_usd,
            models,
        } => {
            let clock = SystemClock;
            let ts = clock.now_ms();

            // Generate a secret.
            let secret = firstboot::generate_secret();
            let key_id = format!(
                "key_{}",
                name.as_deref()
                    .map(|n| n.replace(' ', "_"))
                    .unwrap_or_else(|| format!("user_{ts}"))
            );
            let token_prefix: String = secret.chars().take(12).collect();

            let max_budget = budget_usd.map(Usd::from_dollars_f64);
            let model_allowlist = models.map(|m| {
                m.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            });

            let key = VirtualKey {
                id: key_id.clone(),
                token_hash: VirtualKey::hash_secret(&secret),
                token_prefix: token_prefix.clone(),
                max_budget,
                limits: RateLimits::default(),
                model_allowlist,
                expires_at: None,
                revoked: false,
                parent_id: None,
            };

            crate::firstboot::KeyStore::insert_key(&sf, &key)?;
            sf.save(&state_path)?;

            println!("\n  Key created successfully!");
            println!("  ID:     {key_id}");
            println!("  Prefix: {token_prefix}");
            if let Some(b) = budget_usd {
                println!("  Budget: ${b:.2}");
            } else {
                println!("  Budget: unlimited");
            }
            println!(
                "\n  Secret (shown ONCE — store it now):\n\n    {secret}\n\n  \
                 Use as: Authorization: Bearer {secret}\n"
            );
        }

        KeysCommand::List => {
            let keys = sf.load_keys();
            if keys.is_empty() {
                println!("No keys found in {}", data_dir.display());
                return Ok(());
            }
            println!(
                "\n  {:<35} {:<14} {:<12} {:<10} MODELS",
                "ID", "PREFIX", "BUDGET", "REVOKED"
            );
            println!("  {}", "-".repeat(85));
            for k in keys {
                let budget = match k.max_budget {
                    Some(b) => format!("${:.4}", b.as_dollars_f64()),
                    None => "unlimited".into(),
                };
                let models = match &k.model_allowlist {
                    Some(list) => list.join(","),
                    None => "all".into(),
                };
                println!(
                    "  {:<35} {:<14} {:<12} {:<10} {}",
                    k.id,
                    k.token_prefix,
                    budget,
                    if k.revoked { "yes" } else { "no" },
                    models
                );
            }
            println!();
        }

        KeysCommand::Revoke { id } => {
            sf.revoke_key(&id)?;
            sf.save(&state_path)?;
            println!("Key '{id}' revoked and persisted.");
        }
    }

    Ok(())
}

/// Print the freshly minted admin secret exactly once.
fn print_minted_key(minted: &firstboot::MintedKey) {
    eprintln!(
        "\n  \u{250c}\u{2500} First boot \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\
          \u{2502}  A default admin key was created. It is shown ONCE:\n\
          \u{2502}\n\
          \u{2502}     {secret}\n\
          \u{2502}\n\
          \u{2502}  Use it as your Bearer token for the API and dashboard.\n\
          \u{2502}  Store it now \u{2014} it cannot be recovered.\n\
          \u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n",
        secret = minted.secret
    );
}

/// For display, show 127.0.0.1 even when bound to 0.0.0.0.
fn display_host(host: &str) -> &str {
    if host == "0.0.0.0" { "127.0.0.1" } else { host }
}
