//! Session-grounded line retention: score and greedily select lines under budget.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::context::{SgrMode, TaskFrame};
use super::types::CompressOptions;

const FAILURE_MARKERS: &[&str] = &[
    "error", "Error", "ERROR", "failed", "FAILED", "failure", "Failure", "panic", "Panic",
    "assertion", "Assertion", "exception", "Exception", "not found", "No such file",
];

const BOILERPLATE_MARKERS: &[&str] = &[
    "Compiling ", "Downloading ", "Finished ", "Building ", "Installing ", "Updating ",
    "   Compiling", "    Finished", "cargo: ",
];

pub struct RetentionOutput {
    pub text: String,
    pub chars_out: usize,
}

/// Per-line retention feature flags. These are the signals the learned controller
/// (Act 1) trains on, recorded per shadow decision so every line is a labeled sample.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LineFlags {
    pub failure: bool,
    pub focus_path: bool,
    pub focus_symbol: bool,
    pub correction_term: bool,
    pub prompt_keyword: bool,
    pub dedup: bool,
    pub boilerplate: bool,
    pub empty: bool,
}

pub struct ScoredLine {
    pub idx: usize,
    pub line: String,
    pub score: i32,
    pub flags: LineFlags,
}

/// A full retention plan: which lines would be kept vs dropped, with per-line features.
/// Both the live SGR path ([`apply_line_retention`]) and shadow logging consume this so
/// the recorded decision matches what compression would actually do.
pub struct RetentionPlan {
    pub lines_total: usize,
    pub kept_idx: Vec<usize>,
    pub scored: Vec<ScoredLine>,
    pub text: String,
    pub chars_out: usize,
}

pub fn plan_retention(input: &str, frame: &TaskFrame, opts: &CompressOptions) -> RetentionPlan {
    let scored: Vec<ScoredLine> = input
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            let (score, flags) = score_line(line, frame, opts, idx);
            ScoredLine {
                idx,
                line: line.to_string(),
                score,
                flags,
            }
        })
        .collect();
    let lines_total = scored.len();
    let chars_in = input.chars().count();

    if chars_in <= opts.target_chars {
        return RetentionPlan {
            lines_total,
            kept_idx: (0..lines_total).collect(),
            scored,
            text: input.to_string(),
            chars_out: chars_in,
        };
    }

    let mut pinned: Vec<usize> = Vec::new();
    let mut candidates: Vec<&ScoredLine> = Vec::new();
    for s in &scored {
        if opts.preserve_errors && s.flags.failure {
            pinned.push(s.idx);
        } else {
            candidates.push(s);
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.idx.cmp(&b.idx)));

    let mut kept: Vec<usize> = pinned.clone();
    let line_chars = |idx: usize| scored[idx].line.chars().count();
    let mut used: usize = pinned.iter().map(|i| line_chars(*i)).sum();
    for c in candidates {
        let add = c.line.chars().count() + if kept.is_empty() { 0 } else { 1 };
        if used + add > opts.target_chars {
            continue;
        }
        kept.push(c.idx);
        used += add;
    }

    if kept.is_empty() {
        let fallback: String = input.chars().take(opts.target_chars.saturating_sub(1)).collect();
        let text = format!("{fallback}…");
        return RetentionPlan {
            lines_total,
            kept_idx: Vec::new(),
            scored,
            chars_out: text.chars().count(),
            text,
        };
    }

    kept.sort_unstable();
    let mut out_lines: Vec<String> = kept.iter().map(|i| scored[*i].line.clone()).collect();
    let omitted = lines_total.saturating_sub(out_lines.len());
    if omitted > 0 {
        out_lines.push(format!(
            "… {omitted} lines omitted by session-grounded retention (mode: {})",
            frame.mode.as_str()
        ));
    }
    let text = out_lines.join("\n");
    RetentionPlan {
        lines_total,
        kept_idx: kept,
        scored,
        chars_out: text.chars().count(),
        text,
    }
}

