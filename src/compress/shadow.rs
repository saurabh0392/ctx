//! Shadow-mode decision computation (Act 0 self-labeling).
//!
//! Computes the retention decision ctx *would* make for a tool result, plus the
//! per-line feature aggregates, without ever modifying the output the model sees.
//! Each decision is written to `compress_decisions` and joined to its outcome
//! (correction / re-read) by a later ingest pass, producing a clean, fully-labeled
//! training corpus with zero UX risk.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::config::Config;

use super::classify::classify_tool;
use super::context::{adaptive_target_chars, build_task_frame};
use super::retain::{plan_retention, LineFlags};
use super::types::{CompressKind, CompressOptions};

#[derive(Debug, Clone, Serialize, Default)]
pub struct FlagCounts {
    pub failure: usize,
    pub focus_path: usize,
    pub focus_symbol: usize,
    pub correction_term: usize,
    pub prompt_keyword: usize,
    pub dedup: usize,
    pub boilerplate: usize,
    pub empty: usize,
}

impl FlagCounts {
    fn add(&mut self, f: &LineFlags) {
        self.failure += f.failure as usize;
        self.focus_path += f.focus_path as usize;
        self.focus_symbol += f.focus_symbol as usize;
        self.correction_term += f.correction_term as usize;
        self.prompt_keyword += f.prompt_keyword as usize;
        self.dedup += f.dedup as usize;
        self.boilerplate += f.boilerplate as usize;
        self.empty += f.empty as usize;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowFeatures {
    pub target_chars: usize,
    pub kept_ratio: f32,
    /// Feature counts over all lines, over kept lines, and over dropped lines.
    pub totals: FlagCounts,
    pub keep: FlagCounts,
    pub drop: FlagCounts,
    /// Dropped lines that carried a failure marker. Should stay ~0 (errors are pinned);
    /// a non-zero value is an early warning the heuristic is throwing away signal.
    pub risky_drops: usize,
    /// Intent signal from the agent's narration (ADR 0004 / CTX-11). `Some(..)` when the signal ran
    /// for a Read decision; `None` when it does not apply (non-Read kind or signal disabled). The
    /// three components are recorded separately so coverage (`has_text`) is measurable apart from
    /// the intent rate. Set by the controller, which is where the transcript is read;
    /// compute_shadow_decision leaves it None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<crate::compress::intent::IntentSignal>,
    /// `Some(true)` when the Read edit-intent guard (CTX-8) deliberately kept a read in full
    /// because the file looks editable, so the trim never happened on purpose. `None` when the
    /// guard did not apply (non-Read kind, guard off, or read was trim-eligible). This lets the
    /// activity feed tell a *protected* read apart from one that is merely still being watched.
    /// Set by the controller in `agent::decide`; compute_shadow_decision leaves it None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_protected: Option<bool>,
    // --- Per-decision model groundwork (ADR 0007 / CTX-16). All set by the controller in
    // `agent::decide`; compute_shadow_decision leaves them None. These are logged only and never
    // change what gets trimmed in this phase; they are the data the per-decision model needs.
    /// Stable key for the repo this decision happened in (git root path, else cwd), so the model can
    /// later be trained and reported per repo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_key: Option<String>,
    /// Lowercased file extension for read decisions (`rs`, `md`, ...), a cheap personal/contextual
    /// signal absent from today's trim-shape-only feature set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_ext: Option<String>,
    /// Coarse file role for read decisions (`src`, `test`, `config`, `generated`, `vendored`,
    /// `docs`), derived by `agent::path_role_of` (CTX-45 / ADR 0030). Absent on non-read decisions
    /// and on reads with no file path. The retention model's path-role one-hot reads this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_role: Option<String>,
    /// What the served retention model *would* have predicted for this decision (P(correction)).
    /// `None` when no trustworthy model is being served. Logged for forward measurement only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_score: Option<f64>,
    /// Whether the model's score clears the conservative act threshold, i.e. the model *would* have
    /// chosen to trim. `None` when there is no model. The real `apply` decision ignores this entirely
    /// in this phase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub would_model_apply: Option<bool>,
    /// `Some(true)` when the file-aware model proposed trimming this working read that the static
    /// guard would otherwise have kept (ADR 0032 / CTX-46 increment 3). Only set when
    /// `compress_model_propose` is on and the proposal actually lifted the guard. The proposal still
    /// has to clear the preset, burn-in, and causal gate to be applied, so this records intent, not
    /// a guaranteed trim. Set by the controller in `agent::decide`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_proposed: Option<bool>,
    /// `Some(true)` when this decision happened inside ctx's own source repo (a Cargo.toml with
    /// package name "ctx" at the repo root). Building ctx, re-editing its files, and running its
    /// commands is the developer's own churn, not user behavior, so it must not feed the learning
    /// corpus, the causal gate, or the precision audit (CTX-32). The row is still recorded so the
    /// Activity feed stays complete; the corpus queries exclude it. Set by the controller in
    /// `agent::decide`; compute_shadow_decision leaves it None. Serializes to the exact token
    /// `"self_dev":true`, which the corpus-exclusion SQL matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_dev: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ShadowDecision {
    pub kind: CompressKind,
    pub task_mode: String,
    pub server_prefix: Option<String>,
    pub lines_total: usize,
    pub lines_keep: usize,
    pub lines_drop: usize,
    pub chars_in: usize,
    pub would_chars_out: usize,
    pub features: ShadowFeatures,
}

impl ShadowDecision {
    pub fn kind_str(&self) -> &'static str {
        kind_str(self.kind)
    }
    pub fn features_json(&self) -> String {
        serde_json::to_string(&self.features).unwrap_or_else(|_| "{}".into())
    }
}

pub fn kind_str(k: CompressKind) -> &'static str {
    match k {
        CompressKind::Passthrough => "passthrough",
        CompressKind::Generic => "generic",
        CompressKind::GitStatus => "git-status",
        CompressKind::GitDiff => "git-diff",
        CompressKind::GitLog => "git-log",
        CompressKind::TestRunner => "test",
        CompressKind::Grep => "grep",
        CompressKind::Read => "read",
        CompressKind::Mcp => "mcp",
    }
}

