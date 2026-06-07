//! Calendar-driven experiment plan: phased config patches, daily digest, journal.

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::{ctx_dir, ensure_dir, AbTestConfig, Config};
use crate::tuning::{self, FeatureVerdict, MIN_SAMPLES};

pub const PLAN_FILENAME: &str = "experiment-plan.toml";
pub const JOURNAL_FILENAME: &str = "experiment-journal.jsonl";
pub const STATE_FILENAME: &str = "experiment-plan-state.json";
pub const BACKUP_FILENAME: &str = "experiment-plan-backup.toml";

const MILESTONE_DAYS: [u32; 3] = [7, 10, 16];

#[derive(Debug, Clone, Serialize)]
pub struct BaselineComparison {
    pub pre_ctx_turns: u64,
    pub pre_ctx_avg_cost_usd: Option<f64>,
    pub ctx_on_turns: u64,
    pub ctx_on_avg_cost_usd: Option<f64>,
    pub delta_cost_pct: Option<f64>,
    /// True when both arms have at least one indexed turn.
    pub ready: bool,
}

fn phase_day_span(plan: &ExperimentPlan, phase_name: &str) -> Option<(u32, u32)> {
    let mut prev_until = 0u32;
    for p in &plan.phases {
        if p.name == phase_name {
            return Some((prev_until + 1, p.until_day));
        }
        prev_until = p.until_day;
    }
    None
}

fn day_to_date(plan: &ExperimentPlan, day: u32) -> NaiveDate {
    plan.started_at + chrono::Duration::days(day.saturating_sub(1) as i64)
}

