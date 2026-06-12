//! `ctx context` command surface: the self-learning context controller's CLI.
//!
//! This is the user-facing control for the Act 0 collection window and the Act 1
//! activation. It never invents numbers: everything comes from `compress_decisions`.

use anyhow::Result;
use serde::Serialize;

use crate::compress::activation::{causal_clears_bar, CausalThresholds};
use crate::config::{CompressPreset, Config};

#[derive(Serialize)]
struct ToolStatus {
    tool: String,
    decisions: i64,
    joined: i64,
    clean_runs: i64,
    corrections: i64,
    rereads: i64,
    earned: bool,
    active: bool,
}

#[derive(Serialize)]
struct ContextStatus {
    preset: String,
    shadow_enabled: bool,
    total_decisions: i64,
    joined: i64,
    today: i64,
    corrections_caused: i64,
    shadow_only: i64,
    active: i64,
    trial_tools: Vec<String>,
    tools: Vec<ToolStatus>,
}

fn gather() -> ContextStatus {
    let cfg = Config::load();
    let conn = crate::db::open_db().ok();
    let (stats, progress, causal) = match conn {
        Some(c) => {
            let _ = crate::db::ensure_schema(&c);
            (
                crate::db::compress_decision_stats(&c),
                crate::db::compress_tool_progress(&c),
                crate::db::causal_tool_outcomes(&c, None),
            )
        }
        None => Default::default(),
    };
    let th = CausalThresholds::default();
    let tools = progress
        .into_iter()
        .map(|p| {
            // "ready" means earned to trim: causal evidence that trimming this tool is not
            // measurably worse than leaving it alone. Fails closed until a trial collects the
            // trimmed arm, so we never label a tool ready on baseline counts alone.
            let earned = causal
                .iter()
                .find(|o| o.tool_name == p.tool_name)
                .map(|o| causal_clears_bar(o, &th))
                .unwrap_or(false);
            ToolStatus {
                tool: p.tool_name.clone(),
                decisions: p.decisions,
                joined: p.joined,
                clean_runs: p.clean_runs,
                corrections: p.corrections,
                rereads: p.rereads,
                earned,
                active: p.active,
            }
        })
        .collect();

    ContextStatus {
        preset: cfg.compress_preset.as_str().to_string(),
        shadow_enabled: cfg.compress_shadow_enabled,
        total_decisions: stats.total,
        joined: stats.joined,
        today: stats.today,
        corrections_caused: stats.corrections_caused,
        shadow_only: stats.shadow,
        active: stats.active,
        trial_tools: cfg.compress_trial_tools.clone(),
        tools,
    }
}

pub fn status(json: bool) -> Result<()> {
    let s = gather();
    if json {
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }

    println!("ctx context controller");
    println!(
        "  preset: {}  (shadow collection: {})",
        s.preset,
        if s.shadow_enabled { "on" } else { "off" }
    );
    if s.trial_tools.is_empty() {
        println!("  live trim trial: none (no tool is being trimmed for the before/after)");
    } else {
        println!(
            "  live trim trial: {} (being trimmed live to collect the after arm)",
            s.trial_tools.join(", ")
        );
    }
    println!();
    println!("Learning");
    println!("  decisions recorded:   {}", s.total_decisions);
    println!("  with an outcome yet:  {}", s.joined);
    println!("  recorded today:       {}", s.today);
    println!("  corrections caused:   {}", s.corrections_caused);
    if s.total_decisions == 0 {
        println!();
        println!("  No decisions yet. Run some Claude Code turns, then `ctx ingest`.");
        return Ok(());
    }
    println!();
    println!("Per tool");
    for t in &s.tools {
        let state = if t.active {
            "on"
        } else if t.earned {
            "ready"
        } else {
            "watching"
        };
        println!(
            "  {:<28} {:>5} seen  {:>5} judged  {:>4} clean  {:>3} corrections  [{}]",
            t.tool, t.decisions, t.joined, t.clean_runs, t.corrections, state
        );
    }
    Ok(())
}

use crate::stats::{newcombe_diff, wilson_interval};

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

