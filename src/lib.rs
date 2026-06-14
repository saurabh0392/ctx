//! ctx library surface for integration tests and the `ctx` binary.

pub mod ab;
pub mod adaptive;
pub mod agent;
pub mod allowance;
pub mod analytics;
pub mod behavior_guard;
pub mod bench;
pub mod budget_guard;
pub mod claude_settings;
pub mod cli;
pub mod coach;
pub mod compress;
pub mod config;
pub mod context_ctl;
pub mod conversations;
pub mod daemon;
pub mod dashboard;
pub mod dashboard_push;
pub mod db;
pub mod embedder;
pub mod experiment_plan;
pub mod filter;
pub mod filter_control;
pub mod filter_hook;
pub mod hook;
pub mod host;
pub mod inject;
pub mod learn;
pub mod mcp;
pub mod modes;
pub mod outcome_signals;
pub mod profiles;
pub mod quality_guard;
pub mod rule_signals;
pub mod semantic_tools;
pub mod setup;
pub mod simulate;
pub mod socket;
pub mod stats;
pub mod surface;
pub mod test_lock;
pub mod tool_usage_analysis;
pub mod tuning;
pub mod user_profile;

use anyhow::Result;
use clap::Parser;
use cli::{
    BenchCommand, Cli, Commands, ContextCommand, ExperimentCommand, ExperimentPlanCommand,
    FilterCommand, HookCommand, InjectCommand, ModeCommand, ProfileCommand,
};

pub async fn run() -> Result<()> {
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
            ProfileCommand::Add {
                name,
                keep,
                keep_tool,
            } => profiles::add(
                &name,
                keep.unwrap_or_default(),
                keep_tool.unwrap_or_default(),
            )?,
            ProfileCommand::Remove { name } => profiles::remove(&name)?,
            ProfileCommand::Auto { refresh } => profiles::auto_generate(refresh)?,
            ProfileCommand::Generate => profiles::generate_from_config(true)?,
            ProfileCommand::MigrateTools { name, force } => {
                profiles::migrate_tools(name.as_deref(), force)?
            }
        },
        Commands::Inject { command } => match command {
            InjectCommand::Show => inject::show()?,
            InjectCommand::Edit => inject::edit()?,
            InjectCommand::Disable => inject::disable()?,
            InjectCommand::Enable => inject::enable()?,
        },
        Commands::Setup {
            uninstall,
            no_install,
            no_zshrc_prompt,
            dry_run,
            yes,
        } => {
            if uninstall {
                setup::uninstall()?;
            } else {
                setup::run(no_install, no_zshrc_prompt, dry_run, yes)?;
            }
        }
        Commands::Dashboard { port, no_open } => dashboard::serve(port, no_open).await?,
        Commands::Hook { command } => match command {
            HookCommand::UserPromptSubmit => hook::user_prompt_submit()?,
            HookCommand::PostToolUse => compress::post_tool_use()?,
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
                            simulate::format_all_profiles(
                                &results,
                                &effective_cwd,
                                &effective_prompt
                            )
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
            ExperimentCommand::Tick { dry_run } => experiment_plan::run_tick(dry_run)?,
            ExperimentCommand::Digest { json } => experiment_plan::run_digest(json)?,
            ExperimentCommand::InstallSchedule => experiment_plan::install_schedule()?,
            ExperimentCommand::Analyze { json } => {
                let conn = crate::db::open_db()?;
                crate::db::ensure_schema(&conn)?;
                let analysis = crate::tool_usage_analysis::run(&conn)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&analysis)?);
                } else {
                    crate::tool_usage_analysis::print_human(&analysis);
                }
            }
            ExperimentCommand::Plan { command } => match command {
                ExperimentPlanCommand::Init { corpus, template } => {
                    experiment_plan::plan_init(&corpus, &template)?
                }
                ExperimentPlanCommand::Status => experiment_plan::plan_status()?,
            },
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
        Commands::Context { command } => match command {
            ContextCommand::Status { json } => context_ctl::status(json)?,
            ContextCommand::Preset { value } => context_ctl::set_preset(&value)?,
            ContextCommand::On => context_ctl::set_preset("safe")?,
            ContextCommand::Off => context_ctl::set_preset("off")?,
            ContextCommand::Learn { json } => learn::run(json)?,
            ContextCommand::Labels { tool, limit, json } => {
                context_ctl::labels(tool.as_deref(), limit, json)?
            }
            ContextCommand::Proof { tool, json } => context_ctl::proof(tool.as_deref(), json)?,
            ContextCommand::Trial { tool, on, off } => {
                context_ctl::trial(tool.as_deref(), on, off)?
            }
            ContextCommand::CacheAudit { days, json } => context_ctl::cache_audit(days, json)?,
        },
        Commands::Bench { command } => match command {
            BenchCommand::Run { json } => bench::run(json)?,
        },
    }

    Ok(())
}