fn sessions_cost_in_day_span(
    plan: &ExperimentPlan,
    corpus_path: &str,
    start_day: u32,
    end_day: u32,
) -> Result<(u64, f64)> {
    let conn = crate::db::open_db()?;
    let likes = corpus_like_sql(corpus_path);
    let start_ts = day_to_date(plan, start_day)
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let end_exclusive = day_to_date(plan, end_day + 1)
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let wd = corpus_session_workdir_sql();
    let (turns, spend): (i64, f64) = conn.query_row(
        &format!(
            "SELECT CAST(COALESCE(SUM(turn_count), 0) AS INTEGER),
                    COALESCE(SUM(total_usd), 0.0)
             FROM sessions
             WHERE started_at >= ?1 AND started_at < ?2 AND {wd}"
        ),
        rusqlite::params![
            start_ts,
            end_exclusive,
            likes.full,
            likes.basename,
            likes.basename_spaced
        ],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((turns.max(0) as u64, spend.max(0.0)))
}

pub fn baseline_comparison(plan: &ExperimentPlan) -> Option<BaselineComparison> {
    let (pre_start, pre_end) = phase_day_span(plan, "pre_ctx")?;
    let (on_start, on_end) = phase_day_span(plan, "ctx_warmup")?;
    let (pre_turns, pre_spend) = sessions_cost_in_day_span(plan, &plan.corpus_path, pre_start, pre_end).ok()?;
    let (on_turns, on_spend) = sessions_cost_in_day_span(plan, &plan.corpus_path, on_start, on_end).ok()?;
    let pre_avg = if pre_turns > 0 {
        Some(pre_spend / pre_turns as f64)
    } else {
        None
    };
    let on_avg = if on_turns > 0 {
        Some(on_spend / on_turns as f64)
    } else {
        None
    };
    let delta = match (pre_avg, on_avg) {
        (Some(p), Some(c)) if p > 0.0 => Some(((c - p) / p) * 100.0),
        _ => None,
    };
    Some(BaselineComparison {
        pre_ctx_turns: pre_turns,
        pre_ctx_avg_cost_usd: pre_avg,
        ctx_on_turns: on_turns,
        ctx_on_avg_cost_usd: on_avg,
        delta_cost_pct: delta,
        ready: pre_turns > 0 && on_turns > 0,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentPlan {
    pub started_at: NaiveDate,
    pub corpus_path: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub apply_recommendations_on_final_day: bool,
    pub phases: Vec<ExperimentPhase>,
}

fn default_mode() -> String {
    "stress_test".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentPhase {
    pub name: String,
    pub until_day: u32,
    #[serde(default)]
    pub config: PhaseConfigPatch,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhaseConfigPatch {
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub auto_profile_enabled: Option<bool>,
    /// When false, strip ctx hooks and filters (pre-ctx baseline). When true, reinstall hooks.
    #[serde(default)]
    pub hooks_enabled: Option<bool>,
    #[serde(default)]
    pub ab_test: Option<AbTestConfig>,
    #[serde(default)]
    pub semantic_tool_mix_enabled: Option<bool>,
    #[serde(default)]
    pub compress_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanState {
    pub last_applied_phase: Option<String>,
    pub last_tick_date: Option<NaiveDate>,
    #[serde(default)]
    pub milestones_notified: Vec<u32>,
    #[serde(default)]
    pub decisive_notified: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentDigest {
    pub day: u32,
    pub total_days: u32,
    pub phase: String,
    pub phase_days_remaining: u32,
    pub corpus_path: String,
    pub one_liner: String,
    pub sample_gates: Vec<SampleGate>,
    pub stress: Option<StressMetrics>,
    pub auto_profile: Option<AutoProfileKpi>,
    pub ab_verdicts: Vec<FeatureVerdict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleGate {
    pub feature: String,
    pub treatment_count: u64,
    pub control_count: u64,
    pub min_per_arm: i64,
    pub decisive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressMetrics {
    pub prompts_today: u64,
    pub prompts_7d: u64,
    pub mcp_turn_ratio_pct: f64,
    pub enriched_ratio_pct: f64,
    pub peak_day_prompts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoProfileKpi {
    pub total_prompts: u64,
    pub auto_selected_pct: f64,
    pub similarity_match_pct: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalEntry {
    ts: String,
    day: u32,
    phase: String,
    event: String,
    patch: Option<PhaseConfigPatch>,
    sample_warnings: Vec<String>,
    digest_line: String,
}

pub fn plan_path() -> PathBuf {
    ctx_dir().join(PLAN_FILENAME)
}

pub fn journal_path() -> PathBuf {
    ctx_dir().join(JOURNAL_FILENAME)
}

pub fn state_path() -> PathBuf {
    ctx_dir().join(STATE_FILENAME)
}

pub fn backup_config_path() -> PathBuf {
    ctx_dir().join(BACKUP_FILENAME)
}

const PERSISTED_EXPERIMENT_FILES: [&str; 4] = [
    PLAN_FILENAME,
    JOURNAL_FILENAME,
    STATE_FILENAME,
    BACKUP_FILENAME,
];

/// Survives `rm -rf ~/.ctx` (lives outside the ctx home dir).
pub fn persistent_experiment_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CTX_EXPERIMENT_BACKUP_DIR") {
        return PathBuf::from(p);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/ctx/experiment")
    } else {
        home.join(".ctx-experiment-backup")
    }
}

/// Mirror plan, journal, state, and config snapshot to persistent storage.
pub fn backup_experiment_state() -> Result<()> {
    if !plan_path().is_file() {
        return Ok(());
    }
    let dest_dir = persistent_experiment_dir();
    std::fs::create_dir_all(&dest_dir)?;
    for name in PERSISTED_EXPERIMENT_FILES {
        let src = ctx_dir().join(name);
        if src.is_file() {
            std::fs::copy(&src, dest_dir.join(name))?;
        }
    }
    Ok(())
}

/// Restore experiment files when ~/.ctx was wiped but a persistent backup exists.
pub fn restore_experiment_state_if_missing() -> Result<bool> {
    if plan_path().is_file() {
        return Ok(false);
    }
    let src_dir = persistent_experiment_dir();
    let src_plan = src_dir.join(PLAN_FILENAME);
    if !src_plan.is_file() {
        return Ok(false);
    }
    ensure_dir()?;
    for name in PERSISTED_EXPERIMENT_FILES {
        let src = src_dir.join(name);
        if src.is_file() {
            std::fs::copy(&src, ctx_dir().join(name))?;
        }
    }
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::maybe_reset_stale_install_watermark(&conn);
    }
    Ok(true)
}

pub fn load_plan() -> Result<ExperimentPlan> {
    let path = plan_path();
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("read {} (run `ctx experiment plan init` first)", path.display()))?;
    let mut plan: ExperimentPlan = toml::from_str(&content).context("parse experiment-plan.toml")?;
    if migrate_legacy_plan(&mut plan) {
        save_plan(&plan)?;
    }
    Ok(plan)
}

/// Upgrade old `baseline` phases to pre-ctx + ctx_warmup calendar.
fn migrate_legacy_plan(plan: &mut ExperimentPlan) -> bool {
    let mut changed = false;
    for phase in &mut plan.phases {
        if phase.name == "baseline" {
            phase.name = "pre_ctx".to_string();
            phase.config.hooks_enabled = Some(false);
            phase.config.active_profile = Some("all".into());
            phase.config.auto_profile_enabled = Some(false);
            changed = true;
        }
    }
    if !plan.phases.iter().any(|p| p.name == "ctx_warmup") {
        if let Some(idx) = plan.phases.iter().position(|p| p.name == "pre_ctx") {
            let until = plan.phases[idx].until_day.saturating_add(1);
            plan.phases.insert(
                idx + 1,
                ExperimentPhase {
                    name: "ctx_warmup".to_string(),
                    until_day: until,
                    config: PhaseConfigPatch {
                        hooks_enabled: Some(true),
                        active_profile: Some("all".into()),
                        auto_profile_enabled: Some(false),
                        ab_test: Some(AbTestConfig {
                            profile_pct: 100,
                            inject_pct: 100,
                            adaptive_pct: 100,
                            coaching_pct: 100,
                            compress_pct: 100,
                            compress_sgr_pct: 100,
                            tool_mix_pct: 100,
                        }),
                        ..Default::default()
                    },
                },
            );
            changed = true;
        }
    }
    if !plan.phases.iter().any(|p| p.name == "compress_sgr_ab") {
        let tool_mix_calendar = plan
            .phases
            .iter()
            .any(|p| p.name == "tool_mix_ab" || p.name == "baseline_static");
        if !tool_mix_calendar {
            if let Some(lock_idx) = plan.phases.iter().position(|p| p.name == "lock_in") {
                let lock_until = plan.phases[lock_idx].until_day;
                plan.phases[lock_idx].until_day = lock_until.saturating_add(1);
                plan.phases.insert(
                    lock_idx,
                    ExperimentPhase {
                        name: "compress_sgr_ab".to_string(),
                        until_day: lock_until,
                        config: PhaseConfigPatch {
                            hooks_enabled: Some(true),
                            ab_test: Some(AbTestConfig {
                                profile_pct: 100,
                                inject_pct: 100,
                                adaptive_pct: 100,
                                coaching_pct: 100,
                                compress_pct: 100,
                                compress_sgr_pct: 50,
                                tool_mix_pct: 100,
                            }),
                            ..Default::default()
                        },
                    },
                );
                changed = true;
            }
        }
    }
    changed
}

pub fn save_plan(plan: &ExperimentPlan) -> Result<()> {
    ensure_dir()?;
    let content = toml::to_string_pretty(plan)?;
    std::fs::write(plan_path(), content)?;
    let _ = backup_experiment_state();
    Ok(())
}

pub fn load_state() -> PlanState {
    let path = state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_state(state: &PlanState) -> Result<()> {
    ensure_dir()?;
    std::fs::write(state_path(), serde_json::to_string_pretty(state)?)?;
    let _ = backup_experiment_state();
    Ok(())
}

pub fn total_plan_days(plan: &ExperimentPlan) -> u32 {
    plan.phases.iter().map(|p| p.until_day).max().unwrap_or(15)
}

pub fn current_day(plan: &ExperimentPlan, today: NaiveDate) -> u32 {
    let delta = (today - plan.started_at).num_days();
    (delta.max(0) as u32) + 1
}

pub fn resolve_phase<'a>(plan: &'a ExperimentPlan, day: u32) -> &'a ExperimentPhase {
    plan.phases
        .iter()
        .find(|p| day <= p.until_day)
        .unwrap_or_else(|| plan.phases.last().expect("plan has at least one phase"))
}

pub fn phase_days_remaining(phase: &ExperimentPhase, day: u32) -> u32 {
    phase.until_day.saturating_sub(day)
}

pub fn plan_init(corpus: &str, template: &str) -> Result<()> {
    ensure_dir()?;
    let today = Local::now().date_naive();
    let raw = match template {
        "ctx" => include_str!("../docs/experiment-plan.ctx.toml"),
        "tool-mix" => include_str!("../docs/experiment-plan.tool-mix.toml"),
        _ => include_str!("../docs/experiment-plan.gaffer.toml"),
    };
    let content = raw.replace("__CORPUS__", corpus).replace(
        "__STARTED_AT__",
        &today.format("%Y-%m-%d").to_string(),
    );
    std::fs::write(plan_path(), &content)?;
    let plan: ExperimentPlan = toml::from_str(&content)?;
    save_plan(&plan)?;
    let mut state = PlanState::default();
    state.last_tick_date = None;
    save_state(&state)?;
    println!("Created {}", plan_path().display());
    println!("  started_at: {}", plan.started_at);
    println!("  corpus:     {}", plan.corpus_path);
    println!("  phases:     {}", plan.phases.len());
    println!();
    println!("Next:");
    println!("  ctx experiment install-schedule   # daily tick at 09:00");
    println!("  ctx experiment tick               # run now (phase patch + digest)");
    Ok(())
}

pub fn plan_status() -> Result<()> {
    let plan = load_plan()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    let total = total_plan_days(&plan);
    let state = load_state();
    println!(
        "Experiment plan: day {day}/{total} · phase {} ({} day(s) left in phase)",
        phase.name,
        phase_days_remaining(phase, day)
    );
    println!("  started_at:  {}", plan.started_at);
    println!("  corpus:      {}", plan.corpus_path);
    println!("  last phase:  {}", state.last_applied_phase.as_deref().unwrap_or("(none)"));
    if state.last_applied_phase.as_deref() != Some(phase.name.as_str()) {
        println!(
            "  ⚠ pending:   calendar is {} but config still on {} — run `ctx experiment tick`",
            phase.name,
            state.last_applied_phase.as_deref().unwrap_or("(none)")
        );
    }
    if let Some(last) = state.last_tick_date {
        println!("  last tick:   {last}");
    }
    if let Ok(entry) = last_journal_line() {
        println!("  last journal: {entry}");
    }
    Ok(())
}

fn last_journal_line() -> Result<String> {
    let content = std::fs::read_to_string(journal_path())?;
    content
        .lines()
        .last()
        .map(|l| {
            serde_json::from_str::<JournalEntry>(l)
                .map(|e| format!("{} · {}", e.event, e.digest_line))
                .unwrap_or_else(|_| l.chars().take(80).collect())
        })
        .context("empty journal")
}

pub fn run_digest(json: bool) -> Result<()> {
    let digest = build_digest()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&digest)?);
    } else {
        print_digest(&digest);
    }
    Ok(())
}

/// Apply the calendar phase patch when the plan day advanced but the daily tick has not run yet.
/// Returns true when a new phase was applied.
pub fn ensure_pending_phase_applied() -> Result<bool> {
    let plan = load_plan()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    let mut state = load_state();
    if state.last_applied_phase.as_deref() == Some(phase.name.as_str()) {
        return Ok(false);
    }
    backup_config_if_needed()?;
    apply_phase_patch(&phase.config)?;
    state.last_applied_phase = Some(phase.name.clone());
    save_state(&state)?;
    append_journal(JournalEntry {
        ts: chrono::Utc::now().to_rfc3339(),
        day,
        phase: phase.name.clone(),
        event: "phase_change".to_string(),
        patch: Some(phase.config.clone()),
        sample_warnings: Vec::new(),
        digest_line: format!("Day {day} · {} (auto-applied)", phase.name),
    })?;
    Ok(true)
}

/// Active profile pinned by the current experiment phase, if any.
pub fn experiment_active_profile_pin() -> Option<String> {
    let plan = load_plan().ok()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    phase.config.active_profile.clone()
}

/// Re-apply the calendar phase patch (ingest and profile bootstrap can override config).
pub fn reapply_current_phase_config() -> Result<()> {
    let plan = load_plan()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    apply_phase_patch(&phase.config)?;
    crate::claude_settings::sync_experiment_hooks_from_config()?;
    Ok(())
}

pub fn run_tick(dry_run: bool) -> Result<()> {
    let plan = load_plan()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    let mut state = load_state();

    let phase_changed = state.last_applied_phase.as_deref() != Some(phase.name.as_str());
    let mut notifications: Vec<(String, String)> = Vec::new();

    if phase_changed {
        if dry_run {
            println!(
                "[dry-run] Would apply phase {} patch: {:?}",
                phase.name, phase.config
            );
        } else {
            let _ = ensure_pending_phase_applied()?;
            state = load_state();
            let reload = phase.config.hooks_enabled.is_some();
            let body = if phase.name == "pre_ctx" {
                "Hooks off. Work normally — ingest records your without-ctx baseline. Reload your IDE.".to_string()
            } else if phase.name == "ctx_warmup" {
                "Hooks back on. ctx is fully active before feature A/B tests. Reload your IDE.".to_string()
            } else if reload {
                format!("Day {day}: entered phase {}. Reload your IDE if hooks changed.", phase.name)
            } else {
                format!("Day {day}: entered phase {}", phase.name)
            };
            notifications.push(("ctx experiment".to_string(), body));
        }
    }

    if !dry_run {
        let _ = crate::conversations::ingest_claude_jsonl();
        if let Ok(conn) = crate::db::open_db() {
            let _ = tuning::run_tuning_after_ingest(&conn);
        }
        // Ingest may touch settings (e.g. personal profile upsert); restore phase + hooks.
        let _ = reapply_current_phase_config();
    }

    let digest = build_digest_from(&plan, day, phase)?;
    let warnings: Vec<String> = digest
        .sample_gates
        .iter()
        .filter(|g| !g.decisive)
        .map(|g| {
            format!(
                "{}: {}T/{}C (need {}/arm)",
                g.feature, g.treatment_count, g.control_count, g.min_per_arm
            )
        })
        .collect();

    if !dry_run {
        let event = if phase_changed {
            "phase_change"
        } else {
            "daily_tick"
        };
        append_journal(JournalEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            day,
            phase: phase.name.clone(),
            event: event.to_string(),
            patch: if phase_changed {
                Some(phase.config.clone())
            } else {
                None
            },
            sample_warnings: warnings.clone(),
            digest_line: digest.one_liner.clone(),
        })?;

        for &m in &MILESTONE_DAYS {
            if day == m && !state.milestones_notified.contains(&m) {
                notifications.push((
                    "ctx experiment".to_string(),
                    format!("Day {m} milestone — {}", digest.one_liner),
                ));
                state.milestones_notified.push(m);
            }
        }

        for gate in &digest.sample_gates {
            if gate.decisive && !state.decisive_notified.contains(&gate.feature) {
                notifications.push((
                    "ctx experiment".to_string(),
                    format!(
                        "{} decisive ({}T/{}C)",
                        gate.feature, gate.treatment_count, gate.control_count
                    ),
                ));
                state.decisive_notified.push(gate.feature.clone());
            }
        }

        state.last_tick_date = Some(today);
        save_state(&state)?;

        for (title, body) in notifications {
            notify_macos(&title, &body);
        }

        if day == total_plan_days(&plan)
            && plan.apply_recommendations_on_final_day
            && phase.name == "lock_in"
        {
            let _ = tuning::apply_recommendations();
        }
    }

    if dry_run {
        println!("[dry-run] {}", digest.one_liner);
    } else {
        print_digest(&digest);
    }
    Ok(())
}

pub fn install_schedule() -> Result<()> {
    crate::daemon::install_experiment_tick()
}

/// Dashboard summary for the 15-day calendar plan (empty when no plan file).
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentPlanDashboard {
    pub configured: bool,
    pub started_at: String,
    pub corpus_path: String,
    pub day: u32,
    pub total_days: u32,
    pub phase: String,
    pub phase_days_remaining: u32,
    /// Feature under A/B this phase, if any (`profile`, `inject`, …).
    pub phase_ab_feature: Option<String>,
    pub next_phase: Option<String>,
    pub next_phase_starts_day: Option<u32>,
    pub one_liner: String,
    pub sample_gates: Vec<SampleGate>,
    /// False during pre-ctx (hooks stripped).
    #[serde(default = "experiment_hooks_enabled_default")]
    pub hooks_enabled: bool,
    /// False when calendar phase advanced but patch not applied yet (waiting for tick).
    #[serde(default = "experiment_hooks_enabled_default")]
    pub phase_applied: bool,
    pub baseline_comparison: Option<BaselineComparison>,
}

fn experiment_hooks_enabled_default() -> bool {
    true
}

pub fn plan_for_dashboard() -> ExperimentPlanDashboard {
    let _ = ensure_pending_phase_applied();
    let Ok(plan) = load_plan() else {
        return ExperimentPlanDashboard {
            configured: false,
            started_at: String::new(),
            corpus_path: String::new(),
            day: 0,
            total_days: 0,
            phase: String::new(),
            phase_days_remaining: 0,
            phase_ab_feature: None,
            next_phase: None,
            next_phase_starts_day: None,
            one_liner: String::new(),
            sample_gates: Vec::new(),
            hooks_enabled: true,
            phase_applied: true,
            baseline_comparison: None,
        };
    };
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    let state = load_state();
    let cfg = crate::config::Config::load();
    let hooks_enabled = cfg.experiment_hooks_enabled;
    let phase_applied = state.last_applied_phase.as_deref() == Some(phase.name.as_str());
    let baseline_comparison = if day > phase_day_span(&plan, "pre_ctx").map(|(_, e)| e).unwrap_or(0) {
        baseline_comparison(&plan)
    } else {
        None
    };
    let ab_verdicts = tuning::load_ab_results()
        .map(|r| r.features)
        .unwrap_or_default();
    let sample_gates = sample_gates_for_phase(phase, &ab_verdicts);
    let (next_phase, next_phase_starts_day) = next_phase_after(&plan, day);
    let one_liner = build_digest_from(&plan, day, phase)
        .map(|d| d.one_liner)
        .unwrap_or_else(|_| format!("Day {day} · {}", phase.name));
    ExperimentPlanDashboard {
        configured: true,
        started_at: plan.started_at.format("%Y-%m-%d").to_string(),
        corpus_path: plan.corpus_path.clone(),
        day,
        total_days: total_plan_days(&plan),
        phase: phase.name.clone(),
        phase_days_remaining: phase_days_remaining(phase, day),
        phase_ab_feature: active_ab_feature(phase).map(str::to_string),
        next_phase,
        next_phase_starts_day,
        one_liner,
        sample_gates,
        hooks_enabled,
        phase_applied,
        baseline_comparison,
    }
}

fn next_phase_after(plan: &ExperimentPlan, day: u32) -> (Option<String>, Option<u32>) {
    for (i, p) in plan.phases.iter().enumerate() {
        if day <= p.until_day {
            if i + 1 < plan.phases.len() {
                let next = &plan.phases[i + 1];
                return (Some(next.name.clone()), Some(p.until_day.saturating_add(1)));
            }
            return (None, None);
        }
    }
    (None, None)
}

fn build_digest() -> Result<ExperimentDigest> {
    let plan = load_plan()?;
    let today = Local::now().date_naive();
    let day = current_day(&plan, today);
    let phase = resolve_phase(&plan, day);
    build_digest_from(&plan, day, phase)
}

fn build_digest_from(
    plan: &ExperimentPlan,
    day: u32,
    phase: &ExperimentPhase,
) -> Result<ExperimentDigest> {
    let total = total_plan_days(plan);
    let ab_verdicts = tuning::load_ab_results()
        .map(|r| r.features)
        .unwrap_or_default();
    let sample_gates = sample_gates_for_phase(phase, &ab_verdicts);
    let stress = if plan.mode == "stress_test" {
        stress_metrics(&plan.corpus_path).ok()
    } else {
        None
    };
    let auto_profile = if phase.name.contains("auto") {
        auto_profile_kpi(&plan.corpus_path).ok()
    } else {
        None
    };
    let one_liner = format_one_liner(day, phase, &sample_gates, stress.as_ref(), auto_profile.as_ref());
    Ok(ExperimentDigest {
        day,
        total_days: total,
        phase: phase.name.clone(),
        phase_days_remaining: phase_days_remaining(phase, day),
        corpus_path: plan.corpus_path.clone(),
        one_liner,
        sample_gates,
        stress,
        auto_profile,
        ab_verdicts,
    })
}

fn format_one_liner(
    day: u32,
    phase: &ExperimentPhase,
    gates: &[SampleGate],
    stress: Option<&StressMetrics>,
    auto: Option<&AutoProfileKpi>,
) -> String {
    let mut parts = vec![format!("Day {} · {}", day, phase.name)];
    if let Some(g) = gates.first() {
        let tag = if g.decisive { "decisive" } else { "sampling" };
        parts.push(format!(
            "{}: {}T/{}C ({})",
            g.feature, g.treatment_count, g.control_count, tag
        ));
    }
    if let Some(s) = stress {
        parts.push(format!("{} prompts today", s.prompts_today));
        if s.mcp_turn_ratio_pct > 0.0 {
            parts.push(format!("{:.0}% MCP turns", s.mcp_turn_ratio_pct));
        }
    }
    if let Some(a) = auto {
        parts.push(format!("{:.0}% auto-selected", a.auto_selected_pct));
    }
    let rem = phase_days_remaining(phase, day);
    if rem > 0 {
        parts.push(format!("next phase in {rem} day(s)"));
    }
    parts.join(" · ")
}

fn sample_gates_for_phase(phase: &ExperimentPhase, verdicts: &[FeatureVerdict]) -> Vec<SampleGate> {
    let feature = active_ab_feature(phase);
    let Some(name) = feature else {
        return Vec::new();
    };
    let v = verdicts.iter().find(|f| f.feature == name);
    match v {
        Some(f) => vec![SampleGate {
            feature: name.to_string(),
            treatment_count: f.treatment_count,
            control_count: f.control_count,
            min_per_arm: MIN_SAMPLES,
            decisive: f.treatment_count >= MIN_SAMPLES as u64 && f.control_count >= MIN_SAMPLES as u64,
        }],
        None => vec![SampleGate {
            feature: name.to_string(),
            treatment_count: 0,
            control_count: 0,
            min_per_arm: MIN_SAMPLES,
            decisive: false,
        }],
    }
}

fn active_ab_feature(phase: &ExperimentPhase) -> Option<&'static str> {
    match phase.name.as_str() {
        "profile_ab" => Some("profile"),
        "inject_ab" => Some("inject"),
        "adaptive_ab" => Some("adaptive"),
        "compress_ab" => Some("compress"),
        "compress_sgr_ab" => Some("compress_sgr"),
        "tool_mix_ab" => Some("tool_mix"),
        _ => None,
    }
}

struct CorpusLikePatterns {
    full: String,
    basename: String,
    /// Basename with hyphens as spaces (matches JSONL project labels like "the gaffer").
    basename_spaced: String,
}

fn corpus_like_sql(path: &str) -> CorpusLikePatterns {
    let pattern = if path.ends_with('/') {
        format!("%{}%", path.trim_end_matches('/'))
    } else {
        format!("%{}%", path)
    };
    let basename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    CorpusLikePatterns {
        full: pattern,
        basename: format!("%{basename}%"),
        basename_spaced: format!("%{}%", basename.replace('-', " ")),
    }
}

fn corpus_hook_workdir_sql() -> &'static str {
    "(working_directory LIKE ?1 OR working_directory LIKE ?2 OR working_directory LIKE ?3)"
}

fn corpus_session_workdir_sql() -> &'static str {
    "(working_directory LIKE ?3 OR working_directory LIKE ?4 OR working_directory LIKE ?5 \
      OR project LIKE ?3 OR project LIKE ?4 OR project LIKE ?5 \
      OR working_directory = '' OR working_directory IS NULL)"
}

