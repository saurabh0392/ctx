use super::generic::truncate_to_budget;
use super::types::{CompressContext, CompressOptions, CompressResult};

/// Share of the prose budget spent on the head; the rest holds the tail, so the end of a doc or
/// data file survives instead of being dropped by head-only truncation.
const PROSE_HEAD_BUDGET_NUM: usize = 3;
const PROSE_HEAD_BUDGET_DEN: usize = 5;
/// Room reserved for the header and the "… N lines …" marker so they never push out real content.
const PROSE_OVERHEAD_CHARS: usize = 120;

pub fn compress_read_output(
    input: &str,
    file_path: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "read-passthrough".into(),
        };
    }

    // The code-signature outline is only useful for source. Applied to a markdown/prose/data file it
    // finds no `fn`/`struct` lines and collapses to the first 30 lines, dropping the body the agent
    // read the file for (CTX-58). Route non-code files to a structure + head + tail view instead.
    if is_code_file(file_path) {
        compress_code_outline(input, file_path, opts, ctx)
    } else {
        compress_prose(input, file_path, opts, ctx)
    }
}

/// Source-code strategy: a signature outline (functions, types, imports) plus any keyword lines.
fn compress_code_outline(
    input: &str,
    file_path: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    let lines: Vec<&str> = input.lines().collect();
    let total = lines.len();

    let mut outline: Vec<String> = Vec::new();
    outline.push(format!(
        "File: {file_path} ({total} lines, compressed for context budget)"
    ));
    if !ctx.cwd.is_empty() && !file_path.starts_with('/') {
        outline.push(format!("Working dir: {}", ctx.cwd.trim_end_matches('/')));
    }

    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("pub fn ")
            || t.starts_with("pub async fn ")
            || t.starts_with("fn ")
            || t.starts_with("impl ")
            || t.starts_with("pub struct ")
            || t.starts_with("struct ")
            || t.starts_with("pub enum ")
            || t.starts_with("enum ")
            || t.starts_with("export ")
            || t.starts_with("import ")
            || t.starts_with("class ")
            || t.starts_with("def ")
        {
            outline.push(format!("L{}: {t}", i + 1));
        }
    }

    if !ctx.prompt_keywords.is_empty() {
        let lower_kw: Vec<String> = ctx
            .prompt_keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();
        for (i, line) in lines.iter().enumerate() {
            let ll = line.to_lowercase();
            if lower_kw.iter().any(|k| !k.is_empty() && ll.contains(k)) {
                outline.push(format!("L{}: {}", i + 1, line.trim()));
            }
        }
    }

    if outline.len() <= 1 {
        outline.push("First lines:".to_string());
        for line in lines.iter().take(30) {
            outline.push(line.to_string());
        }
        outline.push(format!("… {} lines omitted", total.saturating_sub(30)));
    }

    let mut text = outline.join("\n");
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 50);
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "read".into(),
    }
}

