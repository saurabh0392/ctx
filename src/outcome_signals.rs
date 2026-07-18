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
    /// Topic pivot or session redirect ("drop it", "lets do the fun stuff"). Recorded as
    /// `session_steer` for observation only; must not feed the causal gate (CTX-50).
    Steer,
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

/// Session steers: topic pivots and scope redirects without output-specific complaints.
/// Checked before negative cues so a bare "nope" plus a pivot is not a gate correction.
const STEER_PHRASES: &[&str] = &[
    "drop it",
    "just drop it",
    "start scoping",
    "do the fun stuff",
    "lets do the fun stuff",
    "let's do the fun stuff",
    "design and build",
    "move on",
    "move to the next",
    "whats next",
    "what's next",
    "start sim",
    "restart sim",
    "run sim again",
];

/// Output-quality complaints that keep a steer from swallowing a real pushback.
const OUTPUT_COMPLAINT_PHRASES: &[&str] = &[
    "bad bg",
    "bad image",
    "bad background",
    "looks wrong",
    "still wrong",
    "still broken",
    "not what i",
    "not what we",
    "wrong file",
    "wrong approach",
    "doesn't look",
    "does not look",
];

/// Bare dismissals that are not output-specific on their own ("nope", "no,").
const BARE_DISMISSAL_CUES: &[&str] = &["nope", "no,", "no "];