fn stress_metrics(corpus_path: &str) -> Result<StressMetrics> {
    let conn = crate::db::open_db()?;
    let likes = corpus_like_sql(corpus_path);
    let wd = corpus_hook_workdir_sql();
    let hook_params = rusqlite::params![likes.full, likes.basename, likes.basename_spaced];

    let prompts_today: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM hook_traces WHERE date(ts) = date('now') AND {wd}"),
        hook_params,
        |r| r.get(0),
    )?;
    let prompts_7d: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM hook_traces WHERE ts >= datetime('now', '-7 days') AND {wd}"
        ),
        hook_params,
        |r| r.get(0),
    )?;
    let mcp_turns: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM hook_traces WHERE date(ts) = date('now') AND {wd} AND COALESCE(tools_removed, 0) > 0"
        ),
        hook_params,
        |r| r.get(0),
    )?;
    let enriched: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM hook_traces WHERE date(ts) = date('now') AND {wd} AND enriched = 1"
        ),
        hook_params,
        |r| r.get(0),
    )?;
    let peak_day: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(MAX(c), 0) FROM (
                SELECT COUNT(*) AS c FROM hook_traces WHERE {wd} GROUP BY date(ts)
             )"
        ),
        hook_params,
        |r| r.get(0),
    )?;

    let today = prompts_today.max(0) as u64;
    let mcp_ratio = if today > 0 {
        (mcp_turns.max(0) as f64 / today as f64) * 100.0
    } else {
        0.0
    };
    let enriched_ratio = if today > 0 {
        (enriched.max(0) as f64 / today as f64) * 100.0
    } else {
        0.0
    };

    Ok(StressMetrics {
        prompts_today: today,
        prompts_7d: prompts_7d.max(0) as u64,
        mcp_turn_ratio_pct: mcp_ratio,
        enriched_ratio_pct: enriched_ratio,
        peak_day_prompts: peak_day.max(0) as u64,
    })
}

