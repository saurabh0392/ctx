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

/// Content-free evidence emitted by the typed MCP adapter while it remains shadow-only.
#[derive(Debug, Clone, Serialize)]
pub struct McpContractShadow {
    pub round_trip_identical: bool,
    pub content_blocks: usize,
    pub text_blocks: usize,
    pub unknown_blocks: usize,
    pub has_structured_content: bool,
    pub has_metadata: bool,
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_chars_in: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_chars_out: Option<usize>,
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
    /// Lossless MCP parse/render and typed-compressor evidence. This is observation only: T1 does
    /// not use it to authorize an apply or rebuild a native result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_contract: Option<McpContractShadow>,
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
    /// For edit tools, a distinctive signature line of what this edit *wrote* (`new_string`) and what
    /// it *sought* (`old_string`), taken from the tool input (CTX-62 content-overlap fix). A re-edit
    /// only counts as harm when a later edit sought the exact text this one wrote, i.e. the agent
    /// went back and changed the very lines it just wrote. This replaces the old file-level signal,
    /// which fired on any second edit anywhere in a big file and read as ~70% harm on normal
    /// multi-part editing. `None` when the strings carry nothing distinctive, in which case the
    /// re-edit does not fire for that edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_wrote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_sought: Option<String>,
    /// For a targeted read, the `[start, end]` line range the `offset`/`limit` covered (CTX-62). A
    /// re-read only counts as harm when a later read of the same file overlaps this range: paging
    /// through a large file with several non-overlapping targeted views is normal, not the agent
    /// re-reading because a trim cut what it needed. `None` for a whole-file read (no offset/limit),
    /// which covers everything and so overlaps any later read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_lines: Option<[u32; 2]>,
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
        CompressKind::Edit => "edit",
    }
}

/// A distinctive signature line from an edit's `old_string` or `new_string`: the longest meaningful
/// line, whitespace-normalized and capped. It lets the re-edit harm signal tell a same-region redo
/// (a later edit that sought the exact text this edit wrote) apart from a normal edit elsewhere in
/// the same file (CTX-62). Taken from the tool input, so it does not depend on the echo format the
/// way the old `cat -n` parse did. `None` when nothing distinctive enough exists, in which case the
/// re-edit does not fire for that edit.
pub(crate) fn edit_content_anchor(s: &str) -> Option<String> {
    let best = s
        .lines()
        .map(str::trim)
        .max_by_key(|l| l.len())
        .unwrap_or("");
    let norm = best.split_whitespace().collect::<Vec<_>>().join(" ");
    if norm.chars().count() < 8 {
        return None;
    }
    Some(norm.chars().take(160).collect())
}

/// The `[start, end]` line range a Read's `offset`/`limit` covered (CTX-62). `None` for a whole-file
/// read (neither present), which the re-read join treats as covering everything. An `offset` with no
/// `limit` reads to end of file, so its end is left open (a large sentinel).
pub(crate) fn read_line_range(tool_input: &Value) -> Option<[u32; 2]> {
    let offset = tool_input.get("offset").and_then(|v| v.as_u64());
    let limit = tool_input.get("limit").and_then(|v| v.as_u64());
    if offset.is_none() && limit.is_none() {
        return None;
    }
    let start = offset.unwrap_or(1).max(1) as u32;
    let end = match limit {
        Some(l) if l > 0 => start.saturating_add((l - 1) as u32),
        _ => u32::MAX, // offset to end of file
    };
    Some([start, end])
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
    compute_shadow_decision_with_mcp(
        tool_name, tool_input, raw_output, None, cfg, session_id, cwd,
    )
}

