use super::types::{CompressContext, CompressOptions, CompressResult};

const SECRET_PATTERNS: &[&str] = &[
    "sk-ant-",
    "sk-",
    "Bearer ",
    "bearer ",
    "api_key=",
    "API_KEY=",
    "password=",
    "PASSWORD=",
    "secret=",
    "SECRET=",
];

pub fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for pat in SECRET_PATTERNS {
        if let Some(idx) = out.find(pat) {
            let line_start = out[..idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = out[idx..]
                .find('\n')
                .map(|i| idx + i)
                .unwrap_or(out.len());
            out.replace_range(line_start..line_end, "[ctx redacted secret line]");
        }
    }
    out
}

pub fn dedupe_lines(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<String> = None;
    let mut run = 0usize;
    for line in text.lines() {
        let key = line.trim();
        if prev.as_deref() == Some(key) && !key.is_empty() {
            run += 1;
            continue;
        }
        if run > 0 {
            if let Some(p) = prev {
                out.push(format!("{p} (×{})", run + 1));
            }
            run = 0;
        }
        prev = Some(line.to_string());
        out.push(line.to_string());
    }
    if run > 0 {
        if let Some(p) = prev {
            out.push(format!("{p} (×{})", run + 1));
        }
    }
    out.join("\n")
}

pub fn collapse_blank_runs(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut blank_run = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                out.push(line);
            }
            continue;
        }
        blank_run = 0;
        out.push(line);
    }
    out.join("\n")
}

pub fn truncate_to_budget(text: &str, budget: usize, head_lines: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut head: Vec<String> = lines
        .iter()
        .take(head_lines)
        .map(|l| (*l).to_string())
        .collect();
    let omitted = lines.len().saturating_sub(head_lines);
    head.push(format!(
        "… {omitted} lines omitted ({}) chars total). Ask to re-run with a narrower scope if you need more.",
        text.chars().count()
    ));
    let joined = head.join("\n");
    if joined.chars().count() <= budget {
        return joined;
    }
    joined.chars().take(budget.saturating_sub(1)).collect::<String>() + "…"
}

pub fn compress_generic(
    input: &str,
    opts: &CompressOptions,
    ctx: &CompressContext,
    strategy: &str,
) -> CompressResult {
    let chars_in = input.chars().count();
    if chars_in <= opts.target_chars {
        return CompressResult {
            text: input.to_string(),
            chars_in,
            chars_out: chars_in,
            strategy: "passthrough".into(),
        };
    }

    let mut text = input.chars().take(opts.max_input_chars).collect::<String>();
    if opts.redact_secrets {
        text = redact_secrets(&text);
    }
    text = collapse_blank_runs(&text);
    text = dedupe_lines(&text);

    if !ctx.prompt_keywords.is_empty() && text.chars().count() > opts.target_chars {
        text = rank_by_keywords(&text, &ctx.prompt_keywords, opts.target_chars);
    } else if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 40);
    }

    CompressResult {
        chars_out: text.chars().count(),
        text,
        chars_in,
        strategy: strategy.to_string(),
    }
}

fn rank_by_keywords(text: &str, keywords: &[String], budget: usize) -> String {
    let lower_kw: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
    let mut matched: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for line in text.lines() {
        let ll = line.to_lowercase();
        if lower_kw.iter().any(|k| !k.is_empty() && ll.contains(k)) {
            matched.push(line.to_string());
        } else {
            other.push(line.to_string());
        }
    }
    let mut out: Vec<String> = matched;
    if out.is_empty() {
        return truncate_to_budget(text, budget, 30);
    }
    let hidden = other.len();
    if hidden > 0 {
        out.push(format!("… {hidden} lines without prompt keywords omitted"));
    }
    let joined = out.join("\n");
    if joined.chars().count() <= budget {
        joined
    } else {
        truncate_to_budget(&joined, budget, out.len().min(30))
    }
}