/// Causal before/after proof for one tool (E1.3 / SAU-150). Compares the correction and
/// re-read rate on decisions where ctx wanted to trim, split by whether the trim was
/// actually applied. The "after" stays empty until the tool is deliberately activated, so
/// this honestly reads "not yet" rather than inventing a result.
pub fn proof(tool: Option<&str>, json: bool) -> Result<()> {
    let conn = crate::db::open_db()?;
    let _ = crate::db::ensure_schema(&conn);
    let rows = crate::db::causal_tool_outcomes(&conn, tool);

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    let scope = tool.unwrap_or("all tools");
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| r.baseline_n > 0 || r.trimmed_n > 0)
        .collect();
    if rows.is_empty() {
        println!("No would-trim decisions collected yet for {scope}.");
        println!("ctx only has a before/after to show once the heuristic wants to trim a tool. Keep using your agents.");
        return Ok(());
    }

    println!("Causal before/after for {scope}");
    println!(
        "We look only at runs where ctx wanted to trim. Baseline is when we left the output alone."
    );
    println!("Trimmed is when we actually cut it. A correction or re-read soon after is the cost of cutting.");
    println!("Trimming is safe for a tool only when the trimmed rate is not higher than baseline.");
    println!();

    for r in &rows {
        println!("{}", r.tool_name);
        let (bc_lo, bc_hi) = wilson_interval(r.baseline_corrections, r.baseline_n);
        let (br_lo, br_hi) = wilson_interval(r.baseline_rereads, r.baseline_n);
        if r.baseline_n > 0 {
            let bc = r.baseline_corrections as f64 / r.baseline_n as f64;
            let br = r.baseline_rereads as f64 / r.baseline_n as f64;
            println!(
                "  baseline (left alone)  n={:<4}  corrections {} [{}, {}]   re-reads {} [{}, {}]",
                r.baseline_n,
                pct(bc),
                pct(bc_lo),
                pct(bc_hi),
                pct(br),
                pct(br_lo),
                pct(br_hi)
            );
        } else {
            println!("  baseline (left alone)  n=0     not yet");
        }

        if r.trimmed_n > 0 {
            let tc = r.trimmed_corrections as f64 / r.trimmed_n as f64;
            let tr = r.trimmed_rereads as f64 / r.trimmed_n as f64;
            let (tc_lo, tc_hi) = wilson_interval(r.trimmed_corrections, r.trimmed_n);
            let (tr_lo, tr_hi) = wilson_interval(r.trimmed_rereads, r.trimmed_n);
            println!(
                "  trimmed (cut)          n={:<4}  corrections {} [{}, {}]   re-reads {} [{}, {}]",
                r.trimmed_n,
                pct(tc),
                pct(tc_lo),
                pct(tc_hi),
                pct(tr),
                pct(tr_lo),
                pct(tr_hi)
            );
            let (dc, dc_lo, dc_hi) = newcombe_diff(
                r.trimmed_corrections,
                r.trimmed_n,
                r.baseline_corrections,
                r.baseline_n,
            );
            let (dr, dr_lo, dr_hi) = newcombe_diff(
                r.trimmed_rereads,
                r.trimmed_n,
                r.baseline_rereads,
                r.baseline_n,
            );
            let verdict = |lo: f64, hi: f64| -> &'static str {
                if hi <= 0.0 {
                    "trimming looks safe so far"
                } else if lo > 0.0 {
                    "trimming looks harmful, keep it off"
                } else {
                    "too close to call, need more trimmed runs"
                }
            };
            println!(
                "  correction delta (trimmed minus baseline): {} [{}, {}]  {}",
                signed_pct(dc),
                signed_pct(dc_lo),
                signed_pct(dc_hi),
                verdict(dc_lo, dc_hi)
            );
            println!(
                "  re-read delta:                              {} [{}, {}]  {}",
                signed_pct(dr),
                signed_pct(dr_lo),
                signed_pct(dr_hi),
                verdict(dr_lo, dr_hi)
            );
        } else {
            println!(
                "  trimmed (cut)          n=0     not yet. Nothing has been trimmed for this tool."
            );
            println!(
                "  We cannot show an honest after until ctx actually trims it during real use."
            );
        }
        println!();
    }
    Ok(())
}

fn signed_pct(x: f64) -> String {
    format!("{:+.1}%", x * 100.0)
}

/// Read-only label audit (E0.3 / SAU-148): pull recent positive-labeled decisions and the
/// raw evidence behind each label, so we can judge by hand whether a "correction" or
/// "re-read" is real context harm or just noise from normal work.
pub fn labels(tool: Option<&str>, limit: usize, json: bool) -> Result<()> {
    let conn = crate::db::open_db()?;
    let _ = crate::db::ensure_schema(&conn);
    let rows = crate::db::audit_labeled_decisions(&conn, tool, limit);

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    let scope = tool.unwrap_or("all tools");
    if rows.is_empty() {
        println!("No positive labels yet for {scope}.");
        println!("Either nothing has been flagged a correction or re-read, or ingest has not joined outcomes. Run `ctx ingest`.");
        return Ok(());
    }

    println!(
        "Label audit for {scope}  (showing {} of the most recent positive labels)",
        rows.len()
    );
    println!(
        "Each label is what ctx would score as harm. Read the evidence and judge if it really is."
    );
    println!();

    for (i, r) in rows.iter().enumerate() {
        let mut kinds = Vec::new();
        if r.correction {
            kinds.push("correction");
        }
        if r.reread {
            kinds.push("re-read");
        }
        let label = kinds.join(" + ");
        let when = local_short(&r.ts);
        let target = r
            .command_or_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("(none)");
        let surface = r
            .surface
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("claude-code");
        println!("{}. [{}] {} on {}", i + 1, label, r.tool_name, target);
        println!("   when: {when}   surface: {surface}   kind: {}", r.kind);

        for ev in &r.correction_evidence {
            let snippet = one_line(&ev.text, 160);
            let body = if snippet.is_empty() {
                "(no prompt text stored)".to_string()
            } else {
                snippet
            };
            println!("   correction +{:.0} min: {}", ev.minutes_after, body);
        }
        for ev in &r.reread_evidence {
            println!(
                "   re-read   +{:.0} min: {} hit the same target again",
                ev.minutes_after, ev.tool_name
            );
        }
        println!();
    }
    Ok(())
}

