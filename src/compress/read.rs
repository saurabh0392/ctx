use super::generic::truncate_to_budget;
use super::types::{CompressContext, CompressOptions, CompressResult};

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
        outline.push(format!("First lines:"));
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