fn auto_profile_kpi(corpus_path: &str) -> Result<AutoProfileKpi> {
    let conn = crate::db::open_db()?;
    let likes = corpus_like_sql(corpus_path);
    let wd = corpus_hook_workdir_sql();
    let hook_params = rusqlite::params![likes.full, likes.basename, likes.basename_spaced];

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM hook_traces WHERE ts >= datetime('now', '-3 days') AND {wd}"),
        hook_params,
        |r| r.get(0),
    )?;
    let auto_n: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM hook_traces WHERE ts >= datetime('now', '-3 days') AND {wd} AND auto_selected = 1"
        ),
        hook_params,
        |r| r.get(0),
    )?;
    let sim_n: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM hook_traces WHERE ts >= datetime('now', '-3 days') AND {wd} AND auto_trigger LIKE 'similarity:%'"
        ),
        hook_params,
        |r| r.get(0),
    )?;

    let total_u = total.max(0) as u64;
    let pct = |n: i64| {
        if total_u == 0 {
            0.0
        } else {
            (n.max(0) as f64 / total_u as f64) * 100.0
        }
    };

    Ok(AutoProfileKpi {
        total_prompts: total_u,
        auto_selected_pct: pct(auto_n),
        similarity_match_pct: pct(sim_n),
    })
}