/// Non-code strategy: keep the head and tail of the file, splitting the budget so the end survives
/// (head-only truncation always dropped it). Budget-aware by construction, so the tail is never
/// re-cut by a final head-truncate. Points at ctx_expand for the omitted middle.
fn compress_prose(
    input: &str,
    file_path: &str,
    opts: &CompressOptions,
    _ctx: &CompressContext,
) -> CompressResult {
    let chars_in = input.chars().count();
    let lines: Vec<&str> = input.lines().collect();
    let total = lines.len();

    let header = format!(
        "File: {file_path} ({total} lines, compressed for context budget; ctx_expand for the full file)"
    );
    let budget = opts
        .target_chars
        .saturating_sub(header.chars().count() + PROSE_OVERHEAD_CHARS);
    let head_budget = budget * PROSE_HEAD_BUDGET_NUM / PROSE_HEAD_BUDGET_DEN;
    let tail_budget = budget.saturating_sub(head_budget);

    // Head lines up to head_budget; always keep at least one so a single huge line still shows.
    let mut head_end = 0usize;
    let mut used = 0usize;
    while head_end < total {
        let c = lines[head_end].chars().count() + 1;
        if used + c > head_budget && head_end > 0 {
            break;
        }
        used += c;
        head_end += 1;
    }

    // Tail lines from the end, within tail_budget, never overlapping the head.
    let mut tail_start = total;
    let mut tused = 0usize;
    while tail_start > head_end {
        let c = lines[tail_start - 1].chars().count() + 1;
        if tused + c > tail_budget && tail_start < total {
            break;
        }
        tused += c;
        tail_start -= 1;
    }

    let omitted = tail_start.saturating_sub(head_end);
    let mut out: Vec<String> = Vec::with_capacity(head_end + (total - tail_start) + 2);
    out.push(header);
    out.extend(lines[..head_end].iter().map(|s| s.to_string()));
    if omitted > 0 {
        out.push(format!("… {omitted} lines … (ctx_expand for the full file)"));
    }
    out.extend(lines[tail_start..].iter().map(|s| s.to_string()));

    let mut text = out.join("\n");
    // Guard only against a pathological single giant head line; head+tail already fit the budget.
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, head_end + 2);
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "read-prose".into(),
    }
}

/// True for a source-code file, by extension. Anything else (markdown, txt, json, yaml, csv, log,
/// unknown) is treated as prose/data.
fn is_code_file(file_path: &str) -> bool {
    let ext = file_path
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    // No dot at all, or the "extension" is the whole path (a dotfile like ".gitignore"): not code.
    if !file_path.contains('.') {
        return false;
    }
    matches!(
        ext.as_str(),
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cxx"
            | "rb"
            | "php"
            | "cs"
            | "swift"
            | "kt"
            | "kts"
            | "scala"
            | "sh"
            | "bash"
            | "zsh"
            | "lua"
            | "r"
            | "m"
            | "mm"
            | "dart"
            | "vue"
            | "svelte"
            | "sql"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> CompressOptions {
        CompressOptions {
            target_chars: 400,
            ..Default::default()
        }
    }

    #[test]
    fn code_file_uses_signature_outline() {
        let src = (0..200)
            .map(|i| {
                if i == 5 {
                    "pub fn important_thing() {".to_string()
                } else {
                    format!("    let x{i} = {i};")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let r = compress_read_output(&src, "src/lib.rs", &opts(), &CompressContext::default());
        assert_eq!(r.strategy, "read");
        assert!(r.text.contains("important_thing"));
    }

    #[test]
    fn prose_file_keeps_head_and_tail_not_just_head() {
        // The failure mode: a markdown memory file gutted to the first 30 lines. The end must survive.
        let mut lines: Vec<String> = Vec::new();
        lines.push("# Project memory".to_string());
        for i in 0..200 {
            lines.push(format!("body line {i} with enough text to exceed the budget comfortably"));
        }
        lines.push("## Final decision: ship it".to_string());
        lines.push("the concluding detail the agent needed".to_string());
        let doc = lines.join("\n");
        let r = compress_read_output(&doc, "notes/memory.md", &opts(), &CompressContext::default());
        assert_eq!(r.strategy, "read-prose");
        assert!(r.text.contains("# Project memory"), "keeps the head");
        assert!(r.text.contains("Final decision"), "keeps the tail heading");
        assert!(r.text.contains("concluding detail"), "keeps the last line");
        assert!(r.text.contains("ctx_expand"), "points at recovery");
    }

    #[test]
    fn is_code_file_discriminates() {
        assert!(is_code_file("src/lib.rs"));
        assert!(is_code_file("/a/b/Main.ts"));
        assert!(!is_code_file("notes/memory.md"));
        assert!(!is_code_file("data/atlas.json"));
        assert!(!is_code_file("README"));
        assert!(!is_code_file(".gitignore"));
    }
}
