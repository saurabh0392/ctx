//! ctx library surface for integration tests and the `ctx` binary.

pub mod ab;
pub mod allowance;
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
pub mod dashboard_push;
pub mod daemon;
pub mod db;
pub mod embedder;
pub mod filter;
pub mod filter_control;
pub mod filter_hook;
pub mod host;
pub mod hook;
pub mod inject;
pub mod profiles;
pub mod semantic_tools;
pub mod proxy;
pub mod quality_guard;
pub mod setup;
pub mod test_lock;
pub mod mcp;
pub mod modes;
pub mod simulate;
pub mod socket;
pub mod tuning;
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
use cli::{
    Cli, Commands, ExperimentCommand, FilterCommand, HookCommand, InjectCommand, ModeCommand,
    ProfileCommand, ProxyCommand,
};

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
            ProfileCommand::Add { name, keep, keep_tool } => {
                profiles::add(
                    &name,
                    keep.unwrap_or_default(),
                    keep_tool.unwrap_or_default(),
                )?
            }
            ProfileCommand::Remove { name } => profiles::remove(&name)?,
            ProfileCommand::Auto { refresh } => profiles::auto_generate(refresh)?,
            ProfileCommand::Generate => profiles::generate_from_config(true)?,
            ProfileCommand::MigrateTools { name, force } => {
                profiles::migrate_tools(name.as_deref(), force)?
            }
        },
        Commands::Proxy { command } => match command {
            ProxyCommand::Start { port, upstream } => proxy::start(port, &upstream).await?,
            ProxyCommand::Install { mode, port, upstream } => {
                let mode = crate::config::ProxyMode::parse(&mode)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Invalid --mode {mode:?}; use complement, standalone, or filter-only"
                    ))?;
                proxy::install(port, &upstream, mode)?;
            }
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
        Commands::Mode { name, command } => match command {
            Some(ModeCommand::List) => {
                let cfg = config::Config::load();
                let names = modes::list_modes(&cfg);
                if names.is_empty() {
                    println!("No modes configured. Add [modes.debug] to ~/.ctx/config.toml");
                } else {
                    for n in names {
                        println!("{n}");
                    }
                }
            }
            Some(ModeCommand::Show { name }) => {
                let cfg = config::Config::load();
                modes::show_mode(&cfg, &name)?;
            }
            Some(ModeCommand::Save { name }) => modes::save_current_as_mode(&name)?,
            None => {
                let mode_name = name.ok_or_else(|| {
                    anyhow::anyhow!("usage: ctx mode <name> | list | show <name> | save <name>")
                })?;
                modes::switch_mode(&mode_name)?;
            }
        },
        Commands::Simulate {
            prompt,
            cwd,
            session,
            profile,
            all_profiles,
            replay_last,
            json,
        } => {
            if let Some(n) = replay_last {
                let comparisons = simulate::replay_last_traces(n)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&comparisons)?);
                } else {
                    print!("{}", simulate::format_replay(&comparisons));
                }
            } else {
                let effective_cwd = cwd.unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| ".".to_string())
                });
                let effective_prompt = prompt.unwrap_or_else(|| {
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
                    buf
                });
                if all_profiles {
                    let results = simulate::simulate_all_profiles(
                        &effective_cwd,
                        &effective_prompt,
                        session.as_deref(),
                        None,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&results)?);
                    } else {
                        print!(
                            "{}",
                            simulate::format_all_profiles(&results, &effective_cwd, &effective_prompt)
                        );
                    }
                } else {
                    let result = simulate::simulate_pipeline(
                        &effective_cwd,
                        &effective_prompt,
                        session.as_deref(),
                        None,
                        profile.as_deref(),
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        print!("{}", simulate::format_result(&result));
                    }
                }
            }
        }
        Commands::Experiment { command } => match command {
            ExperimentCommand::Status => tuning::print_experiment_status()?,
            ExperimentCommand::Apply => tuning::apply_recommendations()?,
            ExperimentCommand::Reset => tuning::reset_experiment()?,
        },
        Commands::Filter { command } => match command {
            FilterCommand::Mode { mode } => {
                let fm = crate::config::FilterMode::parse(&mode).ok_or_else(|| {
                    anyhow::anyhow!("unknown filter mode '{mode}' (use soft, strict, or off)")
                })?;
                filter_control::set_filter_mode(fm)?;
            }
            FilterCommand::Expand { target } => filter_control::expand_session_target(&target)?,
            FilterCommand::ClearExpansion => filter_control::clear_session_expansion()?,
        },
    }

    Ok(())
}
