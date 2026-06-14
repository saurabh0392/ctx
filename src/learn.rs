//! Act 1: train a tiny local outcome model from forward-collected labels.
//!
//! The model predicts P(this retention decision precedes a correction) from the
//! aggregate decision features. It is deterministic, sub-ms, and never calls an LLM.
//! Training is volume-gated: with too few labels we say so rather than ship a guess.
//!
//! The honesty gate (Act 1) lives here too: per tool, the observed correction and
//! re-read rate on the user's own joined labels. A tool only earns activation when
//! those rates clear the bar. We report the numbers; we do not assert a win.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::compress::activation::{activation_clears_bar, ActivationThresholds};
use crate::db::LabeledDecision;

/// Minimum joined labels before we attempt to fit a model at all.
const MIN_LABELS_TO_TRAIN: usize = 100;

/// Minimum correction-labeled rows before a fit can mean anything. A handful of positives
/// cannot teach a model what a correction looks like; without this a near-all-clean label
/// set yields a model that just predicts "clean" and scores a meaningless holdout.
const MIN_POSITIVE_LABELS: usize = 15;

/// The held-out model must beat a coin flip by this margin to be kept. A model at or near
/// 0.5 has learned nothing, so we refuse to serve it as "trained" no matter how many
/// labels exist. This is the Act 1 honesty gate, enforced rather than just reported.
const MIN_HOLDOUT_AUC: f64 = 0.60;

const FEATURE_NAMES: &[&str] = &[
    "drop_ratio",
    "kept_ratio",
    "risky_drop_ratio",
    "log_lines",
    "drop_failure_ratio",
    "drop_focus_path_ratio",
    "drop_focus_symbol_ratio",
    "drop_correction_ratio",
    "drop_prompt_kw_ratio",
    "drop_dedup_ratio",
    "drop_boilerplate_ratio",
    // Tool-kind one-hot (ADR 0007 / CTX-17). Without these the model is blind to which tool it is
    // trimming, so it just mirrors the heuristic. With them it can learn that trimming a read is
    // riskier than trimming a grep, which is what lets it diverge from the rules. Available for every
    // historical label (kind is stored per decision), so it takes effect on the next retrain.
    "kind_read",
    "kind_grep",
    "kind_test",
    "kind_git_status",
    "kind_git_diff",
    "kind_git_log",
    "kind_mcp",
    "kind_generic",
    // Path-role one-hot (ADR 0030 / CTX-46). The model was blind to *which file* a read touched; it
    // could not learn "reads of src get edited, reads of vendored code don't." `path_role` is logged
    // per read decision from CTX-45 on; historical rows without it contribute an all-zero block,
    // which is a safe default, so this takes effect as file-tagged data accrues.
    "role_src",
    "role_test",
    "role_config",
    "role_generated",
    "role_vendored",
    "role_docs",
];

/// Canonical path-role strings for the one-hot block above, in the same order. Must align with
/// `agent::path_role_of` outputs. A role not listed here (or a missing `path_role`) contributes an
/// all-zero block, a safe default.
const ROLE_ORDER: &[&str] = &["src", "test", "config", "generated", "vendored", "docs"];

/// Canonical kind strings for the one-hot block above, in the same order. Must align with
/// `compress::shadow::kind_str` outputs. A kind not listed here contributes an all-zero block,
/// which is a safe default.
const KIND_ORDER: &[&str] = &[
    "read",
    "grep",
    "test",
    "git-status",
    "git-diff",
    "git-log",
    "mcp",
    "generic",
];

