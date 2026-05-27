use anyhow::Result;
use std::io::{self, Read, Write};

pub fn hook() -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&input) else {
        return Ok(());
    };

    let tool_name = value
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if tool_name != "Bash" {
        return Ok(());
    }

    let command = value
        .pointer("/tool_input/command")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    if command.is_empty() || command.contains("ctx compress") {
        return Ok(());
    }

    let kind = detect_kind(command);
    let rewritten = format!("{{ {command}; }} 2>&1 | ctx compress --kind {kind}");
    value["tool_input"]["command"] = serde_json::Value::String(rewritten);

    print!("{}", serde_json::to_string(&value)?);
    Ok(())
}

fn detect_kind(cmd: &str) -> &'static str {
    let c = cmd.to_lowercase();
    if c.contains("databricks")
        || c.contains(" sql")
        || c.contains("psql")
        || c.contains("mysql")
        || c.contains(" bq ")
        || c.contains("spark-sql")
    {
        "sql"
    } else if c.contains("pytest")
        || c.contains("python -m")
        || (c.contains("python") && c.contains(".py"))
    {
        "traceback"
    } else if c.contains("git diff")
        || c.contains("git log")
        || c.contains("git show")
        || c.contains("diff ")
    {
        "diff"
    } else if c.contains("curl") || c.contains(" jq") || c.ends_with(".json") {
        "json"
    } else {
        "generic"
    }
}

pub fn run(kind: Option<&str>) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let out = match kind.unwrap_or("generic") {
        "sql" => sql(&input),
        "traceback" => traceback(&input),
        "diff" => diff(&input),
        "json" => json(&input),
        _ => generic(&input),
    };

    let saved = input.len().saturating_sub(out.len());
    if saved > 0 {
        crate::analytics::record_compress(saved);
    }

    io::stdout().write_all(out.as_bytes())?;
    Ok(())
}

fn sql(input: &str) -> String {
    const MAX_HEAD: usize = 50;
    const TAIL: usize = 5;
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() <= MAX_HEAD + TAIL + 2 {
        return input.to_string();
    }
    let total = lines.len();
    let skipped = total - MAX_HEAD - TAIL;
    format!(
        "{}\n... [{skipped} rows omitted, {total} total] ...\n{}",
        lines[..MAX_HEAD].join("\n"),
        lines[total - TAIL..].join("\n"),
    )
}

fn traceback(input: &str) -> String {
    const PREAMBLE: usize = 5;
    const TAIL: usize = 35;
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() <= PREAMBLE + TAIL + 5 {
        return input.to_string();
    }
    let total = lines.len();
    let anchor = lines
        .iter()
        .rposition(|l| l.starts_with("Traceback") || l.trim_start().starts_with("File \""))
        .map(|i| i.saturating_sub(2))
        .unwrap_or(total.saturating_sub(TAIL));

    let keep_from = anchor.min(total.saturating_sub(TAIL));
    if keep_from <= PREAMBLE {
        return input.to_string();
    }
    let skipped = keep_from - PREAMBLE;
    format!(
        "{}\n... [{skipped} lines omitted] ...\n{}",
        lines[..PREAMBLE].join("\n"),
        lines[keep_from..].join("\n"),
    )
}

fn diff(input: &str) -> String {
    const MAX_CHARS: usize = 14_000;
    const MAX_CTX: usize = 3;
    if input.len() <= MAX_CHARS {
        return input.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut ctx_run = 0usize;
    let mut skipped = 0usize;

    for line in input.lines() {
        let is_ctx = !line.starts_with('+')
            && !line.starts_with('-')
            && !line.starts_with('@')
            && !line.starts_with("diff")
            && !line.starts_with("index")
            && !line.starts_with("---")
            && !line.starts_with("+++");

        if is_ctx {
            ctx_run += 1;
            if ctx_run > MAX_CTX {
                skipped += 1;
                continue;
            }
        } else {
            if skipped > 0 {
                out.push(format!("... [{skipped} unchanged lines] ..."));
                skipped = 0;
            }
            ctx_run = 0;
        }
        out.push(line.to_string());
    }
    if skipped > 0 {
        out.push(format!("... [{skipped} unchanged lines] ..."));
    }
    out.join("\n")
}

fn json(input: &str) -> String {
    const MAX: usize = 8_000;
    if input.len() <= MAX {
        return input.to_string();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        if let Ok(pretty) = serde_json::to_string_pretty(&v) {
            if pretty.len() <= MAX {
                return pretty;
            }
            let head: String = pretty.chars().take(MAX).collect();
            return format!(
                "{head}\n... [truncated, {:.1}KB total] ...",
                input.len() as f64 / 1024.0
            );
        }
    }
    generic(input)
}

fn generic(input: &str) -> String {
    const MAX: usize = 12_000;
    const HEAD: usize = 8_000;
    const TAIL: usize = 2_000;
    if input.len() <= MAX {
        return input.to_string();
    }
    let head: String = input.chars().take(HEAD).collect();
    let tail: String = input
        .chars()
        .rev()
        .take(TAIL)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let omitted = input.len().saturating_sub(HEAD + TAIL);
    format!("{head}\n... [{omitted} chars omitted] ...\n{tail}")
}