pub fn apply_line_retention(input: &str, frame: &TaskFrame, opts: &CompressOptions) -> RetentionOutput {
    let plan = plan_retention(input, frame, opts);
    RetentionOutput {
        text: plan.text,
        chars_out: plan.chars_out,
    }
}

fn score_line(
    line: &str,
    frame: &TaskFrame,
    opts: &CompressOptions,
    line_index: usize,
) -> (i32, LineFlags) {
    let mut flags = LineFlags::default();
    let trimmed = line.trim();
    if trimmed.is_empty() {
        flags.empty = true;
        return (-10, flags);
    }

    let mut score = 0i32;
    if line_has_failure_marker(line) {
        flags.failure = true;
        score += 100;
    }
    for path in &frame.focus_paths {
        if path_matches_line(line, path) {
            flags.focus_path = true;
            score += 40;
            break;
        }
    }
    for sym in &frame.focus_symbols {
        if symbol_matches_line(line, sym) {
            flags.focus_symbol = true;
            score += 35;
            break;
        }
    }
    for snippet in &frame.correction_snippets {
        for term in keywords_from_text(snippet) {
            if line.to_lowercase().contains(&term) {
                flags.correction_term = true;
                score += 30;
                break;
            }
        }
    }
    for kw in &frame.prompt_keywords {
        if !kw.is_empty() && line.to_lowercase().contains(kw) {
            flags.prompt_keyword = true;
            score += 20;
        }
    }
    if frame.prior_line_hashes.contains(&line_hash(trimmed)) {
        flags.dedup = true;
        score -= 30;
    }
    if BOILERPLATE_MARKERS.iter().any(|m| line.contains(m)) {
        flags.boilerplate = true;
        score -= 20;
    }
    if frame.mode == SgrMode::Debug && flags.failure {
        score += 50;
    }
    let _ = line_index;
    let _ = opts;
    (score, flags)
}

fn line_has_failure_marker(line: &str) -> bool {
    FAILURE_MARKERS.iter().any(|m| line.contains(m))
}

fn path_matches_line(line: &str, path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if line.contains(path) {
        return true;
    }
    if let Some(base) = path.rsplit('/').next().filter(|s| !s.is_empty()) {
        if line.contains(base) {
            return true;
        }
    }
    false
}

fn symbol_matches_line(line: &str, sym: &str) -> bool {
    if sym.len() < 2 {
        return false;
    }
    let lower = line.to_lowercase();
    let s = sym.to_lowercase();
    lower.contains(&s)
}

fn keywords_from_text(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .take(8)
        .collect()
}

pub fn line_hash(line: &str) -> u64 {
    let mut h = DefaultHasher::new();
    line.trim().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compress::context::{build_task_frame_minimal, SgrMode};

    #[test]
    fn keeps_focus_path_lines() {
        let mut frame = build_task_frame_minimal("fix foo.rs compile error", "/tmp");
        frame.focus_paths = vec!["foo.rs".into()];
        frame.mode = SgrMode::Debug;
        let input = (0..80)
            .map(|i| format!("noise line {i} unrelated module bar/baz.rs"))
            .chain([
                "error in foo.rs: expected identifier".to_string(),
                "another unrelated line in lib/other.rs".to_string(),
            ])
            .collect::<Vec<_>>()
            .join("\n");
        let opts = CompressOptions {
            target_chars: 120,
            ..Default::default()
        };
        let out = apply_line_retention(&input, &frame, &opts);
        assert!(out.text.contains("foo.rs"));
    }

    #[test]
    fn correction_terms_boost_matching_lines() {
        let mut frame = build_task_frame_minimal("continue", "/tmp");
        frame.correction_snippets = vec!["no that's the wrong module".into()];
        frame.focus_symbols = vec!["wrong_module".into()];
        let input = "alpha beta gamma\nwrong_module::bad() failed\nzeta eta\n";
        let opts = CompressOptions {
            target_chars: 40,
            ..Default::default()
        };
        let out = apply_line_retention(input, &frame, &opts);
        assert!(out.text.contains("wrong_module"));
    }
}
