//! Dry-run pipeline simulation. Mirrors `hook::user_prompt_submit()` without side effects.

use anyhow::Result;
use serde::Serialize;
use crate::analytics::{CACHE_READ_RATE_PER_MTOK, WORST_CASE_INPUT_RATE_PER_MTOK};
use crate::profiles::{self, TOTAL_TOOLS};

const TOKENS_PER_TOOL: f64 = 600.0;

#[derive(Debug, Clone, Serialize)]
pub struct SimulateResult {
    pub profile_slug: String,
    pub effective_profile: String,
    pub auto_selected: bool,
    pub auto_trigger: Option<String>,
    pub tools_kept: usize,
    pub tools_removed: usize,
    pub tokens_saved: usize,
    pub inject_fired: bool,
    pub inject_chars: usize,
    pub adaptive_fired: bool,
    pub adaptive_chars: usize,
    pub coaching_fired: bool,
    pub coach_kind: Option<String>,
    pub coach_suggestion: Option<String>,
    pub budget_blocked: bool,
    pub budget_reason: Option<String>,
    pub fatigue_blocked: bool,
    pub additional_context: String,
    pub estimated_cost_with_ctx: f64,
    pub estimated_cost_without_ctx: f64,
    pub savings_usd: f64,
    pub savings_pct: f64,
}

pub fn simulate_pipeline(
    cwd: &str,
    prompt: &str,
    session_id: Option<&str>,
    model_hint: Option<&str>,
    profile_override: Option<&str>,
) -> Result<SimulateResult> {
    let cfg = crate::config::Config::load();
    let pseudo_system = format!("Primary working directory: {cwd}\n");

    let base_profile = cfg.active_profile.as_deref().unwrap_or("all").to_string();
    let mut auto_selected = false;
    let mut auto_trigger: Option<String> = None;
    let effective_profile = if let Some(ovr) = profile_override {
        ovr.to_string()
    } else if cfg.auto_profile_enabled {
        if let Some((slug, trigger)) = profiles::auto_select(&pseudo_system, &base_profile) {
            auto_selected = true;
            auto_trigger = Some(trigger);
            slug
        } else {
            base_profile.clone()
        }
    } else {
        base_profile.clone()
    };

    let all = profiles::load_all();
    let (tools_kept, tools_removed, tokens_saved) = if let Some(p) = all.get(&effective_profile) {
        let kept = p.tool_count();
        let removed = TOTAL_TOOLS.saturating_sub(kept);
        let saved = p.savings_vs_all();
        (kept, removed, saved)
    } else {
        (TOTAL_TOOLS, 0, 0)
    };

    let budget_reason = crate::budget_guard::hard_block_reason_for_prompt(prompt);
    let budget_blocked = budget_reason.is_some();

    let coaching_texts = if cfg.coaching_enabled {
        crate::hook::coaching_user_texts_public(session_id, prompt)
    } else {
        Vec::new()
    };

    let fatigue_blocked = cfg.coaching_enabled
        && crate::coach::severe_correction_fatigue_reason(&coaching_texts).is_some();

    let mut inject_fired = false;
    let mut inject_chars = 0usize;
    let mut extra = String::new();
    if cfg.inject_enabled {
        if let Some(prefix) = crate::inject::load_prefix() {
            let trimmed = prefix.trim();
            inject_chars = trimmed.chars().count();
            extra.push_str(trimmed);
            extra.push_str("\n\n");
            inject_fired = true;
        }
    }

    let mut coach_kind: Option<String> = None;
    let mut coach_suggestion: Option<String> = None;
    let coaching_fired;
    if cfg.coaching_enabled && !fatigue_blocked {
        if let Some(sig) = crate::coach::detect_from_user_texts(&coaching_texts) {
            extra.push_str(sig.suggestion.trim());
            extra.push_str("\n\n");
            coach_kind = Some(match sig.kind {
                crate::coach::SignalKind::CorrectionCascade => "correction-cascade".to_string(),
                crate::coach::SignalKind::ReAsk => "re-ask".to_string(),
            });
            coach_suggestion = Some(sig.suggestion);
            coaching_fired = true;
        } else {
            coaching_fired = false;
        }
    } else {
        coaching_fired = false;
    }

    let max_adaptive = crate::adaptive::max_chars_for_hook_input(model_hint);
    let mut adaptive_fired = false;
    let mut adaptive_chars = 0usize;
    if cfg.adaptive_prefix_enabled {
        if let Some(ad) = crate::adaptive::load_adaptive_prefix() {
            let trimmed = crate::adaptive::truncate_to_char_budget(ad.trim(), max_adaptive);
            if !trimmed.is_empty() {
                adaptive_chars = trimmed.chars().count();
                extra.push_str(&trimmed);
                extra.push_str("\n\n");
                adaptive_fired = true;
            }
        }
    }

    let total_tokens_all = TOTAL_TOOLS as f64 * TOKENS_PER_TOOL;
    let kept_tokens = tools_kept as f64 * TOKENS_PER_TOOL;
    let cost_without = total_tokens_all / 1_000_000.0 * WORST_CASE_INPUT_RATE_PER_MTOK;
    let cost_with = kept_tokens / 1_000_000.0 * WORST_CASE_INPUT_RATE_PER_MTOK
        + tokens_saved as f64 / 1_000_000.0 * CACHE_READ_RATE_PER_MTOK;
    let savings_usd = cost_without - cost_with;
    let savings_pct = if cost_without > 0.0 {
        savings_usd / cost_without * 100.0
    } else {
        0.0
    };

    let display = all
        .get(&effective_profile)
        .map(|p| p.display.clone())
        .unwrap_or_else(|| effective_profile.clone());

    Ok(SimulateResult {
        profile_slug: effective_profile,
        effective_profile: display,
        auto_selected,
        auto_trigger,
        tools_kept,
        tools_removed,
        tokens_saved,
        inject_fired,
        inject_chars,
        adaptive_fired,
        adaptive_chars,
        coaching_fired,
        coach_kind,
        coach_suggestion,
        budget_blocked,
        budget_reason,
        fatigue_blocked,
        additional_context: extra.trim_end().to_string(),
        estimated_cost_with_ctx: cost_with,
        estimated_cost_without_ctx: cost_without,
        savings_usd,
        savings_pct,
    })
}

