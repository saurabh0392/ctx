//! Shared outcome-signal heuristics for every surface (Claude Code parser and the
//! transcript adapters). The point is precision: a "correction" label must mean the user
//! pushed back on the agent's work, not that they typed a short go-ahead like "lets do 1".
//!
//! This lexicon used to live only in the Cursor adapter, so the Claude Code path scored
//! corrections with a blunt "short turn after substantial work" rule and mislabeled menu
//! picks and approvals as harm (see SAU-148 label audit). Centralizing it here lets both
//! surfaces label the same turn the same way, and gives the audit and training one
//! definition to trust.

/// Confidence tier for a correction signal. The tier travels with the label so the
/// fail-safe activation gate can keep using every correction while the learned model and
/// the causal before/after can prefer the high-confidence ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionClass {
    /// Not a correction.
    None,
    /// Low confidence: a short follow-up after real work, with no explicit complaint.
    /// Could be a redirect or could be benign. Kept for the fail-safe gate, down-weighted
    /// for training.
    Terse,
    /// High confidence: explicit complaint language ("wrong", "revert", "undo", "broken").
    Explicit,
}

/// A user turn at or below this many characters, following substantial assistant work and
/// with no approval/continuation wording, is treated as a likely terse course-correction.
/// Surfaces with a personal length baseline (Claude Code) pass that instead.
pub const DEFAULT_TERSE_MAX_CHARS: usize = 160;

/// Explicit complaint cues. If any appears the turn is a high-confidence correction,
/// regardless of length or approval words. These protect the safe direction: a real
/// complaint is never suppressed. Matched as lowercase substrings.
const NEGATIVE_CUES: &[&str] = &[
    "wrong",
    "revert",
    "undo",
    "broken",
    "doesn't work",
    "does not work",
    "not working",
    "didn't work",
    "did not work",
    "go back",
    "that's not",
    "thats not",
    "not what",
    "incorrect",
    "don't do",
    "dont do",
    "no, ",
    "no,",
    "nope",
    "remove that",
    "still broken",
    "not right",
];

/// Multi-word approvals and continuations that read as "keep going" or "that's good",
/// not as a correction. Matched as lowercase substrings.
const APPROVAL_PHRASES: &[&str] = &[
    "no problem",
    "no worries",
    "looks good",
    "sounds good",
    "go ahead",
    "ship it",
    "do it",
    "move to",
    "start with",
    "keep going",
    "carry on",
    "let's go",
    "lets go",
    "go for it",
    "that works",
    "works now",
    "makes sense",
    "yes please",
    "please proceed",
    "hell yeah",
    "thank you",
];

/// Single approval/continuation/filler tokens. A message whose alphabetic tokens are all
/// drawn from this set (numbers and punctuation ignored) is a pure approval or
/// continuation, so it is not a correction.
const APPROVAL_TOKENS: &[&str] = &[
    "yes",
    "yep",
    "yup",
    "yeah",
    "ya",
    "ok",
    "okay",
    "k",
    "kk",
    "sure",
    "thanks",
    "thank",
    "you",
    "ty",
    "lgtm",
    "perfect",
    "great",
    "nice",
    "cool",
    "awesome",
    "good",
    "fine",
    "done",
    "proceed",
    "go",
    "next",
    "continue",
    "please",
    "sounds",
    "looks",
    "works",
    "that",
    "this",
    "it",
    "the",
    "a",
    "an",
    "and",
    "to",
    "now",
    "then",
    "do",
    "ship",
    "keep",
    "going",
    "move",
    "start",
    "with",
    "on",
    "let",
    "lets",
    "phase",
    "man",
    "love",
    "makes",
    "sense",
    "hell",
    "right",
    "correct",
    "exactly",
    "agreed",
    "agree",
    "carry",
    "we",
    "can",
    "plus",
    "cheers",
    "beautiful",
    "clean",
    "ready",
    "yer",
    "step",
];

pub fn has_negative_cue(raw_lower: &str) -> bool {
    NEGATIVE_CUES.iter().any(|c| raw_lower.contains(c))
}

/// Window, in transcript turns, within which a structural follow-up (re-edit, retry) still
/// counts as caused by the call it follows. Matches the correction window the join uses, so
/// every structural signal reads the timeline the same way `reread` does.
pub const STRUCTURAL_WINDOW_TURNS: u32 = 3;

/// Structural outcome signals derived from the tool-call timeline rather than user language
/// (ADR 0019 / CTX-32). These are pure and unit-testable: the join supplies the ordinals. They
/// are observation-only by design and must not feed the activation gate until a per-signal
/// precision spot-check promotes them; recording them is safe, voting with them is not.
///
/// A file the agent just read or wrote, then edited again within `window` turns: the first
/// result was not enough to act on, a churn signal. `touch_ordinal` is the read/write; the
/// `edit_ordinals` are later edits of the *same* path (the caller resolves "same path").
pub fn is_immediate_reedit(touch_ordinal: u32, edit_ordinals: &[u32], window: u32) -> bool {
    edit_ordinals
        .iter()
        .any(|&o| o > touch_ordinal && o <= touch_ordinal.saturating_add(window))
}

