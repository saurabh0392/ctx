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

/// Spot-check the observation-only richer signals (ADR 0019 / CTX-32). Prints a per-signal
/// count across recent joined decisions, then a sample of decisions for hand-labeling. None of
/// these signals influence the gate yet; this is the proof step that has to pass before they do.
pub fn signal_audit(signal: Option<&str>, limit: usize, json: bool) -> Result<()> {
    let conn = crate::db::open_db()?;
    let _ = crate::db::ensure_schema(&conn);

    // Read a wide window for the counts, regardless of how many samples we print.
    const COUNT_CAP: usize = 5000;
    let all = crate::db::signal_audit_rows(&conn, signal, COUNT_CAP);

    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }

    if all.is_empty() {
        let scope = signal.unwrap_or("any signal");
        println!("No decisions carry {scope} yet.");
        println!("Signals are recorded when ingest joins a transcript outcome. Run `ctx ingest`,");
        println!("or wait for more sessions. (Recording started with ADR 0019; older rows have none.)");
        return Ok(());
    }

    // Count how often each signal fired across the window, so a noisy signal is obvious.
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &all {
        for s in &r.signals {
            *counts.entry(s.clone()).or_default() += 1;
        }
    }
    let scope = signal.unwrap_or("all signals");
    println!(
        "Signal audit for {scope}  ({} joined decisions carry a signal)",
        all.len()
    );
    println!("None of these vote in the gate yet. Hand-label a sample and check precision per signal");
    println!("before promoting any of them (ADR 0019). A false positive here is worse than no signal.");
    println!();
    println!("This corpus excludes ctx's own development activity (CTX-32), so these are your real");
    println!("sessions, not the churn of building ctx. To promote a signal it needs at least 0.8");
    println!("precision on at least 20 hand-labeled samples, and it has to add positives that");
    println!("corrections alone miss.");
    println!();
    println!("How often each signal fired (and whether there is enough to label yet):");
    const PROMOTE_MIN_SAMPLES: usize = 20;
    for (name, n) in &counts {
        let status = if *n >= PROMOTE_MIN_SAMPLES {
            "enough to start labeling".to_string()
        } else {
            format!("not yet, need {} more", PROMOTE_MIN_SAMPLES.saturating_sub(*n))
        };
        println!("   {name:<22} {n:>4}   {status}");
    }
    println!();

    let shown = limit.min(all.len());
    println!("{shown} most recent samples to judge by hand:");
    println!();
    for (i, r) in all.iter().take(limit).enumerate() {
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
        println!(
            "{}. [{}] {} on {}",
            i + 1,
            r.signals.join(" + "),
            r.tool_name,
            target
        );
        println!(
            "   when: {when}   surface: {surface}   kind: {}   gate label: {}",
            r.kind,
            if r.correction { "correction" } else { "clean" }
        );
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
        // The deny-set is absolute: a held tool (ctx's own recovery tools, or a mutation whose
        // harm the re-read/re-edit gate cannot observe) is never trimmed, so a trial would not
        // trim it and would misreport as "on trial". Refuse rather than set a dead slot.
        if crate::compress::is_trim_denied(t, &cfg) {
            let why = crate::compress::held_reason(t, &cfg)
                .unwrap_or_else(|| "it is on the never-trim deny-set".into());
            println!("{t} is held, so a trial would not trim it: {why}");
            return Ok(());
        }
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

/// Cache-safety audit (CTX-28). Prompt caching bills cached-prefix reads at about 0.1x and
/// cache writes at 1.25x to 2x. Filtering MCP tool schemas edits the `tools` block and
/// injecting into the system prompt edits the `system` block; both sit inside the cached
/// prefix and can force a cache write. Tool-output trimming edits content after the prefix,
/// so it is cache-safe by position and is not counted here. This groups the user's own
/// enriched requests by what ctx did to the prefix and reports cache behavior per bucket, so
/// we can see whether prefix edits correlate with more writes and fewer reads. Read-only.
pub fn cache_audit(days: Option<i64>, json: bool) -> Result<()> {
    let conn = crate::db::open_db()?;
    let _ = crate::db::ensure_schema(&conn);
    let since =
        days.map(|d| (chrono::Utc::now() - chrono::Duration::days(d.max(0))).to_rfc3339());
    let buckets = crate::db::cache_audit(&conn, since.as_deref());

    if json {
        println!("{}", serde_json::to_string_pretty(&buckets)?);
        return Ok(());
    }

    if buckets.is_empty() {
        println!("No enriched requests with cache data yet.");
        println!("Run some Claude Code turns, then `ctx ingest`, then try this again.");
        return Ok(());
    }

    println!("Cache-safety audit");
    println!(
        "Prompt caching bills cached-prefix reads at about 0.1x and cache writes at 1.25x to 2x."
    );
    println!("Filtering MCP tool schemas edits the tools block, and injecting into the system");
    println!("prompt edits the system block. Both sit inside the cached prefix, so editing them");
    println!("can force a cache write. Tool-output trimming edits content after the prefix, so it");
    println!("is cache-safe by position and is not counted here.");
    println!();
    println!("This is correlational across your own traffic, not a controlled A/B. Read it as a");
    println!("smell test: if a touched bucket shows a lower cache-read share and a higher");
    println!("cache-write share than untouched, ctx may be busting the cache there.");
    println!();
    println!(
        "  {:<16} {:>8} {:>12} {:>12} {:>10}",
        "bucket", "requests", "cache-read", "cache-write", "fresh"
    );
    for b in &buckets {
        let total = (b.input_tokens + b.cache_read_tokens + b.cache_creation_tokens).max(1);
        let read = b.cache_read_tokens as f64 / total as f64 * 100.0;
        let write = b.cache_creation_tokens as f64 / total as f64 * 100.0;
        let fresh = b.input_tokens as f64 / total as f64 * 100.0;
        println!(
            "  {:<16} {:>8} {:>11.1}% {:>11.1}% {:>9.1}%",
            b.category, b.requests, read, write, fresh
        );
    }
    println!();
    println!("cache-read: share of input served from cache at the discount. Higher is better.");
    println!("cache-write: share re-cached at a premium. Higher in a touched bucket is the warning.");
    println!("fresh: share processed uncached at full price.");

    let arms = crate::db::cache_audit_arms(&conn, since.as_deref());
    if !arms.is_empty() {
        use std::collections::BTreeMap;
        let mut by_feature: BTreeMap<String, (Option<&crate::db::CacheAuditArm>, Option<&crate::db::CacheAuditArm>)> =
            BTreeMap::new();
        for a in &arms {
            let e = by_feature.entry(a.feature.clone()).or_default();
            if a.arm == "treatment" {
                e.0 = Some(a);
            } else {
                e.1 = Some(a);
            }
        }
        println!();
        println!("By experiment arm");
        println!("If an A/B is running, this compares cache behavior with each feature on (treatment)");
        println!("vs off (control) on your own traffic. A feature that busts the cache shows a lower");
        println!("cache-read share and higher cost in treatment. Assignment is per request and the");
        println!("cache is shared across arms, so read this as strong-suggestive, not a clean verdict.");
        for (feature, (t, c)) in by_feature {
            println!();
            println!("  {feature}");
            match (t, c) {
                (Some(t), Some(c)) => {
                    print_arm("    on  (treatment)", t);
                    print_arm("    off (control)  ", c);
                }
                (Some(t), None) => {
                    print_arm("    on  (treatment)", t);
                    println!("    off (control)   none yet. Set this feature's pct to 50 to get a control arm.");
                }
                (None, Some(c)) => {
                    print_arm("    off (control)  ", c);
                    println!("    on  (treatment) none yet.");
                }
                (None, None) => {}
            }
        }
    }
    Ok(())
}

fn print_arm(label: &str, a: &crate::db::CacheAuditArm) {
    let total = (a.input_tokens + a.cache_read_tokens + a.cache_creation_tokens).max(1);
    let read = a.cache_read_tokens as f64 / total as f64 * 100.0;
    let write = a.cache_creation_tokens as f64 / total as f64 * 100.0;
    let avg_cost = if a.requests > 0 {
        a.total_cost / a.requests as f64
    } else {
        0.0
    };
    println!(
        "{label}  n={:<5} cache-read {:>5.1}%  cache-write {:>5.1}%  avg cost ${:.3}",
        a.requests, read, write, avg_cost
    );
}

/// Archive the live DB, then recreate an empty one. Destructive, so it refuses without `yes`.
/// The archive uses the existing `ctx.db.post-wipe-<ts>` name so prior wipe backups and this one
/// sort together. Callers should stop the dashboard first; a fresh schema is written so the file
/// is immediately valid for the next process that opens it.
pub fn reset(yes: bool) -> Result<()> {
    let path = crate::config::db_path();
    if !yes {
        println!("Would archive and wipe {}.", path.display());
        println!("Re-run `ctx context reset --yes` to confirm.");
        return Ok(());
    }
    if path.exists() {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let backup = path.with_file_name(format!("ctx.db.post-wipe-{ts}"));
        std::fs::copy(&path, &backup)?;
        println!("archived -> {}", backup.display());
    }
    // Remove the DB and its WAL sidecars so the next open starts from an empty schema.
    for name in ["ctx.db", "ctx.db-wal", "ctx.db-shm"] {
        let _ = std::fs::remove_file(path.with_file_name(name));
    }
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    println!("fresh ctx.db at {}", path.display());
    Ok(())
}

/// Print the verbatim original of a trim, looked up by its rewind id (CTX-51). Backs the
/// `ctx expand <id>` fallback the trim marker points at; the agent path is the ctx_expand MCP tool.
pub fn expand(id: &str) -> Result<()> {
    let conn = crate::db::open_db()?;
    let _ = crate::db::ensure_schema(&conn);
    match crate::db::get_rewind(&conn, id) {
        Some(e) => {
            crate::db::mark_rewind_expanded(&conn, id);
            println!("{}", e.original);
            Ok(())
        }
        None => anyhow::bail!("No stored output for id \"{id}\"."),
    }
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

#[derive(Serialize)]
struct RepairReport {
    sessions_ingested: usize,
    decisions_joined: usize,
    gate_corrections: usize,
    interrupt_turns_clean: usize,
    model_trained: bool,
}

/// Full corpus repair: re-parse sessions, clean interrupt flags, rejoin labels, retrain.
pub fn repair(skip_ingest: bool, json: bool) -> Result<()> {
    let ingested = if skip_ingest {
        0
    } else {
        crate::conversations::ingest_claude_jsonl(true)?
    };
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let (joined, corrections, interrupt_clean) = crate::db::repair_corpus(&conn)?;
    let model_trained = crate::learn::train()?.is_some();
    let report = RepairReport {
        sessions_ingested: ingested,
        decisions_joined: joined,
        gate_corrections: corrections,
        interrupt_turns_clean: interrupt_clean,
        model_trained,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Corpus repair complete.");
        if !skip_ingest {
            println!("  Re-parsed {ingested} session file(s) with current flag rules.");
        }
        println!("  {joined} decisions joined, {corrections} gate corrections, {interrupt_clean} clean interrupt turns.");
        println!(
            "  Model {}",
            if model_trained {
                "retrained"
            } else {
                "unchanged (not enough labels yet)"
            }
        );
    }
    Ok(())
}