/// Shadow computation with an optional lossless MCP result. The existing retention decision stays
/// byte-for-byte on the old path; the typed candidate is recorded only as content-free evidence.
pub fn compute_shadow_decision_with_mcp(
    tool_name: &str,
    tool_input: &Value,
    raw_output: &str,
    canonical_mcp: Option<&crate::tool_result::CanonicalMcpResult>,
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

    let mcp_contract = if cfg.compress_shadow_enabled && matches!(kind, CompressKind::Mcp) {
        canonical_mcp.map(|result| {
            let coverage = result.coverage();
            let context = super::types::CompressContext {
                cwd: cwd.to_string(),
                prompt_keywords: frame.prompt_keywords.clone(),
            };
            let candidate = super::mcp::compress_mcp_result_shadow(result, &opts, &context);
            McpContractShadow {
                // Keep the live evidence honest: this is computed from the parsed value rather
                // than inferred from the parser contract. The work happens only while shadow
                // collection is enabled.
                round_trip_identical: result.render() == *result.raw(),
                content_blocks: coverage.content_blocks,
                text_blocks: coverage.text_blocks,
                unknown_blocks: coverage.unknown_blocks,
                has_structured_content: coverage.has_structured_content,
                has_metadata: coverage.has_metadata,
                is_error: result.is_error(),
                candidate_strategy: candidate.as_ref().map(|r| r.strategy.clone()),
                candidate_chars_in: candidate.as_ref().map(|r| r.chars_in),
                candidate_chars_out: candidate.as_ref().map(|r| r.chars_out),
            }
        })
    } else {
        None
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
            mcp_contract,
            intent: None,
            read_protected: None,
            repo_key: None,
            file_ext: None,
            path_role: None,
            model_score: None,
            would_model_apply: None,
            model_proposed: None,
            self_dev: None,
            edit_wrote: if matches!(kind, CompressKind::Edit) {
                edit_content_anchor(
                    tool_input
                        .get("new_string")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            } else {
                None
            },
            edit_sought: if matches!(kind, CompressKind::Edit) {
                edit_content_anchor(
                    tool_input
                        .get("old_string")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            } else {
                None
            },
            read_lines: if matches!(kind, CompressKind::Read) {
                read_line_range(tool_input)
            } else {
                None
            },
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
    fn read_line_range_from_offset_and_limit() {
        assert_eq!(
            read_line_range(&serde_json::json!({"offset": 500, "limit": 50})),
            Some([500, 549])
        );
        // Whole-file read (no offset/limit) -> None, so it overlaps any later read.
        assert_eq!(
            read_line_range(&serde_json::json!({"file_path": "/a.rs"})),
            None
        );
        // offset with no limit reads to end of file.
        assert_eq!(
            read_line_range(&serde_json::json!({"offset": 10})),
            Some([10, u32::MAX])
        );
    }

    #[test]
    fn edit_content_anchor_takes_the_longest_meaningful_line() {
        assert_eq!(
            edit_content_anchor("if x {\n    let value = compute(a, b, c)\n}"),
            Some("let value = compute(a, b, c)".into())
        );
        // Nothing distinctive enough -> None, so a re-edit of it never fires.
        assert_eq!(edit_content_anchor("x"), None);
        assert_eq!(edit_content_anchor("  \n\t"), None);
    }

    #[test]
    fn edit_decision_records_what_it_wrote_and_sought_from_the_input() {
        let d = compute_shadow_decision(
            "Edit",
            &serde_json::json!({
                "file_path": "/a/b.rs",
                "old_string": "let total = old_value + 1",
                "new_string": "let total = new_value + 2",
            }),
            // The echo format no longer matters; anchors come from the input.
            "The file /a/b.rs has been updated successfully.",
            &cfg(),
            None,
            "/tmp",
        )
        .expect("decision");
        assert_eq!(
            d.features.edit_wrote.as_deref(),
            Some("let total = new_value + 2")
        );
        assert_eq!(
            d.features.edit_sought.as_deref(),
            Some("let total = old_value + 1")
        );
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
        assert!(
            js.contains("\"self_dev\":true"),
            "serialized features: {js}"
        );
        assert!(crate::db::EXCLUDE_SELF_DEV.contains("\"self_dev\":true"));
    }
}
