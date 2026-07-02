//! Edit/Write confirmation strategy (CTX-60). An Edit tool result echoes a `cat -n` snippet of the
//! region it just changed. For long-line files (minified or data-heavy) that snippet is enormous,
//! and line-based trimming cannot touch a single 292K-char line, which is where most of the Edit
//! sink lives. This strategy collapses those long lines while keeping the structure and the change
//! location.
//!
//! Shadow-only. The controller blocks every edit tool from the apply path by name
//! (`outcome_signals::is_edit_tool`), so this only ever measures what a trim would save. It never
//! alters what the agent sees, which is the safety rule for write confirmations: the agent must
//! never misread what it just wrote.

use super::generic::{collapse_blank_runs, collapse_long_lines, dedupe_lines, truncate_to_budget};
use super::types::{CompressContext, CompressOptions, CompressResult};

/// Any single line longer than this in an edit echo is collapsed. The agent already knows what it
/// wrote, so the interior of a giant minified line is the least useful part of the confirmation.
const EDIT_MAX_LINE_CHARS: usize = 400;

pub fn compress_edit_output(
    input: &str,
    opts: &CompressOptions,
    _ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "edit-passthrough".into(),
        };
    }

    // Collapse the long echoed lines first: that is where the bulk is. Then the usual blank/dup
    // passes, then a line budget as a backstop.
    let mut text = collapse_long_lines(input, EDIT_MAX_LINE_CHARS);
    text = collapse_blank_runs(&text);
    text = dedupe_lines(&text);
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 60);
    }

    CompressResult {
        chars_out: text.chars().count(),
        text,
        chars_in,
        strategy: "edit".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_the_giant_echoed_line_but_keeps_structure() {
        // Mimics an Edit confirmation on a long-line file: a header, then one enormous line.
        let giant = "x".repeat(292_000);
        let input = format!(
            "The file /a/b/movement-engine.ts has been updated. Here is the result of cat -n:\n1\t{giant}\n2\tconst done = true;"
        );
        let opts = CompressOptions {
            target_chars: 2_000,
            ..Default::default()
        };
        let r = compress_edit_output(&input, &opts, &CompressContext::default());
        assert_eq!(r.strategy, "edit");
        // The bulk of the giant line is gone.
        assert!(r.chars_out < 3_000, "collapsed, got {}", r.chars_out);
        assert!(r.chars_saved() > 280_000);
        // Header and the trailing real line survive.
        assert!(r.text.contains("has been updated"));
        assert!(r.text.contains("const done = true;"));
        assert!(r.text.contains("ctx_expand"));
    }

    #[test]
    fn short_confirmation_passes_through() {
        let input = "The file /a.rs has been updated.";
        let opts = CompressOptions {
            target_chars: 2_000,
            ..Default::default()
        };
        let r = compress_edit_output(input, &opts, &CompressContext::default());
        assert_eq!(r.strategy, "edit-passthrough");
        assert_eq!(r.chars_saved(), 0);
    }
}