fn local_short(ts: &str) -> String {
    use chrono::{DateTime, Local, Utc};
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return dt.with_timezone(&Local).format("%b %d %H:%M").to_string();
    }
    if let Ok(dt) = ts.parse::<DateTime<Utc>>() {
        return dt.with_timezone(&Local).format("%b %d %H:%M").to_string();
    }
    ts.chars().take(16).collect()
}

fn one_line(s: &str, max: usize) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max {
        let head: String = collapsed.chars().take(max).collect();
        format!("{head}...")
    } else {
        collapsed
    }
}

/// Start or stop a deliberate trim trial (SAU-150). A trialed tool is trimmed live even while
/// the preset stays off and the evidence gate is unmet, which is the only honest way to gather
/// the trimmed "after" arm. We keep it scoped to one tool at a time on purpose: a trial is a
/// real intervention on the user's live output, so it should be explicit and narrow.
pub fn trial(tool: Option<&str>, on: bool, off: bool) -> Result<()> {
    let mut cfg = Config::load();

    if off {
        match tool {
            Some(t) => {
                cfg.compress_trial_tools.retain(|x| x != t);
                cfg.save()?;
                println!("Stopped the trim trial for {t}. It is back to shadow only (recorded, not changed).");
            }
            None => {
                let had = cfg.compress_trial_tools.clone();
                cfg.compress_trial_tools.clear();
                cfg.save()?;
                if had.is_empty() {
                    println!("No trim trial was running. Nothing changed.");
                } else {
                    println!(
                        "Stopped all trim trials ({}). Everything is back to shadow only.",
                        had.join(", ")
                    );
                }
            }
        }
        return Ok(());
    }

    if on {
        let Some(t) = tool else {
            anyhow::bail!("name the tool to trial, e.g. `ctx context trial Read --on`");
        };
        if !cfg.compress_tools.iter().any(|x| x == t) {
            println!("Heads up: {t} is not in compress_tools, so the heuristic may rarely want to trim it.");
        }
        // One tool at a time. Replace any prior trial so the before/after stays clean.
        if !cfg.compress_trial_tools.is_empty() && cfg.compress_trial_tools != vec![t.to_string()] {
            println!(
                "Replacing the previous trial ({}).",
                cfg.compress_trial_tools.join(", ")
            );
        }
        cfg.compress_trial_tools = vec![t.to_string()];
        cfg.save()?;
        println!("Trial on for {t}. ctx will trim it live whenever the heuristic wants to, even with the preset off.");
        println!("This builds the trimmed side of the before/after. Watch it with `ctx context proof --tool {t}`.");
        println!("Stop any time with `ctx context trial {t} --off`.");
        return Ok(());
    }

    // No flag: report current state.
    if cfg.compress_trial_tools.is_empty() {
        println!("No trim trial is running. Start one with `ctx context trial <Tool> --on`.");
    } else {
        println!(
            "Trim trial running for: {}",
            cfg.compress_trial_tools.join(", ")
        );
        println!(
            "Stop with `ctx context trial <Tool> --off`, or see results with `ctx context proof`."
        );
    }
    Ok(())
}

pub fn set_preset(value: &str) -> Result<()> {
    let preset = CompressPreset::parse(value)
        .ok_or_else(|| anyhow::anyhow!("unknown preset '{value}' (use off, safe, or full)"))?;
    let mut cfg = Config::load();
    cfg.compress_preset = preset;
    cfg.save()?;
    match preset {
        CompressPreset::Off => {
            println!("Compression is off. ctx keeps watching and recording decisions, but does not change tool output.");
        }
        CompressPreset::Safe => {
            println!("Safe preset on. ctx will trim git, test, and grep output once each clears its evidence bar.");
        }
        CompressPreset::Full => {
            println!("Full preset on. ctx will trim every supported tool once it clears its evidence bar.");
        }
    }
    if !cfg.compress_force_active && preset != CompressPreset::Off {
        println!("Tools still activate only after your own runs prove them safe. See `ctx context status`.");
    }
    Ok(())
}
