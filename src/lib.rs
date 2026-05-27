//! ctx library surface for integration tests and the `ctx` binary.

pub mod analytics;
pub mod adaptive;
pub mod ca;
pub mod behavior_guard;
pub mod budget_guard;
pub mod claude_settings;
pub mod cli;
pub mod coach;
pub mod config;
pub mod conversations;
pub mod dashboard;
pub mod daemon;
pub mod db;
pub mod embedder;
pub mod filter;
pub mod filter_hook;
pub mod host;
pub mod hook;
pub mod inject;
pub mod profiles;
pub mod proxy;
pub mod quality_guard;
pub mod setup;
pub mod test_lock;
pub mod mcp;
pub mod user_profile;

/// Install rustls ring crypto provider once (required for [`ca::CertAuthority`]).
pub fn ensure_tls_crypto_provider() {
    use std::sync::OnceLock;
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, HookCommand, InjectCommand, ProfileCommand, ProxyCommand};

pub async fn run() -> Result<()> {
    ensure_tls_crypto_provider();
    let cli = Cli::parse();

    match cli.command {
        Commands::Use { profile, force } => profiles::switch(&profile, force)?,
        Commands::Ingest => {
            let n = conversations::ingest_claude_jsonl()?;
            println!("Ingested {n} session file(s) into ~/.ctx/ctx.db (Claude Code + Desktop)");
        }
        Commands::Status => profiles::status()?,
        Commands::Gain { brief } => {
            if brief {
                analytics::show_brief()?;
            } else {
                analytics::show()?;
            }
        }
        Commands::Profile { command } => match command {
            ProfileCommand::List => profiles::list()?,
            ProfileCommand::Show { name } => profiles::show(&name)?,
            ProfileCommand::Add { name, keep } => profiles::add(&name, keep)?,
            ProfileCommand::Remove { name } => profiles::remove(&name)?,
            ProfileCommand::Auto { refresh } => profiles::auto_generate(refresh)?,
            ProfileCommand::Generate => profiles::generate_from_config()?,
        },
        Commands::Proxy { command } => match command {
            ProxyCommand::Start { port, upstream } => proxy::start(port, &upstream).await?,
            ProxyCommand::Install { port, upstream } => proxy::install(port, &upstream)?,
            ProxyCommand::Uninstall => proxy::uninstall()?,
            ProxyCommand::Status => proxy::status()?,
        },
        Commands::Inject { command } => match command {
            InjectCommand::Show => inject::show()?,
            InjectCommand::Edit => inject::edit()?,
            InjectCommand::Disable => inject::disable()?,
            InjectCommand::Enable => inject::enable()?,
        },
        Commands::Setup {
            port,
            upstream,
            uninstall,
            no_install,
            no_zshrc_prompt,
            dry_run,
            yes,
        } => {
            if uninstall {
                setup::uninstall()?;
            } else {
                setup::run(port, &upstream, no_install, no_zshrc_prompt, dry_run, yes)?;
            }
        }
        Commands::Dashboard { port, no_open } => dashboard::serve(port, no_open).await?,
        Commands::Hook { command } => match command {
            HookCommand::UserPromptSubmit => hook::user_prompt_submit()?,
        },
        Commands::Mcp => mcp::serve_stdio()?,
    }

    Ok(())
}