/// The tool names ctx treats as an edit/write of a file, across every surface, lowercased. The
/// single source of truth for both [`is_edit_tool`] (used by the transcript join) and
/// [`edit_tool_sql_in_list`] (used by the timestamp join's SQL), so the same-file edit-follow
/// label means the same thing on both paths.
pub const EDIT_TOOL_NAMES: &[&str] = &[
    "write",
    "edit",
    "multiedit",
    "str_replace",
    "str_replace_editor",
    "create_file",
    "applypatch",
    "apply_patch",
    "searchreplace",
    "search_replace",
];

/// Whether a tool name is an edit/write of a file (as opposed to a read or a shell/MCP call).
/// Used to tell an immediate re-edit apart from a benign re-read of the same path. Matched
/// case-insensitively against the tool names ctx sees across surfaces.
pub fn is_edit_tool(tool_name: &str) -> bool {
    let n = tool_name.trim().to_ascii_lowercase();
    EDIT_TOOL_NAMES.contains(&n.as_str())
}

/// A SQL boolean fragment `LOWER(TRIM(<col>)) IN ('write','edit',...)` over [`EDIT_TOOL_NAMES`],
/// so the timestamp join (which cannot call [`is_edit_tool`] from SQL) classifies an edit the
/// same way the transcript join does. `col` is a fixed internal column reference and the names
/// are static identifiers with no quotes, so the interpolation carries no injectable input.
pub fn edit_tool_sql_in_list(col: &str) -> String {
    let quoted: Vec<String> = EDIT_TOOL_NAMES.iter().map(|n| format!("'{n}'")).collect();
    format!("LOWER(TRIM({col})) IN ({})", quoted.join(", "))
}

/// A tool call that failed, then was retried within `window` turns (the retry is another call
/// with the same input fingerprint, which the caller resolves). Only counts when the first call
/// actually failed: a clean call followed by a benign repeat is a re-read, not an error-retry.
pub fn is_error_then_retry(
    failed: bool,
    call_ordinal: u32,
    retry_ordinals: &[u32],
    window: u32,
) -> bool {
    failed
        && retry_ordinals
            .iter()
            .any(|&o| o > call_ordinal && o <= call_ordinal.saturating_add(window))
}

/// Whether a user turn is actually an interrupt marker: the user pressed ESC to stop the
/// agent mid-action ("[Request interrupted by user]" / "...for tool use"). This is a
/// distinct, high-precision dissatisfaction signal from explicit complaint language: the
/// user halted the current trajectory. It is independent of the correction heuristic, so
/// it gives the corpus a second trustworthy signal type rather than leaning on one.
pub fn is_user_interrupt(text: &str) -> bool {
    text.trim()
        .to_lowercase()
        .contains("[request interrupted by user")
}

pub fn is_approval_or_continuation(raw_lower: &str) -> bool {
    if APPROVAL_PHRASES.iter().any(|p| raw_lower.contains(p)) {
        return true;
    }
    let mut saw_alpha = false;
    for tok in raw_lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if tok.is_empty() || tok.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        saw_alpha = true;
        if !APPROVAL_TOKENS.contains(&tok) {
            return false;
        }
    }
    saw_alpha
}

