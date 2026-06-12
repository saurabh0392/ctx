use super::generic::{compress_generic, truncate_to_budget};
use super::types::{CompressContext, CompressOptions, CompressResult};

const FAILURE_MARKERS: &[&str] = &[
    "FAILED",
    "FAIL:",
    "failures:",
    "error:",
    "Error:",
    "panicked at",
    "assertion",
    "AssertionError",
    "E   ",
    "✕",
    "--- FAIL:",
    "test result: FAILED",
    "not ok ",
];

pub fn compress_test_output(
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
            strategy: "test-passthrough".into(),
        };
    }

    let mut failure_blocks: Vec<String> = Vec::new();
    let mut summary_lines: Vec<String> = Vec::new();
    let mut current_block: Vec<String> = Vec::new();
    let mut in_failure = false;

    for line in input.lines() {
        let is_fail = FAILURE_MARKERS.iter().any(|m| line.contains(m));
        let is_summary = line.contains("test result:")
            || line.contains("tests passed")
            || line.contains("passed;")
            || line.contains("Test Suites:")
            || line.contains("Tests:");

        if is_summary {
            summary_lines.push(line.to_string());
            continue;
        }

        if is_fail {
            if !in_failure && !current_block.is_empty() {
                failure_blocks.push(current_block.join("\n"));
                current_block.clear();
            }
            in_failure = true;
            current_block.push(line.to_string());
        } else if in_failure {
            if line.trim().is_empty() || line.starts_with("   ") || line.starts_with("\t") {
                current_block.push(line.to_string());
            } else {
                failure_blocks.push(current_block.join("\n"));
                current_block.clear();
                in_failure = false;
            }
        }
    }
    if !current_block.is_empty() {
        failure_blocks.push(current_block.join("\n"));
    }

    let mut parts: Vec<String> = Vec::new();
    if failure_blocks.is_empty() {
        parts.push("Tests finished. No failure markers found in output.".into());
        if let Some(s) = summary_lines.last() {
            parts.push(s.clone());
        }
    } else {
        parts.push(format!("{} failure block(s):", failure_blocks.len()));
        for (i, block) in failure_blocks.iter().take(8).enumerate() {
            parts.push(format!("--- failure {} ---", i + 1));
            parts.push(block.clone());
        }
        if failure_blocks.len() > 8 {
            parts.push(format!(
                "… {} more failure blocks omitted",
                failure_blocks.len() - 8
            ));
        }
        for s in summary_lines {
            parts.push(s);
        }
    }

    let mut text = parts.join("\n");
    if text.chars().count() > opts.target_chars {
        text = truncate_to_budget(&text, opts.target_chars, 50);
    }
    if text.chars().count() >= chars_in {
        return compress_generic(input, opts, ctx, "test-generic");
    }

    CompressResult {
        chars_in,
        chars_out: text.chars().count(),
        text,
        strategy: "test-runner".into(),
    }
}