fn backup_config_if_needed() -> Result<()> {
    let src = ctx_dir().join("config.toml");
    if src.exists() && !backup_config_path().exists() {
        std::fs::copy(&src, backup_config_path())?;
    }
    Ok(())
}

pub fn apply_phase_patch(patch: &PhaseConfigPatch) -> Result<()> {
    let mut cfg = Config::load();
    if let Some(ref p) = patch.active_profile {
        cfg.active_profile = Some(p.clone());
    }
    if let Some(v) = patch.auto_profile_enabled {
        cfg.auto_profile_enabled = v;
    }
    if let Some(v) = patch.hooks_enabled {
        cfg.experiment_hooks_enabled = v;
    }
    if let Some(ref ab) = patch.ab_test {
        cfg.ab_test = Some(ab.clone());
    }
    if let Some(v) = patch.semantic_tool_mix_enabled {
        cfg.semantic_tool_mix_enabled = v;
        if v
            && cfg.semantic_tool_mix_min_similarity <= 0.0
            && cfg.semantic_tool_mix_min_neighbor_fraction <= 0.0
            && cfg.semantic_tool_mix_top_k == 0
        {
            cfg.semantic_tool_mix_min_similarity = 0.75;
            cfg.semantic_tool_mix_min_neighbor_fraction = 0.3;
            cfg.semantic_tool_mix_top_k = 7;
        }
    }
    if let Some(v) = patch.compress_enabled {
        cfg.compress_enabled = v;
    }
    cfg.save()?;
    crate::claude_settings::sync_experiment_hooks_from_config()?;
    Ok(())
}