pub fn simulate_all_profiles(
    cwd: &str,
    prompt: &str,
    session_id: Option<&str>,
    model_hint: Option<&str>,
) -> Result<Vec<SimulateResult>> {
    let all = profiles::load_all();
    let mut names: Vec<String> = all.keys().cloned().collect();
    names.sort();
    if !names.contains(&"all".to_string()) {
        names.insert(0, "all".to_string());
    }
    let mut results = Vec::new();
    for name in &names {
        results.push(simulate_pipeline(cwd, prompt, session_id, model_hint, Some(name))?);
    }
    results.sort_by(|a, b| {
        b.savings_pct
            .partial_cmp(&a.savings_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results)
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayComparison {
    pub trace_id: i64,
    pub trace_ts: String,
    pub trace_profile: String,
    pub trace_tools_kept: usize,
    pub trace_tools_removed: usize,
    pub trace_tokens_saved: usize,
    pub trace_cost_usd: Option<f64>,
    pub simulated: SimulateResult,
}

pub fn replay_last_traces(n: usize) -> Result<Vec<ReplayComparison>> {
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let traces = crate::db::load_hook_traces(&conn, n, 0, None)?;
    let mut results = Vec::new();
    for t in traces {
        let sim = simulate_pipeline(
            &t.working_directory,
            "",
            t.session_id.as_deref(),
            t.model.as_deref(),
            Some(&t.profile),
        )?;
        results.push(ReplayComparison {
            trace_id: t.id,
            trace_ts: t.ts,
            trace_profile: t.profile,
            trace_tools_kept: t.tools_kept,
            trace_tools_removed: t.tools_removed,
            trace_tokens_saved: t.tokens_saved,
            trace_cost_usd: t.cost_usd,
            simulated: sim,
        });
    }
    Ok(results)
}

pub fn format_result(r: &SimulateResult) -> String {
    let mut out = String::new();
    out.push_str("ctx simulate -- dry-run pipeline result\n");
    out.push_str(&"-".repeat(40));
    out.push('\n');
    out.push('\n');

    out.push_str(&format!(
        "PROFILE       {} ({}){}\n",
        r.profile_slug,
        r.effective_profile,
        if r.auto_selected {
            format!(
                " auto-selected from cwd: {}",
                r.auto_trigger.as_deref().unwrap_or("matched")
            )
        } else {
            String::new()
        }
    ));
    let total = r.tools_kept + r.tools_removed;
    let pct = if total > 0 {
        r.tools_removed * 100 / total
    } else {
        0
    };
    out.push_str(&format!(
        "TOOLS         {} kept, {} stripped ({}% cut)\n",
        r.tools_kept, r.tools_removed, pct
    ));
    out.push_str(&format!(
        "TOKENS SAVED  ~{} per request\n\n",
        fmt_k(r.tokens_saved)
    ));

    out.push_str("GATES FIRED\n");
    gate_line(
        &mut out,
        "Auto-Profile",
        r.auto_selected,
        if r.auto_selected {
            format!(
                "matched {} -> {}",
                r.auto_trigger.as_deref().unwrap_or("cwd"),
                r.profile_slug
            )
        } else {
            "no switch".to_string()
        },
    );
    gate_line(
        &mut out,
        "Profile Filter",
        r.tools_removed > 0,
        if r.tools_removed > 0 {
            format!("-{} tools", r.tools_removed)
        } else {
            "no tools stripped".to_string()
        },
    );
    gate_line(
        &mut out,
        "Inject",
        r.inject_fired,
        if r.inject_fired {
            format!("system_prefix.md ({} chars)", r.inject_chars)
        } else {
            "not active".to_string()
        },
    );
    gate_line(
        &mut out,
        "Adaptive",
        r.adaptive_fired,
        if r.adaptive_fired {
            format!("adaptive_prefix.md ({} chars)", r.adaptive_chars)
        } else {
            "not active".to_string()
        },
    );
    gate_line(
        &mut out,
        "Coaching",
        r.coaching_fired,
        r.coach_kind
            .as_deref()
            .unwrap_or("no signal detected")
            .to_string(),
    );
    gate_line(
        &mut out,
        "Budget Guard",
        r.budget_blocked,
        if r.budget_blocked {
            "BLOCKED".to_string()
        } else {
            "within budget".to_string()
        },
    );
    if r.fatigue_blocked {
        gate_line(&mut out, "Fatigue", true, "session would be blocked".to_string());
    }
    out.push('\n');

    if !r.additional_context.is_empty() {
        let preview: String = r.additional_context.chars().take(500).collect();
        out.push_str(&format!(
            "INJECTED CONTEXT ({} chars total)\n  {}\n\n",
            r.additional_context.chars().count(),
            preview.replace('\n', "\n  ")
        ));
    }

    out.push_str("COST ESTIMATE (per request)\n");
    out.push_str(&format!("  Without ctx:  ${:.3}\n", r.estimated_cost_without_ctx));
    out.push_str(&format!("  With ctx:     ${:.3}\n", r.estimated_cost_with_ctx));
    out.push_str(&format!("  Savings:      ${:.3}  ({:.0}%)\n", r.savings_usd, r.savings_pct));
    out
}

pub fn format_all_profiles(results: &[SimulateResult], cwd: &str, prompt: &str) -> String {
    let mut out = String::new();
    out.push_str("ctx simulate --all-profiles -- profile comparison\n");
    out.push_str(&"-".repeat(50));
    out.push('\n');
    let prompt_preview: String = prompt.chars().take(60).collect();
    out.push_str(&format!("\nPROMPT  \"{prompt_preview}\"\nCWD     {cwd}\n\n"));
    out.push_str(&format!(
        "  {:12} {:>6} {:>9} {:>13} {:>10} {:>8}\n",
        "Profile", "Tools", "Stripped", "Tokens Saved", "Est. Cost", "Savings"
    ));
    out.push_str(&format!(
        "  {:12} {:>6} {:>9} {:>13} {:>10} {:>8}\n",
        "-".repeat(12),
        "-".repeat(6),
        "-".repeat(9),
        "-".repeat(13),
        "-".repeat(10),
        "-".repeat(8),
    ));
    for r in results {
        out.push_str(&format!(
            "  {:12} {:>6} {:>9} {:>13} {:>10} {:>7.0}%\n",
            r.profile_slug,
            r.tools_kept,
            r.tools_removed,
            fmt_k(r.tokens_saved),
            format!("${:.3}", r.estimated_cost_with_ctx),
            r.savings_pct,
        ));
    }
    if let Some(best) = results.first() {
        if best.savings_pct > 0.0 {
            out.push_str(&format!(
                "\nBEST FIT  {} ({:.0}% savings)\n",
                best.profile_slug, best.savings_pct
            ));
        }
    }
    out
}

pub fn format_replay(comparisons: &[ReplayComparison]) -> String {
    let mut out = String::new();
    out.push_str("ctx simulate --replay-last -- actual vs simulated\n");
    out.push_str(&"-".repeat(50));
    out.push('\n');
    out.push_str(&format!(
        "\n  {:>5} {:20} {:10} {:>8} {:>8} {:>8} {:>8}\n",
        "ID", "Timestamp", "Profile", "Kept(A)", "Kept(S)", "Saved(A)", "Saved(S)"
    ));
    for c in comparisons {
        let ts: String = c.trace_ts.chars().take(19).collect();
        out.push_str(&format!(
            "  {:>5} {:20} {:10} {:>8} {:>8} {:>8} {:>8}\n",
            c.trace_id,
            ts,
            c.trace_profile,
            c.trace_tools_kept,
            c.simulated.tools_kept,
            fmt_k(c.trace_tokens_saved),
            fmt_k(c.simulated.tokens_saved),
        ));
    }
    out
}

fn gate_line(out: &mut String, name: &str, fired: bool, detail: String) {
    let icon = if fired { "+" } else { "." };
    out.push_str(&format!("  {icon} {:16} {detail}\n", name));
}

fn fmt_k(n: usize) -> String {
    if n >= 1000 {
        format!("{},{:03}", n / 1000, n % 1000)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulate_all_profile_has_zero_removed() {
        std::env::set_var("CTX_HOME", "/tmp/ctx-sim-test-nonexist");
        let r = simulate_pipeline("/tmp", "test", None, None, Some("all")).unwrap();
        assert_eq!(r.tools_removed, 0);
        assert_eq!(r.tools_kept, TOTAL_TOOLS);
    }

    #[test]
    fn cost_estimates_positive() {
        std::env::set_var("CTX_HOME", "/tmp/ctx-sim-test-nonexist");
        let r = simulate_pipeline("/tmp", "test", None, None, Some("carrier")).unwrap();
        assert!(r.estimated_cost_without_ctx > 0.0);
        assert!(r.estimated_cost_with_ctx > 0.0);
        assert!(r.savings_usd >= 0.0);
    }
}
