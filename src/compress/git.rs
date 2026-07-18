use super::generic::{compress_generic, truncate_to_budget};
use super::types::{CompressContext, CompressOptions, CompressResult};

pub fn compress_git_status(
    input: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "git-status-passthrough".into(),
        };
    }

    let mut staged: Vec<String> = Vec::new();
    let mut unstaged: Vec<String> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();
    let mut branch_line = String::new();

    for line in input.lines() {
        if line.starts_with("On branch") || line.starts_with("##") {
            branch_line = line.to_string();
            continue;
        }
        if line.starts_with("Changes to be committed") {
            continue;
        }
        if line.starts_with("Changes not staged") {
            continue;
        }
        if line.starts_with("Untracked files") {
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with('(') {
            continue;
        }
        if line.starts_with('\t') || line.starts_with("  ") {
            let entry = t.trim_start_matches(['\t', ' ']).to_string();
            if input.contains("Changes not staged") && unstaged.len() < staged.len() + 5 {
                unstaged.push(entry);
            } else if input.contains("Untracked files") && untracked.len() <= unstaged.len() {
                untracked.push(entry);
            } else {
                staged.push(entry);
            }
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !branch_line.is_empty() {
        parts.push(branch_line);
    }
    if !staged.is_empty() {
        parts.push(format!("Staged ({}): {}", staged.len(), staged.join(", ")));
    }
    if !unstaged.is_empty() {
        parts.push(format!(
            "Modified ({}): {}",
            unstaged.len(),
            unstaged.join(", ")
        ));
    }
    if !untracked.is_empty() {
        let show: Vec<_> = untracked.iter().take(8).cloned().collect();
        let extra = untracked.len().saturating_sub(8);
        let mut line = format!("Untracked ({}): {}", untracked.len(), show.join(", "));
        if extra > 0 {
            line.push_str(&format!(", … {extra} more"));
        }
        parts.push(line);
    }

    let mut text = if parts.is_empty() {
        compress_generic(input, opts, ctx, "git-status-generic").text
    } else {
        parts.join("\n")
    };

    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 20);
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "git-status".into(),
    }
}

pub fn compress_git_diff(
    input: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "git-diff-passthrough".into(),
        };
    }

    let mut out: Vec<String> = Vec::new();
    let mut current_file: Option<String> = None;
    let mut hunk_lines: Vec<String> = Vec::new();

    for line in input.lines() {
        if line.starts_with("diff --git") || line.starts_with("+++ ") || line.starts_with("--- ") {
            if let Some(f) = current_file.take() {
                flush_hunk(&mut out, &f, &mut hunk_lines, opts);
            }
            if line.starts_with("+++ b/") {
                current_file = Some(line.trim_start_matches("+++ b/").to_string());
            } else if line.starts_with("diff --git") {
                current_file = line
                    .split_whitespace()
                    .nth(3)
                    .map(|s| s.trim_start_matches("b/").to_string());
            }
            out.push(line.to_string());
            continue;
        }
        if line.starts_with("@@") {
            if let Some(ref f) = current_file {
                flush_hunk(&mut out, f, &mut hunk_lines, opts);
            }
            out.push(line.to_string());
            continue;
        }
        if (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
        {
            hunk_lines.push(line.to_string());
        }
    }
    if let Some(f) = current_file {
        flush_hunk(&mut out, &f, &mut hunk_lines, opts);
    }

    let mut text = out.join("\n");
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 60);
    }
    // A diff with no recognizable hunks (`--stat`, `--name-only`, `--summary`) parses to an empty
    // `out`, which would erase the whole output. Fall back to generic compaction, which truncates
    // to budget while keeping content, so the summary survives instead of vanishing (CTX-40). The
    // `>= chars_in` check catches the case where structured parsing didn't actually save anything.
    if text.trim().is_empty() || text.chars().count() >= chars_in {
        return compress_generic(input, opts, ctx, "git-diff-generic");
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "git-diff".into(),
    }
}

fn flush_hunk(out: &mut Vec<String>, file: &str, hunk: &mut Vec<String>, opts: &CompressOptions) {
    if hunk.is_empty() {
        return;
    }
    let max = if opts.preserve_errors { 24 } else { 12 };
    if hunk.len() > max {
        let kept: Vec<_> = hunk
            .iter()
            .take(max / 2)
            .chain(hunk.iter().skip(hunk.len().saturating_sub(max / 2)))
            .cloned()
            .collect();
        out.push(format!(
            "… {file}: {} diff lines, showing {} …",
            hunk.len(),
            kept.len()
        ));
        out.extend(kept);
    } else {
        out.append(hunk);
    }
}

pub fn compress_git_log(
    input: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "git-log-passthrough".into(),
        };
    }

    let mut lines: Vec<String> = Vec::new();
    for line in input.lines() {
        if line.starts_with("commit ") || line.len() > 8 && line.chars().nth(8) == Some(' ') {
            lines.push(line.to_string());
        } else if line.starts_with("Author:") || line.starts_with("Date:") {
            continue;
        } else if !line.trim().is_empty() {
            let one = line.trim();
            if one.len() > 120 {
                lines.push(format!("{}…", &one[..117]));
            } else {
                lines.push(one.to_string());
            }
        }
    }
    let mut text = lines.join("\n");
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 40);
    }
    if text.chars().count() >= chars_in.saturating_sub(50) {
        return compress_generic(input, opts, ctx, "git-log-generic");
    }
    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "git-log".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_diff_stat_is_not_erased() {
        // `git diff --stat` has no `diff --git`/`@@`/`+`/`-` line starts, so the structured parser
        // produces nothing. Before CTX-40 that returned empty text (silent data loss). It must now
        // fall back to generic compaction and keep the summary.
        let mut input = String::new();
        for i in 0..400 {
            input.push_str(&format!(
                " src/module_{i}/file_{i}.rs | {} ++++----\n",
                i % 30
            ));
        }
        input.push_str(" 400 files changed, 1234 insertions(+), 567 deletions(-)\n");
        assert!(input.chars().count() > 2_500, "input must exceed target");

        let opts = CompressOptions::default();
        let ctx = CompressContext::default();
        let r = compress_git_diff(&input, &opts, &ctx);

        assert!(
            r.chars_out > 0,
            "must never erase a --stat summary to empty"
        );
        assert!(!r.text.trim().is_empty(), "text must carry content");
        assert!(
            r.text.contains("file_0.rs") || r.text.contains("files changed"),
            "the stat content must survive, got: {}",
            r.text
        );
        assert_eq!(
            r.strategy, "git-diff-generic",
            "no hunks means it should fall back to generic, not the hunk path"
        );
        assert!(r.chars_out < r.chars_in, "it should still compress");
    }

    #[test]
    fn git_diff_with_hunks_still_uses_structured_path() {
        // A real diff with hunks must still take the structured git-diff path, not the fallback.
        let mut input =
            String::from("diff --git a/src/x.rs b/src/x.rs\n+++ b/src/x.rs\n@@ -1,3 +1,3 @@\n");
        for i in 0..400 {
            input.push_str(&format!(
                "+added line {i} with some content to grow the hunk\n"
            ));
            input.push_str(&format!(
                "-removed line {i} with some content to grow the hunk\n"
            ));
        }
        let opts = CompressOptions::default();
        let ctx = CompressContext::default();
        let r = compress_git_diff(&input, &opts, &ctx);
        assert_eq!(r.strategy, "git-diff");
        assert!(r.chars_out > 0 && r.chars_out < r.chars_in);
    }
}