fn append_journal(entry: JournalEntry) -> Result<()> {
    ensure_dir()?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path())?;
    writeln!(f, "{}", serde_json::to_string(&entry)?)?;
    let _ = backup_experiment_state();
    Ok(())
}

fn print_digest(d: &ExperimentDigest) {
    println!("{}", d.one_liner);
    if let Some(s) = &d.stress {
        println!(
            "  stress: {} prompts (7d {}), {:.0}% MCP, {:.0}% enriched, peak day {}",
            s.prompts_today,
            s.prompts_7d,
            s.mcp_turn_ratio_pct,
            s.enriched_ratio_pct,
            s.peak_day_prompts
        );
    }
    if let Some(a) = &d.auto_profile {
        println!(
            "  auto-profile (3d): {:.0}% selected, {:.0}% similarity ({} prompts)",
            a.auto_selected_pct, a.similarity_match_pct, a.total_prompts
        );
    }
    for g in &d.sample_gates {
        let status = if g.decisive { "decisive" } else { "need more samples" };
        println!(
            "  {} A/B: {}T / {}C — {}",
            g.feature, g.treatment_count, g.control_count, status
        );
    }
    for v in &d.ab_verdicts {
        if active_ab_feature(&ExperimentPhase {
            name: d.phase.clone(),
            until_day: 0,
            config: PhaseConfigPatch::default(),
        })
        .is_some_and(|f| f == v.feature)
        {
            println!("  verdict [{}]: {}", v.verdict, v.message);
        }
    }
}

