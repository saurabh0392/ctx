//! Post-ingest A/B comparison and optional auto-apply.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::{ctx_dir, AbTestConfig, Config};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVerdict {
    pub feature: String,
    pub verdict: String,
    pub treatment_count: u64,
    pub control_count: u64,
    pub treatment_avg_cost: f64,
    pub control_avg_cost: f64,
    pub treatment_correction_pct: f64,
    pub control_correction_pct: f64,
    pub delta_cost_pct: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AbResultsFile {
    pub generated_at: String,
    pub experiment_active: bool,
    pub features: Vec<FeatureVerdict>,
    pub auto_applied_log: Vec<String>,
}

pub fn ab_results_path() -> PathBuf {
    ctx_dir().join("ab-results.json")
}

pub fn load_ab_results() -> Option<AbResultsFile> {
    let path = ab_results_path();
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_ab_results(results: &AbResultsFile) -> Result<()> {
    crate::config::ensure_dir()?;
    let content = serde_json::to_string_pretty(results)?;
    std::fs::write(ab_results_path(), content)?;
    Ok(())
}

pub const MIN_SAMPLES: i64 = 100;
const BENEFIT_THRESHOLD_PCT: f64 = 10.0;

fn feature_label(feature: &str) -> String {
    match feature {
        "profile" => "Profile filtering".to_string(),
        "inject" => "System prefix".to_string(),
        "adaptive" => "Adaptive prefix".to_string(),
        "coaching" => "Coaching".to_string(),
        "compress" => "Output compression".to_string(),
        "compress_sgr" => "Session-grounded retention (SGR)".to_string(),
        "tool_mix" => "Semantic tool mix".to_string(),
        _ => feature.to_string(),
    }
}

fn cohort_metrics(
    conn: &rusqlite::Connection,
    feature_letter: char,
    treatment: bool,
) -> Result<(u64, f64, f64)> {
    let flag = if treatment { 'T' } else { 'C' };
    let pattern = format!("%{feature_letter}:{flag}%");
    let (count, avg_cost, correction_rate): (i64, Option<f64>, Option<f64>) = conn.query_row(
        r#"SELECT COUNT(*),
                  AVG(cost_usd),
                  AVG(CASE WHEN coach_kind IS NOT NULL AND coach_kind != '' THEN 1.0 ELSE 0.0 END) * 100.0
           FROM hook_traces
           WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE ?1 AND cost_usd IS NOT NULL"#,
        rusqlite::params![pattern],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((
        count.max(0) as u64,
        avg_cost.unwrap_or(0.0),
        correction_rate.unwrap_or(0.0),
    ))
}

fn build_verdict(feature: &str, _letter: char, t: (u64, f64, f64), c: (u64, f64, f64)) -> FeatureVerdict {
    let label = feature_label(feature);
    let delta = if c.1 > 0.0 {
        Some(((t.1 - c.1) / c.1) * 100.0)
    } else {
        None
    };

    let (verdict, message) = if t.0 < MIN_SAMPLES as u64 || c.0 < MIN_SAMPLES as u64 {
        (
            "insufficient_data",
            format!(
                "{label} has {} treatment and {} control requests. Need at least {MIN_SAMPLES} per group for a reliable comparison. Keep the experiment running.",
                t.0, c.0
            ),
        )
    } else if let Some(d) = delta {
        if d < -BENEFIT_THRESHOLD_PCT {
            (
                "beneficial",
                format!(
                    "{label} saves {:.0}% per request (${:.3} vs ${:.3}). Based on {} treatment vs {} control requests. Keep it enabled.",
                    d.abs(),
                    t.1,
                    c.1,
                    t.0,
                    c.0
                ),
            )
        } else if d > BENEFIT_THRESHOLD_PCT {
            (
                "harmful",
                format!(
                    "{label} increases cost by {:.0}% (${:.3} vs ${:.3}). Review whether it is worth it.",
                    d,
                    t.1,
                    c.1
                ),
            )
        } else {
            (
                "no_benefit",
                format!(
                    "{label} shows no meaningful cost difference after {} requests (${:.3} treatment vs ${:.3} control, within noise). Consider disabling to simplify the pipeline.",
                    t.0 + c.0,
                    t.1,
                    c.1
                ),
            )
        }
    } else {
        (
            "insufficient_data",
            format!("{label}: no cost data on enriched rows yet."),
        )
    };

    FeatureVerdict {
        feature: feature.to_string(),
        verdict: verdict.to_string(),
        treatment_count: t.0,
        control_count: c.0,
        treatment_avg_cost: t.1,
        control_avg_cost: c.1,
        treatment_correction_pct: t.2,
        control_correction_pct: c.2,
        delta_cost_pct: delta,
        message,
    }
}

pub fn run_tuning_after_ingest(conn: &rusqlite::Connection) -> Result<Option<AbResultsFile>> {
    let cfg = Config::load();
    match &cfg.ab_test {
        Some(ab) if experiment_active(ab) => {}
        _ => return Ok(load_ab_results()),
    };

    let features_spec = [
        ("profile", 'P'),
        ("inject", 'I'),
        ("adaptive", 'A'),
        ("coaching", 'C'),
        ("compress", 'X'),
        ("compress_sgr", 'S'),
        ("tool_mix", 'M'),
    ];
    let mut features = Vec::new();
    for (name, letter) in features_spec {
        let t = cohort_metrics(conn, letter, true)?;
        let c = cohort_metrics(conn, letter, false)?;
        features.push(build_verdict(name, letter, t, c));
    }

    let results = AbResultsFile {
        generated_at: chrono::Utc::now().to_rfc3339(),
        experiment_active: true,
        features,
        auto_applied_log: Vec::new(),
    };
    save_ab_results(&results)?;

    if cfg.auto_apply_recommendations {
        let mut results = results;
        apply_recommendations_inner(&mut results, true)?;
        save_ab_results(&results)?;
        return Ok(Some(results));
    }

    Ok(Some(results))
}

fn experiment_active(ab: &AbTestConfig) -> bool {
    ab.profile_pct < 100
        || ab.inject_pct < 100
        || ab.adaptive_pct < 100
        || ab.coaching_pct < 100
        || ab.compress_pct < 100
        || ab.compress_sgr_pct < 100
        || ab.tool_mix_pct < 100
}

pub fn apply_recommendations() -> Result<()> {
    let mut results = load_ab_results().ok_or_else(|| anyhow::anyhow!("no ab-results.json yet"))?;
    apply_recommendations_inner(&mut results, false)?;
    save_ab_results(&results)?;
    println!("Applied recommendations to config.toml");
    Ok(())
}

fn apply_recommendations_inner(results: &mut AbResultsFile, auto: bool) -> Result<()> {
    let mut cfg = Config::load();
    let mut log = Vec::new();

    for f in &results.features {
        match f.verdict.as_str() {
            "beneficial" => {}
            "no_benefit" | "harmful" => match f.feature.as_str() {
                "profile" => {
                    // keep profile filtering on; only disable optional gates
                }
                "inject" => {
                    if cfg.inject_enabled {
                        cfg.inject_enabled = false;
                        log.push(format!(
                            "Disabled system prefix injection ({})",
                            if auto { "auto" } else { "manual" }
                        ));
                    }
                }
                "adaptive" => {
                    if cfg.adaptive_prefix_enabled {
                        cfg.adaptive_prefix_enabled = false;
                        log.push(format!(
                            "Disabled adaptive prefix ({})",
                            if auto { "auto" } else { "manual" }
                        ));
                    }
                }
                "coaching" => {
                    if cfg.coaching_enabled {
                        cfg.coaching_enabled = false;
                        log.push(format!(
                            "Disabled coaching ({})",
                            if auto { "auto" } else { "manual" }
                        ));
                    }
                }
                "compress" => {
                    if cfg.compress_enabled {
                        cfg.compress_enabled = false;
                        log.push(format!(
                            "Disabled output compression ({})",
                            if auto { "auto" } else { "manual" }
                        ));
                    }
                }
                "compress_sgr" => {
                    if cfg.compress_sgr_enabled {
                        cfg.compress_sgr_enabled = false;
                        log.push(format!(
                            "Disabled session-grounded retention ({})",
                            if auto { "auto" } else { "manual" }
                        ));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    cfg.ab_test = None;
    cfg.save()?;
    results.experiment_active = false;
    results.auto_applied_log.extend(log);
    Ok(())
}

pub fn reset_experiment() -> Result<()> {
    let path = ab_results_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    println!("Cleared ab-results.json");
    Ok(())
}

pub fn print_experiment_status() -> Result<()> {
    let cfg = Config::load();
    if let Some(ab) = &cfg.ab_test {
        println!(
            "Experiment active: profile {}%, inject {}%, adaptive {}%, coaching {}%, compress {}%, sgr {}%",
            ab.profile_pct, ab.inject_pct, ab.adaptive_pct, ab.coaching_pct, ab.compress_pct, ab.compress_sgr_pct
        );
    } else {
        println!("No experiment configured (all features at 100%)");
    }
    if let Some(r) = load_ab_results() {
        println!("Results from {}", r.generated_at);
        for f in &r.features {
            println!("  [{}] {}", f.verdict, f.message);
        }
        for line in &r.auto_applied_log {
            println!("  auto: {line}");
        }
    } else {
        println!("No ab-results.json yet");
    }
    Ok(())
}
