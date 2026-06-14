use std::collections::HashMap;

use super::generic::{compress_generic, truncate_to_budget};
use super::types::{CompressContext, CompressOptions, CompressResult};

pub fn compress_grep_output(
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
            strategy: "grep-passthrough".into(),
        };
    }

    let mut by_file: HashMap<String, Vec<String>> = HashMap::new();
    for line in input.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (file, rest) = if let Some((f, r)) = line.split_once(':') {
            (f.to_string(), r.to_string())
        } else {
            ("(output)".into(), line.to_string())
        };
        by_file.entry(file).or_default().push(rest);
    }

    let max_per_file = 5usize;
    let mut parts: Vec<String> = Vec::new();
    let mut files: Vec<_> = by_file.into_iter().collect();
    files.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let file_count = files.len();

    for (file, matches) in files.iter().take(20) {
        let show: Vec<_> = matches.iter().take(max_per_file).cloned().collect();
        let extra = matches.len().saturating_sub(max_per_file);
        parts.push(format!("{file} ({} matches)", matches.len()));
        for m in show {
            parts.push(format!("  {m}"));
        }
        if extra > 0 {
            parts.push(format!("  … {extra} more matches in this file"));
        }
    }
    if file_count > 20 {
        parts.push(format!("… {} more files omitted", file_count - 20));
    }

    let mut text = parts.join("\n");
    if !ctx.prompt_keywords.is_empty() && text.chars().count() > opts.target_chars {
        return compress_generic(input, opts, ctx, "grep-keywords");
    }
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 40);
    }
    if text.chars().count() >= chars_in {
        return compress_generic(input, opts, ctx, "grep-generic");
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        strategy: "grep".into(),
        text,
    }
}