#[derive(Serialize, Deserialize, Default)]
struct ShadowFeaturesJson {
    #[serde(default)]
    risky_drops: usize,
    #[serde(default)]
    drop: FlagCountsJson,
    /// Coarse file role logged per read decision (CTX-45). Absent on historical rows and on
    /// non-read decisions, in which case the role one-hot is all-zero.
    #[serde(default)]
    path_role: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct FlagCountsJson {
    #[serde(default)]
    failure: usize,
    #[serde(default)]
    focus_path: usize,
    #[serde(default)]
    focus_symbol: usize,
    #[serde(default)]
    correction_term: usize,
    #[serde(default)]
    prompt_keyword: usize,
    #[serde(default)]
    dedup: usize,
    #[serde(default)]
    boilerplate: usize,
}

#[derive(Serialize, Deserialize)]
pub struct PerToolGate {
    pub tool: String,
    pub joined: i64,
    pub corrections: i64,
    pub rereads: i64,
    pub correction_rate: f64,
    pub reread_rate: f64,
    pub earned: bool,
}

#[derive(Serialize, Deserialize)]
pub struct RetentionModel {
    pub version: u32,
    pub trained_at: String,
    pub n_train: usize,
    pub n_holdout: usize,
    pub feature_names: Vec<String>,
    pub weights: Vec<f64>,
    pub bias: f64,
    pub holdout_auc: f64,
    pub holdout_accuracy: f64,
    pub base_correction_rate: f64,
    pub per_tool: Vec<PerToolGate>,
}

/// Risk threshold for the learned arm: treat a decision as "model would trim" only when predicted
/// correction risk is below this. Conservative on purpose. Shared by the offline benchmark and the
/// live shadow-scoring in `agent::decide` so both judge the model the same way.
pub const LEARNED_ACT_THRESHOLD: f64 = 0.15;

fn feature_row(d: &LabeledDecision) -> Vec<f64> {
    feature_vector(&d.kind, d.lines_total, d.lines_drop, &d.features_json)
}

/// Build the model input vector from a decision's raw parts. Single source of truth so live
/// shadow-scoring uses the exact same features the model was trained on. `features_json` is the
/// serialized `ShadowFeatures`; unknown fields are ignored, so callers can add metadata fields
/// (repo key, file extension, the score itself) without disturbing the vector.
fn feature_vector(kind: &str, lines_total: i64, lines_drop: i64, features_json: &str) -> Vec<f64> {
    let total = lines_total.max(1) as f64;
    let f: ShadowFeaturesJson = serde_json::from_str(features_json).unwrap_or_default();
    let mut v = vec![
        lines_drop as f64 / total,
        1.0 - (lines_drop as f64 / total),
        f.risky_drops as f64 / total,
        (lines_total as f64 + 1.0).ln(),
        f.drop.failure as f64 / total,
        f.drop.focus_path as f64 / total,
        f.drop.focus_symbol as f64 / total,
        f.drop.correction_term as f64 / total,
        f.drop.prompt_keyword as f64 / total,
        f.drop.dedup as f64 / total,
        f.drop.boilerplate as f64 / total,
    ];
    for k in KIND_ORDER {
        v.push(if *k == kind { 1.0 } else { 0.0 });
    }
    let role = f.path_role.as_deref().unwrap_or("");
    for r in ROLE_ORDER {
        v.push(if *r == role { 1.0 } else { 0.0 });
    }
    v
}

fn score_with_model(
    model: &RetentionModel,
    kind: &str,
    lines_total: i64,
    lines_drop: i64,
    features_json: &str,
) -> f64 {
    let x = feature_vector(kind, lines_total, lines_drop, features_json);
    let z = x
        .iter()
        .zip(&model.weights)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        + model.bias;
    sigmoid(z)
}

/// Predicted P(this decision precedes a correction) for a live decision described by its raw parts,
/// loading the served model from disk. Returns `None` when no trustworthy model is being served
/// (version 0 or a feature-shape mismatch), so the controller can record "no model" distinctly from
/// a real score rather than logging the misleading base rate.
pub fn score_parts(
    kind: &str,
    lines_total: i64,
    lines_drop: i64,
    features_json: &str,
) -> Option<f64> {
    let model = load_model().filter(|m| m.version > 0 && m.weights.len() == FEATURE_NAMES.len())?;
    Some(score_with_model(
        &model,
        kind,
        lines_total,
        lines_drop,
        features_json,
    ))
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Fit logistic regression by batch gradient descent. Deterministic (fixed iteration
/// order, no RNG), so the same labels always produce the same model.
fn fit_logistic(x: &[Vec<f64>], y: &[f64], n_features: usize) -> (Vec<f64>, f64) {
    let mut w = vec![0.0f64; n_features];
    let mut b = 0.0f64;
    let lr = 0.1;
    let l2 = 1e-4;
    let epochs = 400;
    let n = x.len() as f64;
    if n == 0.0 {
        return (w, b);
    }
    for _ in 0..epochs {
        let mut grad_w = vec![0.0f64; n_features];
        let mut grad_b = 0.0f64;
        for (xi, yi) in x.iter().zip(y.iter()) {
            let z = xi.iter().zip(w.iter()).map(|(a, b)| a * b).sum::<f64>() + b;
            let err = sigmoid(z) - yi;
            for j in 0..n_features {
                grad_w[j] += err * xi[j];
            }
            grad_b += err;
        }
        for j in 0..n_features {
            w[j] -= lr * (grad_w[j] / n + l2 * w[j]);
        }
        b -= lr * (grad_b / n);
    }
    (w, b)
}

/// Area under ROC, computed by rank (Mann-Whitney U). Returns 0.5 when degenerate.
fn auc(scores: &[f64], labels: &[f64]) -> f64 {
    let pos: Vec<f64> = scores
        .iter()
        .zip(labels)
        .filter(|(_, l)| **l > 0.5)
        .map(|(s, _)| *s)
        .collect();
    let neg: Vec<f64> = scores
        .iter()
        .zip(labels)
        .filter(|(_, l)| **l <= 0.5)
        .map(|(s, _)| *s)
        .collect();
    if pos.is_empty() || neg.is_empty() {
        return 0.5;
    }
    let mut wins = 0.0;
    for p in &pos {
        for n in &neg {
            if p > n {
                wins += 1.0;
            } else if (p - n).abs() < f64::EPSILON {
                wins += 0.5;
            }
        }
    }
    wins / (pos.len() as f64 * neg.len() as f64)
}

fn per_tool_gates(rows: &[LabeledDecision]) -> Vec<PerToolGate> {
    use std::collections::BTreeMap;
    let th = ActivationThresholds::default();
    let mut map: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for d in rows {
        let e = map.entry(d.tool_name.clone()).or_default();
        e.0 += 1;
        e.1 += d.correction;
        e.2 += d.reread;
    }
    map.into_iter()
        .map(|(tool, (joined, corrections, rereads))| {
            let cr = corrections as f64 / joined.max(1) as f64;
            let rr = rereads as f64 / joined.max(1) as f64;
            let prog = crate::db::CompressToolProgress {
                tool_name: tool.clone(),
                decisions: joined,
                joined,
                clean_runs: joined - corrections - rereads,
                corrections,
                rereads,
                active: false,
            };
            PerToolGate {
                tool,
                joined,
                corrections,
                rereads,
                correction_rate: cr,
                reread_rate: rr,
                earned: activation_clears_bar(&prog, &th),
            }
        })
        .collect()
}

pub fn train() -> Result<Option<RetentionModel>> {
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let rows = crate::db::load_joined_decisions(&conn);
    let per_tool = per_tool_gates(&rows);

    let positives = rows.iter().filter(|d| d.correction > 0).count();
    if rows.len() < MIN_LABELS_TO_TRAIN || positives < MIN_POSITIVE_LABELS {
        // Not enough trustworthy evidence to fit a model: too few labels overall, or too
        // few corrections to learn what one looks like. Invalidate any model a richer
        // earlier run persisted (the label set can shrink, for example after a
        // lower-confidence surface is excluded), so nothing stale is served as earned.
        // Still surface the per-tool gates so collection progress is visible.
        clear_model();
        return Ok(Some(untrained_model(&rows, per_tool)));
    }

    // Deterministic 80/20 holdout split by stable hash of the row contents.
    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut hold_x = Vec::new();
    let mut hold_y = Vec::new();
    for (i, d) in rows.iter().enumerate() {
        let x = feature_row(d);
        let y = if d.correction > 0 { 1.0 } else { 0.0 };
        if i % 5 == 0 {
            hold_x.push(x);
            hold_y.push(y);
        } else {
            train_x.push(x);
            train_y.push(y);
        }
    }

    let (weights, bias) = fit_logistic(&train_x, &train_y, FEATURE_NAMES.len());

    let scores: Vec<f64> = hold_x
        .iter()
        .map(|xi| sigmoid(xi.iter().zip(&weights).map(|(a, b)| a * b).sum::<f64>() + bias))
        .collect();
    let holdout_auc = auc(&scores, &hold_y);
    let correct = scores
        .iter()
        .zip(&hold_y)
        .filter(|(s, y)| (**s >= 0.5) == (**y > 0.5))
        .count();
    let holdout_accuracy = if hold_y.is_empty() {
        0.0
    } else {
        correct as f64 / hold_y.len() as f64
    };

    // Enforce the honesty gate: a model that does not beat a coin flip on the holdout has
    // learned nothing and must not be served as "trained". Clear any prior model so the
    // system honestly reports "not enough signal yet" rather than a stale or random one.
    if holdout_auc < MIN_HOLDOUT_AUC {
        clear_model();
        return Ok(Some(untrained_model(&rows, per_tool)));
    }

    let prev_version = load_model().map(|m| m.version).unwrap_or(0);
    let model = RetentionModel {
        version: prev_version + 1,
        trained_at: chrono::Utc::now().to_rfc3339(),
        n_train: train_x.len(),
        n_holdout: hold_x.len(),
        feature_names: FEATURE_NAMES.iter().map(|s| s.to_string()).collect(),
        weights,
        bias,
        holdout_auc,
        holdout_accuracy,
        base_correction_rate: base_rate(&rows),
        per_tool,
    };
    save_model(&model)?;
    Ok(Some(model))
}

/// The version-0 placeholder returned when we will not stand behind a trained model. It
/// still carries the base rate and per-tool gates so the user sees real collection
/// progress, but `version == 0` signals "no model is being served".
fn untrained_model(rows: &[LabeledDecision], per_tool: Vec<PerToolGate>) -> RetentionModel {
    RetentionModel {
        version: 0,
        trained_at: chrono::Utc::now().to_rfc3339(),
        n_train: 0,
        n_holdout: 0,
        feature_names: FEATURE_NAMES.iter().map(|s| s.to_string()).collect(),
        weights: vec![0.0; FEATURE_NAMES.len()],
        bias: 0.0,
        holdout_auc: 0.5,
        holdout_accuracy: 0.0,
        base_correction_rate: base_rate(rows),
        per_tool,
    }
}

fn base_rate(rows: &[LabeledDecision]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let pos = rows.iter().filter(|d| d.correction > 0).count();
    pos as f64 / rows.len() as f64
}

/// Predicted P(this decision precedes a correction) under a trained model. Used by the
/// benchmark's `ctx-learned` arm and by inference.
pub fn score_decision(model: &RetentionModel, d: &LabeledDecision) -> f64 {
    if model.version == 0 || model.weights.len() != FEATURE_NAMES.len() {
        return model.base_correction_rate;
    }
    score_with_model(
        model,
        &d.kind,
        d.lines_total,
        d.lines_drop,
        &d.features_json,
    )
}

pub fn load_model() -> Option<RetentionModel> {
    let path = crate::config::retention_model_path();
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

/// Remove the persisted model so nothing stale is served when there is no longer enough
/// trustworthy evidence to stand behind a trained model. Idempotent.
fn clear_model() {
    let _ = std::fs::remove_file(crate::config::retention_model_path());
}

fn save_model(model: &RetentionModel) -> Result<()> {
    crate::config::ensure_dir()?;
    let json = serde_json::to_value(model)?;
    crate::config::write_json_atomic(&crate::config::retention_model_path(), &json)?;
    // Append a compact history line for the Improving view.
    let line = serde_json::json!({
        "version": model.version,
        "trained_at": model.trained_at,
        "n_train": model.n_train,
        "holdout_auc": model.holdout_auc,
        "holdout_accuracy": model.holdout_accuracy,
        "base_correction_rate": model.base_correction_rate,
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::config::retention_model_history_path())
    {
        let _ = writeln!(f, "{line}");
    }
    Ok(())
}

pub fn run(json: bool) -> Result<()> {
    let Some(model) = train()? else {
        println!("No decisions collected yet. Run Claude Code turns, then `ctx ingest`.");
        return Ok(());
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&model)?);
        return Ok(());
    }

    if model.version == 0 {
        let rows = crate::db::load_joined_decisions(&crate::db::open_db()?);
        let positives = rows.iter().filter(|d| d.correction > 0).count();
        if rows.len() < MIN_LABELS_TO_TRAIN {
            println!(
                "Not enough labeled decisions yet to train ({} collected, need {}).",
                rows.len(),
                MIN_LABELS_TO_TRAIN
            );
            println!("Collection keeps running in the background. Check back after more sessions.");
        } else {
            println!(
                "Enough labels ({}), but not enough correction signal to train a model that beats a coin flip yet ({} corrections, need {}).",
                rows.len(),
                positives,
                MIN_POSITIVE_LABELS
            );
            println!("With compression off, very little is being corrected, so there is nothing to learn yet. The per-tool gate below still guides activation.");
        }
    } else {
        println!("Trained retention model v{}", model.version);
        println!(
            "  labels:        {} train / {} holdout",
            model.n_train, model.n_holdout
        );
        println!("  holdout AUC:   {:.3}", model.holdout_auc);
        println!("  holdout acc:   {:.1}%", model.holdout_accuracy * 100.0);
        println!(
            "  base rate:     {:.1}% of decisions precede a correction",
            model.base_correction_rate * 100.0
        );
    }
    println!();
    println!("Per-tool evidence gate (your own labels)");
    if model.per_tool.is_empty() {
        println!("  none yet");
    }
    for t in &model.per_tool {
        println!(
            "  {:<28} {:>5} judged  corrections {:>5.1}%  re-reads {:>5.1}%  [{}]",
            t.tool,
            t.joined,
            t.correction_rate * 100.0,
            t.reread_rate * 100.0,
            if t.earned {
                "ready to activate"
            } else {
                "watching"
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::LabeledDecision;

    fn model_with(weights: Vec<f64>, bias: f64) -> RetentionModel {
        RetentionModel {
            version: 1,
            trained_at: "2026-06-11T00:00:00Z".into(),
            n_train: 100,
            n_holdout: 20,
            feature_names: FEATURE_NAMES.iter().map(|s| s.to_string()).collect(),
            weights,
            bias,
            holdout_auc: 0.7,
            holdout_accuracy: 0.7,
            base_correction_rate: 0.1,
            per_tool: vec![],
        }
    }

    #[test]
    fn feature_vector_has_expected_shape_and_ratios() {
        let v = feature_vector("generic", 10, 4, "{}");
        assert_eq!(v.len(), FEATURE_NAMES.len());
        assert!((v[0] - 0.4).abs() < 1e-9, "drop_ratio");
        assert!((v[1] - 0.6).abs() < 1e-9, "kept_ratio");
        assert!((v[3] - 11f64.ln()).abs() < 1e-9, "log_lines");
    }

    #[test]
    fn kind_one_hot_sets_exactly_one_flag() {
        // The kind block sits just before the trailing path-role block.
        let kind_lo = FEATURE_NAMES.len() - KIND_ORDER.len() - ROLE_ORDER.len();
        let kind_hi = FEATURE_NAMES.len() - ROLE_ORDER.len();
        let v = feature_vector("read", 10, 4, "{}");
        let kind_block = &v[kind_lo..kind_hi];
        assert_eq!(kind_block.iter().filter(|x| **x == 1.0).count(), 1);
        let read_idx = KIND_ORDER.iter().position(|k| *k == "read").unwrap();
        assert_eq!(kind_block[read_idx], 1.0);
        // An unknown kind sets no flag, which is a safe all-zero default.
        let unknown = feature_vector("totally-unknown", 10, 4, "{}");
        let ub = &unknown[kind_lo..kind_hi];
        assert!(ub.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn path_role_one_hot_sets_exactly_one_flag() {
        let role_lo = FEATURE_NAMES.len() - ROLE_ORDER.len();
        // A read tagged with a role lights exactly that role.
        let v = feature_vector("read", 10, 4, r#"{"path_role":"vendored"}"#);
        let role_block = &v[role_lo..];
        assert_eq!(role_block.iter().filter(|x| **x == 1.0).count(), 1);
        let idx = ROLE_ORDER.iter().position(|r| *r == "vendored").unwrap();
        assert_eq!(role_block[idx], 1.0);
        // No path_role (historical row or non-read) leaves the whole block zero.
        let none = feature_vector("read", 10, 4, "{}");
        assert!(none[role_lo..].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn zero_weight_model_scores_one_half() {
        let m = model_with(vec![0.0; FEATURE_NAMES.len()], 0.0);
        assert!((score_with_model(&m, "read", 10, 4, "{}") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn score_matches_manual_sigmoid() {
        let mut w = vec![0.0; FEATURE_NAMES.len()];
        w[0] = 1.0; // weight only on drop_ratio (0.4 for 4/10)
        let m = model_with(w, 0.0);
        let expect = 1.0 / (1.0 + (-0.4f64).exp());
        assert!((score_with_model(&m, "read", 10, 4, "{}") - expect).abs() < 1e-9);
    }

    #[test]
    fn live_scoring_path_matches_training_path() {
        // The controller's shadow score must equal what score_decision computes for the same
        // decision, or live logs would not match what the model was trained/benchmarked on.
        let mut w = vec![0.0; FEATURE_NAMES.len()];
        w[2] = 2.0; // risky_drop_ratio
        let m = model_with(w, -0.5);
        let d = LabeledDecision {
            tool_name: "Read".into(),
            kind: "read".into(),
            lines_total: 20,
            lines_drop: 5,
            chars_in: 100,
            would_chars_out: 40,
            features_json: r#"{"risky_drops":2,"drop":{"failure":1}}"#.into(),
            correction: 0,
            reread: 0,
        };
        let via_decision = score_decision(&m, &d);
        let via_shared =
            score_with_model(&m, &d.kind, d.lines_total, d.lines_drop, &d.features_json);
        assert!((via_decision - via_shared).abs() < 1e-12);
    }

    #[test]
    fn unknown_metadata_fields_do_not_disturb_the_vector() {
        // repo_key / file_ext / model_score get added to features_json; they must be ignored.
        let plain = feature_vector("read", 12, 3, r#"{"risky_drops":1,"drop":{"failure":1}}"#);
        let with_meta = feature_vector(
            "read",
            12,
            3,
            r#"{"risky_drops":1,"drop":{"failure":1},"repo_key":"/x","file_ext":"rs","model_score":0.2}"#,
        );
        assert_eq!(plain, with_meta);
    }
}