fn notify_macos(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {} with title {}",
            serde_json::to_string(body).unwrap_or_else(|_| "\"\"".to_string()),
            serde_json::to_string(title).unwrap_or_else(|_| "\"ctx\"".to_string()),
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .status();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> ExperimentPlan {
        ExperimentPlan {
            started_at: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            corpus_path: "/tmp/gaffer".to_string(),
            mode: "stress_test".to_string(),
            apply_recommendations_on_final_day: false,
            phases: vec![
                ExperimentPhase {
                    name: "pre_ctx".to_string(),
                    until_day: 2,
                    config: PhaseConfigPatch {
                        hooks_enabled: Some(false),
                        ..Default::default()
                    },
                },
                ExperimentPhase {
                    name: "ctx_warmup".to_string(),
                    until_day: 3,
                    config: PhaseConfigPatch {
                        hooks_enabled: Some(true),
                        ..Default::default()
                    },
                },
                ExperimentPhase {
                    name: "profile_ab".to_string(),
                    until_day: 7,
                    config: PhaseConfigPatch {
                        ab_test: Some(AbTestConfig {
                            profile_pct: 50,
                            inject_pct: 100,
                            adaptive_pct: 100,
                            coaching_pct: 100,
                            compress_pct: 100,
                            compress_sgr_pct: 100,
                            tool_mix_pct: 100,
                        }),
                        ..Default::default()
                    },
                },
                ExperimentPhase {
                    name: "lock_in".to_string(),
                    until_day: 15,
                    config: PhaseConfigPatch::default(),
                },
            ],
        }
    }

    #[test]
    fn current_day_and_phase_resolution() {
        let plan = sample_plan();
        assert_eq!(current_day(&plan, NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()), 1);
        assert_eq!(resolve_phase(&plan, 1).name, "pre_ctx");
        assert_eq!(resolve_phase(&plan, 3).name, "ctx_warmup");
        assert_eq!(resolve_phase(&plan, 5).name, "profile_ab");
        assert_eq!(resolve_phase(&plan, 15).name, "lock_in");
    }

    #[test]
    fn phase_days_remaining_counts() {
        let plan = sample_plan();
        let phase = resolve_phase(&plan, 5);
        assert_eq!(phase_days_remaining(phase, 5), 2);
    }

    #[test]
    fn apply_phase_patch_sets_ab_test() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var("CTX_HOME").ok();
        std::env::set_var("CTX_HOME", tmp.path());
        let _ = ensure_dir();
        apply_phase_patch(&PhaseConfigPatch {
            active_profile: Some("design".to_string()),
            auto_profile_enabled: Some(true),
            ab_test: Some(AbTestConfig {
                profile_pct: 50,
                inject_pct: 100,
                adaptive_pct: 100,
                coaching_pct: 100,
                compress_pct: 100,
                compress_sgr_pct: 100,
                tool_mix_pct: 100,
            }),
            ..Default::default()
        })
        .unwrap();
        let cfg = Config::load();
        assert_eq!(cfg.active_profile.as_deref(), Some("design"));
        assert_eq!(cfg.ab_test.as_ref().unwrap().profile_pct, 50);
        match prev {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
    }

    #[test]
    fn sample_gate_decisive_at_100() {
        let phase = ExperimentPhase {
            name: "profile_ab".to_string(),
            until_day: 7,
            config: PhaseConfigPatch::default(),
        };
        let verdicts = vec![FeatureVerdict {
            feature: "profile".to_string(),
            verdict: "insufficient_data".to_string(),
            treatment_count: 120,
            control_count: 105,
            treatment_avg_cost: 0.03,
            control_avg_cost: 0.04,
            treatment_correction_pct: 0.0,
            control_correction_pct: 0.0,
            delta_cost_pct: Some(-25.0),
            message: String::new(),
        }];
        let gates = sample_gates_for_phase(&phase, &verdicts);
        assert!(gates[0].decisive);
    }

    #[test]
    fn migrate_legacy_baseline_to_pre_ctx() {
        let mut plan = ExperimentPlan {
            started_at: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            corpus_path: "/tmp".to_string(),
            mode: "stress_test".to_string(),
            apply_recommendations_on_final_day: false,
            phases: vec![ExperimentPhase {
                name: "baseline".to_string(),
                until_day: 2,
                config: PhaseConfigPatch::default(),
            }],
        };
        assert!(migrate_legacy_plan(&mut plan));
        assert_eq!(plan.phases[0].name, "pre_ctx");
        assert_eq!(plan.phases[0].config.hooks_enabled, Some(false));
        assert_eq!(plan.phases[1].name, "ctx_warmup");
    }

    #[test]
    fn migrate_inserts_compress_sgr_ab_before_lock_in() {
        let mut plan = ExperimentPlan {
            started_at: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            corpus_path: "/tmp".to_string(),
            mode: "stress_test".to_string(),
            apply_recommendations_on_final_day: false,
            phases: vec![
                ExperimentPhase {
                    name: "compress_ab".to_string(),
                    until_day: 14,
                    config: PhaseConfigPatch::default(),
                },
                ExperimentPhase {
                    name: "lock_in".to_string(),
                    until_day: 15,
                    config: PhaseConfigPatch::default(),
                },
            ],
        };
        assert!(migrate_legacy_plan(&mut plan));
        assert!(plan.phases.iter().any(|p| p.name == "compress_sgr_ab"));
        let sgr = plan
            .phases
            .iter()
            .find(|p| p.name == "compress_sgr_ab")
            .unwrap();
        assert_eq!(sgr.until_day, 15);
        assert_eq!(
            sgr.config.ab_test.as_ref().unwrap().compress_sgr_pct,
            50
        );
        let lock = plan.phases.iter().find(|p| p.name == "lock_in").unwrap();
        assert_eq!(lock.until_day, 16);
    }

    #[test]
    fn active_ab_feature_compress_sgr_phase() {
        let phase = ExperimentPhase {
            name: "compress_sgr_ab".to_string(),
            until_day: 15,
            config: PhaseConfigPatch::default(),
        };
        assert_eq!(active_ab_feature(&phase), Some("compress_sgr"));
    }

    #[test]
    fn phase_day_span_pre_ctx() {
        let plan = sample_plan();
        assert_eq!(phase_day_span(&plan, "pre_ctx"), Some((1, 2)));
        assert_eq!(phase_day_span(&plan, "ctx_warmup"), Some((3, 3)));
    }

    #[test]
    fn experiment_state_survives_ctx_home_wipe() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let backup = tempfile::tempdir().unwrap();
        let prev_home = std::env::var("CTX_HOME").ok();
        let prev_backup = std::env::var("CTX_EXPERIMENT_BACKUP_DIR").ok();
        std::env::set_var("CTX_HOME", tmp.path());
        std::env::set_var("CTX_EXPERIMENT_BACKUP_DIR", backup.path());

        let plan = sample_plan();
        save_plan(&plan).unwrap();
        save_state(&PlanState {
            last_applied_phase: Some("ctx_warmup".into()),
            ..Default::default()
        })
        .unwrap();

        std::fs::remove_dir_all(tmp.path()).unwrap();

        assert!(restore_experiment_state_if_missing().unwrap());
        assert!(plan_path().is_file());
        let restored = load_plan().unwrap();
        assert_eq!(restored.corpus_path, plan.corpus_path);
        assert_eq!(
            load_state().last_applied_phase.as_deref(),
            Some("ctx_warmup")
        );

        match prev_home {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
        match prev_backup {
            Some(v) => std::env::set_var("CTX_EXPERIMENT_BACKUP_DIR", v),
            None => std::env::remove_var("CTX_EXPERIMENT_BACKUP_DIR"),
        }
    }
}
