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

use cli::{Cli, Command, UpArgs};

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
                ExitCode::from(70) // Semantic: runtime failure (not a usage error).
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
    use gateway_control::guard::AllowAll;
    use gateway_control::keystore::StaticKeyStore;
    use gateway_control::providers::{Deployment, ProviderRegistry};
    use gateway_control::state::AppState;
    use gateway_llm::Credentials;
    use gateway_spine::{MemoryAudit, ModelEntry, ModelPrice, SystemClock, Usd};

    // ── 1. Resolve + create the state directory ───────────────────────────────
    let data_dir = cli::resolve_data_dir(args.dir.as_deref())?;
    std::fs::create_dir_all(&data_dir)?;
    let state_path = cli::state_path(&data_dir);
    tracing::info!(dir = %data_dir.display(), "data directory");

    // ── 2. Load or initialize the key store from the JSON state file ──────────
    let sf = state_file::StateFile::load_or_create(&state_path)?;

    // First boot: seed admin key, persist, print once.
    let clock = SystemClock;
    if let Some(minted) = firstboot::ensure_admin_key(&sf, &clock)? {
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

    // ── 3. Build the in-memory key store ─────────────────────────────────────
    let mut ks = StaticKeyStore::new();
    for key in sf.load_keys() {
        ks.insert(key);
    }

    // ── 4. Register LLM providers from env vars ───────────────────────────────
    let mut providers = ProviderRegistry::new();

    if let Ok(api_key) = std::env::var("OPENAI_API_KEY")
        && !api_key.is_empty()
    {
        use gateway_llm::transports::openai::OpenAi;
        let mut creds = Credentials::new(api_key);
        // Optional base-URL override (Azure / self-hosted / OpenAI-compatible proxy).
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

    // OpenRouter is OpenAI-compatible: reuse the OpenAI transport with its base URL.
    if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY")
        && !api_key.is_empty()
    {
        use gateway_llm::transports::openai::OpenAi;
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

    if providers.get("openai").is_none()
        && providers.get("anthropic").is_none()
        && providers.get("gemini").is_none()
        && providers.get("openrouter").is_none()
    {
        eprintln!(
            "  Warning: no provider API keys found (OPENAI_API_KEY, ANTHROPIC_API_KEY,\n\
             \x20          GEMINI_API_KEY, OPENROUTER_API_KEY). The server will start but\n\
             \x20          chat requests will fail until at least one key is set."
        );
    }

    // ── 5. Build AppState with hardcoded model price entries ─────────────────
    let state = Arc::new(AppState::with_parts(
        Arc::new(ks),
        Arc::new(SystemClock),
        providers,
        Arc::new(AllowAll),
        Arc::new(MemoryAudit::new()),
    ));

    {
        let mut reg = state.registry.write().unwrap();
        // gpt-4o: $2.50/MTok input, $10/MTok output
        reg.insert(ModelEntry {
            id: "gpt-4o".into(),
            provider: "openai".into(),
            price: ModelPrice {
                input_per_mtok: 2_500_000,
                output_per_mtok: 10_000_000,
                cache_read_per_mtok: 1_250_000,
                cache_write_per_mtok: 0,
            },
            context_window: Some(128_000),
            max_output_tokens: Some(16_384),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        });
        // gpt-4o-mini: $0.15/MTok input, $0.60/MTok output
        reg.insert(ModelEntry {
            id: "gpt-4o-mini".into(),
            provider: "openai".into(),
            price: ModelPrice {
                input_per_mtok: 150_000,
                output_per_mtok: 600_000,
                cache_read_per_mtok: 75_000,
                cache_write_per_mtok: 0,
            },
            context_window: Some(128_000),
            max_output_tokens: Some(16_384),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        });
        // claude-3-5-sonnet-20241022: $3/MTok input, $15/MTok output
        reg.insert(ModelEntry {
            id: "claude-3-5-sonnet-20241022".into(),
            provider: "anthropic".into(),
            price: ModelPrice {
                input_per_mtok: 3_000_000,
                output_per_mtok: 15_000_000,
                cache_read_per_mtok: 300_000,
                cache_write_per_mtok: 3_750_000,
            },
            context_window: Some(200_000),
            max_output_tokens: Some(8_192),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        });
        // gemini-1.5-pro: $1.25/MTok input, $5/MTok output
        reg.insert(ModelEntry {
            id: "gemini-1.5-pro".into(),
            provider: "gemini".into(),
            price: ModelPrice {
                input_per_mtok: 1_250_000,
                output_per_mtok: 5_000_000,
                cache_read_per_mtok: 312_500,
                cache_write_per_mtok: 0,
            },
            context_window: Some(2_000_000),
            max_output_tokens: Some(8_192),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        });
        // OpenRouter (OpenAI-compatible) — current models reachable via one key.
        for (id, input_per_mtok, output_per_mtok) in [
            ("openai/gpt-4o-mini", 150_000_i64, 600_000_i64),
            ("openai/gpt-4o", 2_500_000, 10_000_000),
            ("anthropic/claude-3.5-haiku", 800_000, 4_000_000),
            ("deepseek/deepseek-chat", 280_000, 880_000),
            ("meta-llama/llama-3.3-70b-instruct", 120_000, 300_000),
        ] {
            reg.insert(ModelEntry {
                id: id.into(),
                provider: "openrouter".into(),
                price: ModelPrice {
                    input_per_mtok,
                    output_per_mtok,
                    cache_read_per_mtok: 0,
                    cache_write_per_mtok: 0,
                },
                context_window: Some(128_000),
                max_output_tokens: Some(8_192),
                supports_tools: true,
                supports_vision: false,
                supports_streaming: true,
            });
        }
    }

    // Set unlimited budget for the admin key.
    for key in sf.load_keys() {
        state.ledger.set_budget(&key.id, key.max_budget, Usd::ZERO);
    }

    // ── 6. Build the app router: API + dashboard (mounted last) ───────────────
    let api = gateway_control::router(state);
    let app = api.merge(gateway_dash::dash_router());

    // ── 7. Bind + serve ───────────────────────────────────────────────────────
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

    axum::serve(listener, app).await?;
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