/// Classify a user turn that follows substantial assistant work. `terse_max` is the
/// character cutoff under which a non-complaint follow-up counts as a terse correction
/// (callers pass the user's personal P25 baseline for Claude Code, or
/// [`DEFAULT_TERSE_MAX_CHARS`] for transcript surfaces).
///
/// Order matters: an explicit complaint always wins, then a pure number or punctuation
/// turn (a menu pick like "1") is never a correction, then long non-complaints are new
/// instructions, then approvals and continuations are suppressed, then a short follow-up
/// is the conservative terse default.
pub fn classify_correction(human: &str, terse_max: usize) -> CorrectionClass {
    let raw = human.trim().to_lowercase();
    if raw.is_empty() {
        return CorrectionClass::None;
    }
    if has_negative_cue(&raw) {
        return CorrectionClass::Explicit;
    }
    // No letters at all means a menu selection ("1", "2.") or trivial token, never a
    // complaint. This is the dominant Claude Code false positive ("lets do 1" is caught by
    // the approval guard below; a bare "1" is caught here).
    if !raw.chars().any(|c| c.is_ascii_alphabetic()) {
        return CorrectionClass::None;
    }
    if raw.chars().count() > terse_max {
        return CorrectionClass::None;
    }
    if is_approval_or_continuation(&raw) {
        return CorrectionClass::None;
    }
    CorrectionClass::Terse
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_aheads_and_menu_picks_are_not_corrections() {
        for s in [
            "lets do 1",
            "do 2",
            "1",
            "2.",
            "3",
            "proceed",
            "phase 4",
            "phase 2",
            "1 then 2",
            "do it",
            "ship it",
            "next",
            "yes",
            "ok",
            "okay",
            "sure",
            "go ahead",
            "looks good",
            "sounds good",
            "thanks",
            "thank you",
            "perfect, ship it",
            "leave Phase 3 as-is and move to Phase 4",
            "1 then 2 please",
            "no problem, thanks",
        ] {
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::None,
                "should not flag: {s:?}"
            );
        }
    }

    #[test]
    fn explicit_complaints_flag_high_confidence_any_length() {
        for s in [
            "no, that's broken, revert",
            "nope",
            "that's wrong",
            "this doesn't work",
            "the read compaction is not working",
            "undo that",
            "go back to the previous version",
            "that's not what I asked for",
            // long, but an explicit complaint must still flag as high confidence
            "this whole approach is wrong and we need to go back to the version from before because the build is now broken in three places and the tests do not work",
        ] {
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::Explicit,
                "should flag explicit: {s:?}"
            );
        }
    }

    #[test]
    fn short_non_complaint_followup_is_terse() {
        // No approval tokens, no complaint cue, short: a conservative redirect. Kept as a
        // low-confidence correction, including a bare "no" (a pushback without a cue word).
        for s in ["use the other file instead", "no"] {
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::Terse,
                "should be terse: {s:?}"
            );
        }
    }

    #[test]
    fn edit_tools_recognized_across_surfaces() {
        for t in ["Write", "edit", "MultiEdit", "str_replace", "create_file", "apply_patch"] {
            assert!(is_edit_tool(t), "should be an edit tool: {t}");
        }
        for t in ["Read", "Grep", "Shell", "Bash", "MCP:save_issue", "Glob"] {
            assert!(!is_edit_tool(t), "should not be an edit tool: {t}");
        }
    }

    #[test]
    fn edit_tool_sql_in_list_matches_the_edit_tool_set() {
        let sql = edit_tool_sql_in_list("d2.tool_name");
        assert!(sql.starts_with("LOWER(TRIM(d2.tool_name)) IN ("));
        // Every name in the shared set appears, lowercased and quoted, so the timestamp join
        // classifies an edit exactly as is_edit_tool does.
        for name in EDIT_TOOL_NAMES {
            assert!(sql.contains(&format!("'{name}'")), "missing {name} in: {sql}");
            assert!(is_edit_tool(name));
        }
    }

    #[test]
    fn reedit_only_inside_window_and_after_touch() {
        let w = STRUCTURAL_WINDOW_TURNS;
        // Edit one turn after the read: churn.
        assert!(is_immediate_reedit(10, &[11], w));
        // Edit exactly at the window edge counts; just past it does not.
        assert!(is_immediate_reedit(10, &[13], w));
        assert!(!is_immediate_reedit(10, &[14], w));
        // An edit before the touch is unrelated; no edits means no signal.
        assert!(!is_immediate_reedit(10, &[9], w));
        assert!(!is_immediate_reedit(10, &[], w));
    }

    #[test]
    fn error_retry_requires_a_failure_and_a_retry_in_window() {
        let w = STRUCTURAL_WINDOW_TURNS;
        // Failed, then retried next turn: signal.
        assert!(is_error_then_retry(true, 5, &[6], w));
        // Retry past the window does not count.
        assert!(!is_error_then_retry(true, 5, &[9], w));
        // A clean call retried is a re-read, not an error-retry.
        assert!(!is_error_then_retry(false, 5, &[6], w));
        // Failed but never retried: no churn signal (the agent moved on).
        assert!(!is_error_then_retry(true, 5, &[], w));
    }

    #[test]
    fn detects_user_interrupt_markers() {
        assert!(is_user_interrupt("[Request interrupted by user]"));
        assert!(is_user_interrupt(
            "[Request interrupted by user for tool use]"
        ));
        assert!(!is_user_interrupt("interrupt the build please"));
        assert!(!is_user_interrupt("looks good"));
    }

    #[test]
    fn long_noncomplaint_is_not_a_correction() {
        let long = "why don't we take a step back and design ctx to be an agent platform \
                    that can take all these different operator surfaces as plugins and \
                    normalize them internally before feeding the model";
        assert_eq!(
            classify_correction(long, DEFAULT_TERSE_MAX_CHARS),
            CorrectionClass::None
        );
    }
}