/// Workflow redirects that are not pushback on tool output. Matched as lowercase substrings.
const WORKFLOW_PHRASES: &[&str] = &[
    "committ and push",
    "commit and push",
    "commit & push",
    "push it",
    "fold and adr",
    "narrow pass",
    "open pr",
    "create pr",
    "merge it",
    "run tests",
    "cargo test",
    "ctx setup",
    "reload window",
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

/// Whether the user pivoted topic or scope without complaining about tool output.
pub fn is_session_steer(text: &str) -> bool {
    let raw = text.trim().to_lowercase();
    if raw.is_empty() || is_system_turn(text) || is_workflow_command(text) {
        return false;
    }
    if !STEER_PHRASES.iter().any(|p| raw.contains(p)) {
        return false;
    }
    !has_output_specific_complaint(&raw)
}

/// Complaint language aimed at output quality, not a bare dismissal or pivot.
pub fn has_output_specific_complaint(raw_lower: &str) -> bool {
    if OUTPUT_COMPLAINT_PHRASES
        .iter()
        .any(|p| raw_lower.contains(p))
    {
        return true;
    }
    for cue in NEGATIVE_CUES {
        if !raw_lower.contains(cue) {
            continue;
        }
        if BARE_DISMISSAL_CUES.contains(cue) {
            continue;
        }
        return true;
    }
    false
}

/// Assistant narration that the prior tool result was compressed or truncated.
pub fn is_compression_narration(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    COMPRESSION_NARRATION_CUES.iter().any(|c| lower.contains(c))
}

const COMPRESSION_NARRATION_CUES: &[&str] = &[
    "compressed",
    "compression",
    "truncat",
    "trimmed output",
    "output was trimmed",
    "context limit",
    "too large to include",
    "couldn't fit",
    "could not fit",
];

/// Shell tools sometimes used to write trimmed JSON or dumps back to disk.
pub const BYPASS_SHELL_TOOL_NAMES: &[&str] = &["bash", "shell"];

pub fn is_shell_bypass_call(tool_name: &str, fingerprint: &str) -> bool {
    let n = tool_name.trim().to_ascii_lowercase();
    if !BYPASS_SHELL_TOOL_NAMES.contains(&n.as_str()) {
        return false;
    }
    let fp = fingerprint.trim().to_ascii_lowercase();
    fp.contains("json")
        || fp.contains("<<")
        || fp.contains("heredoc")
        || fp.contains(" > ")
        || fp.contains(">>")
        || fp.contains("python3 -c")
        || fp.contains("node -e")
}

/// Observation-only: trimmed output, assistant named compression, then a shell bypass (CTX-50).
pub fn is_compression_workaround(
    applied: bool,
    lines_drop: i64,
    call_ordinal: u32,
    assistant_turns: &[(u32, &str)],
    later_calls: &[(u32, &str, &str)],
    window: u32,
) -> bool {
    if !applied || lines_drop <= 0 {
        return false;
    }
    let window_end = call_ordinal.saturating_add(window);
    let narrated = assistant_turns
        .iter()
        .any(|(o, text)| *o > call_ordinal && *o <= window_end && is_compression_narration(text));
    if !narrated {
        return false;
    }
    later_calls.iter().any(|(o, tool, fp)| {
        *o > call_ordinal && *o <= window_end && is_shell_bypass_call(tool, fp)
    })
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

/// Tools whose output is session state, not a path or shell command. When the fingerprint
/// falls back to the bare tool name (legacy rows), any later call of the same tool falsely
/// looked like a re-read. Content fingerprints and the join exclusion below fix that.
pub const STATE_MUTATION_TOOL_NAMES: &[&str] = &["todowrite", "task"];

pub fn is_state_mutation_tool(tool_name: &str) -> bool {
    let n = tool_name.trim().to_ascii_lowercase();
    STATE_MUTATION_TOOL_NAMES.contains(&n.as_str())
}

/// True when a decision still uses the pre-CTX-49 bare tool-name fingerprint.
pub fn is_legacy_state_mutation_fingerprint(tool_name: &str, fingerprint: &str) -> bool {
    is_state_mutation_tool(tool_name) && fingerprint == tool_name
}

/// SQL fragment excluding routine TodoWrite/Task churn from the re-read EXISTS clause.
/// When the decision used a legacy bare fingerprint, a later call of the same tool is not
/// treated as needing the prior output back.
pub fn reread_legacy_state_mutation_exclusion_sql() -> String {
    let quoted: Vec<String> = STATE_MUTATION_TOOL_NAMES
        .iter()
        .map(|n| format!("'{n}'"))
        .collect();
    let in_list = quoted.join(", ");
    format!(
        "AND NOT (
            LOWER(TRIM(compress_decisions.tool_name)) IN ({in_list})
            AND compress_decisions.command_or_path = compress_decisions.tool_name
            AND LOWER(TRIM(d2.tool_name)) = LOWER(TRIM(compress_decisions.tool_name))
        )"
    )
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

/// System or plumbing turns that must never count as user corrections.
pub fn is_system_turn(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    if is_user_interrupt(t) {
        return true;
    }
    let lower = t.to_lowercase();
    lower.starts_with("<task-notification>")
        || lower.starts_with("<task_notification>")
        || lower.starts_with("<system-reminder>")
}

/// Short workflow commands ("commit and push", "fold and ADR") are not tool-output corrections.
pub fn is_workflow_command(text: &str) -> bool {
    let raw = text.trim().to_lowercase();
    if raw.is_empty() {
        return false;
    }
    WORKFLOW_PHRASES.iter().any(|p| raw.contains(p))
}

/// Whether this decision earns `outcome_correction` on the causal gate (CTX-48 / ADR 0033).
/// Uniform for every tool: explicit complaint language AND ctx actually trimmed lines.
pub fn gate_correction_label(explicit_complaint: bool, applied: bool, lines_drop: i64) -> bool {
    explicit_complaint && applied && lines_drop > 0
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
    if raw.is_empty() || is_system_turn(human) || is_workflow_command(human) {
        return CorrectionClass::None;
    }
    if is_session_steer(human) {
        return CorrectionClass::Steer;
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
        for t in [
            "Write",
            "edit",
            "MultiEdit",
            "str_replace",
            "create_file",
            "apply_patch",
        ] {
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
            assert!(
                sql.contains(&format!("'{name}'")),
                "missing {name} in: {sql}"
            );
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

    #[test]
    fn interrupts_and_system_turns_are_not_corrections() {
        assert!(is_system_turn("[Request interrupted by user for tool use]"));
        assert!(is_system_turn(
            "<task-notification>done</task-notification>"
        ));
        assert_eq!(
            classify_correction("[Request interrupted by user]", DEFAULT_TERSE_MAX_CHARS),
            CorrectionClass::None
        );
        assert_eq!(
            classify_correction(
                "<system-reminder>be concise</system-reminder>",
                DEFAULT_TERSE_MAX_CHARS
            ),
            CorrectionClass::None
        );
    }

    #[test]
    fn workflow_commands_are_not_corrections() {
        for s in [
            "commit and push",
            "fold and ADR",
            "narrow pass",
            "cargo test",
        ] {
            assert!(is_workflow_command(s), "workflow: {s}");
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::None,
                "workflow must not flag: {s}"
            );
        }
    }

    #[test]
    fn gate_correction_requires_explicit_trim_and_applied() {
        assert!(!gate_correction_label(false, true, 10));
        assert!(!gate_correction_label(true, false, 10));
        assert!(!gate_correction_label(true, true, 0));
        assert!(gate_correction_label(true, true, 10));
    }

    #[test]
    fn session_steers_are_not_explicit_corrections() {
        for s in [
            "nope nope.. lets do the fun stuff",
            "just drop it",
            "start scoping",
            "design and build the dashboard next",
            "whats next?",
        ] {
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::Steer,
                "should steer: {s:?}"
            );
        }
    }

    #[test]
    fn output_complaints_stay_explicit_even_with_nope() {
        for s in [
            "nope. bad bg image",
            "nope that's wrong",
            "still broken, revert",
        ] {
            assert_eq!(
                classify_correction(s, DEFAULT_TERSE_MAX_CHARS),
                CorrectionClass::Explicit,
                "should stay explicit: {s:?}"
            );
        }
    }

    #[test]
    fn compression_workaround_needs_narration_and_shell_bypass() {
        let w = STRUCTURAL_WINDOW_TURNS;
        let assistants = [(2u32, "output was trimmed so I'll write the json to disk")];
        let bypass = [(3u32, "Bash", "python3 -c 'open(\"x.json\")'")];
        assert!(is_compression_workaround(
            true,
            40,
            1,
            &assistants,
            &bypass,
            w
        ));
        assert!(!is_compression_workaround(
            true,
            40,
            1,
            &[(2, "continuing")],
            &bypass,
            w
        ));
        assert!(!is_compression_workaround(
            true,
            40,
            1,
            &assistants,
            &[(3, "Read", "/a.rs")],
            w
        ));
        assert!(!is_compression_workaround(
            false,
            40,
            1,
            &assistants,
            &bypass,
            w
        ));
    }
}