/// Server prefix for an MCP tool name (`mcp__linear__list` -> `mcp__linear`), else None.
pub fn server_prefix_of(tool_name: &str) -> Option<String> {
    if !tool_name.starts_with("mcp__") {
        return None;
    }
    let mut parts = tool_name.splitn(3, "__");
    let _ = parts.next();
    let server = parts.next()?;
    Some(format!("mcp__{server}"))
}

/// Compute the would-do retention decision for a tool result. Returns None for empty
/// input. This never mutates the output; it only describes the decision for logging.
pub fn compute_shadow_decision(
    tool_name: &str,
    tool_input: &Value,
    raw_output: &str,
    cfg: &Config,
    session_id: Option<&str>,
    cwd: &str,
) -> Option<ShadowDecision> {
    if raw_output.is_empty() {
        return None;
    }

    let command = tool_input.get("command").and_then(|v| v.as_str());
    let file_path = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str());

    let kind = classify_tool(tool_name, command, file_path);
    let profile = cfg.active_profile.as_deref().unwrap_or("all");
    let frame = build_task_frame(
        session_id,
        cwd,
        tool_name,
        tool_input,
        profile,
        cfg.compress_sgr_dedup,
    );
    let target = adaptive_target_chars(
        cfg.compress_target_chars,
        &frame,
        cfg.compress_adaptive_budget,
    );
    let opts = CompressOptions {
        target_chars: target,
        max_input_chars: cfg.compress_max_output_chars,
        redact_secrets: cfg.compress_redact_secrets,
        preserve_errors: cfg.compress_preserve_errors,
    };

    let plan = plan_retention(raw_output, &frame, &opts);
    let kept: HashSet<usize> = plan.kept_idx.iter().copied().collect();

    let mut totals = FlagCounts::default();
    let mut keep = FlagCounts::default();
    let mut drop = FlagCounts::default();
    let mut risky_drops = 0usize;
    for s in &plan.scored {
        totals.add(&s.flags);
        if kept.contains(&s.idx) {
            keep.add(&s.flags);
        } else {
            drop.add(&s.flags);
            if s.flags.failure {
                risky_drops += 1;
            }
        }
    }

    let lines_total = plan.lines_total;
    let lines_keep = plan.kept_idx.len();
    let lines_drop = lines_total.saturating_sub(lines_keep);
    let kept_ratio = if lines_total == 0 {
        1.0
    } else {
        lines_keep as f32 / lines_total as f32
    };

    Some(ShadowDecision {
        kind,
        task_mode: frame.mode.as_str().to_string(),
        server_prefix: server_prefix_of(tool_name),
        lines_total,
        lines_keep,
        lines_drop,
        chars_in: raw_output.chars().count(),
        would_chars_out: plan.chars_out,
        features: ShadowFeatures {
            target_chars: target,
            kept_ratio,
            totals,
            keep,
            drop,
            risky_drops,
            intent: None,
            read_protected: None,
            repo_key: None,
            file_ext: None,
            path_role: None,
            model_score: None,
            would_model_apply: None,
            model_proposed: None,
            self_dev: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            compress_target_chars: 200,
            compress_max_output_chars: 12_000,
            compress_preserve_errors: true,
            compress_redact_secrets: true,
            ..Default::default()
        }
    }

    #[test]
    fn records_drop_decision_for_large_output() {
        let mut raw = String::new();
        for i in 0..200 {
            raw.push_str(&format!("noise line {i} in src/other_{i}.rs\n"));
        }
        let d = compute_shadow_decision(
            "Read",
            &serde_json::json!({"file_path": "src/foo.rs"}),
            &raw,
            &cfg(),
            None,
            "/tmp/project",
        )
        .expect("decision");
        assert!(d.lines_total > 100);
        assert!(d.lines_drop > 0);
        assert!(d.would_chars_out <= d.chars_in);
    }

    #[test]
    fn small_output_keeps_everything() {
        let d = compute_shadow_decision(
            "Bash",
            &serde_json::json!({"command": "echo hi"}),
            "hi\nthere\n",
            &cfg(),
            None,
            "/tmp",
        )
        .expect("decision");
        assert_eq!(d.lines_drop, 0);
    }

    #[test]
    fn mcp_server_prefix_extracted() {
        assert_eq!(
            server_prefix_of("mcp__linear__list_issues").as_deref(),
            Some("mcp__linear")
        );
        assert_eq!(server_prefix_of("Bash"), None);
    }

    // The corpus-exclusion SQL (db::EXCLUDE_SELF_DEV) keys off the exact compact serialization of
    // self_dev = Some(true). If serde ever renamed the field or changed spacing, the filter would
    // silently stop excluding ctx's own dev activity, re-polluting the gate. This pins both ends.
    #[test]
    fn self_dev_serializes_to_the_token_the_corpus_filter_matches() {
        let mut d = compute_shadow_decision(
            "Read",
            &serde_json::json!({"file_path": "src/foo.rs"}),
            "one\ntwo\nthree\n",
            &cfg(),
            None,
            "/tmp/project",
        )
        .expect("decision");
        assert!(
            !d.features_json().contains("self_dev"),
            "absent by default so the JSON stays small and the filter never matches user rows"
        );
        d.features.self_dev = Some(true);
        let js = d.features_json();
        assert!(js.contains("\"self_dev\":true"), "serialized features: {js}");
        assert!(crate::db::EXCLUDE_SELF_DEV.contains("\"self_dev\":true"));
    }
}
