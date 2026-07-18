use super::generic::{compress_generic, truncate_to_budget};
use super::types::{CompressContext, CompressOptions, CompressResult};

/// Run the shipping MCP text compressor against a losslessly parsed result for shadow evidence.
/// The returned candidate is never rendered or sent to an agent in T1.
pub(crate) fn compress_mcp_result_shadow(
    result: &crate::tool_result::CanonicalMcpResult,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> Option<CompressResult> {
    if opts.preserve_errors {
        match &result.is_error {
            crate::tool_result::PreservedField::Value(true)
            | crate::tool_result::PreservedField::Opaque(_) => return None,
            crate::tool_result::PreservedField::Absent
            | crate::tool_result::PreservedField::Value(false) => {}
        }
    }
    let text = result.compressible_text()?;
    if text.is_empty() {
        return None;
    }
    Some(compress_mcp_output(&text, opts, ctx))
}

pub fn compress_mcp_output(
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
            strategy: "mcp-passthrough".into(),
        };
    }

    let trimmed = input.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let compact = trim_json_value(&v, 0, 4);
            let text =
                serde_json::to_string_pretty(&compact).unwrap_or_else(|_| trimmed.to_string());
            if text.chars().count() < chars_in && text.chars().count() <= opts.target_chars {
                return CompressResult {
                    chars_in,
                    chars_out: text.chars().count(),
                    text,
                    strategy: "mcp-json".into(),
                };
            }
            let truncated = truncate_to_budget(&text, opts.target_chars, 40);
            return CompressResult {
                chars_in,
                chars_out: truncated.chars().count(),
                text: truncated,
                strategy: "mcp-json-truncated".into(),
            };
        }
    }

    compress_generic(input, opts, ctx, "mcp-text")
}

fn trim_json_value(v: &serde_json::Value, depth: usize, max_array: usize) -> serde_json::Value {
    if depth >= 8 && (v.is_array() || v.is_object()) {
        return serde_json::Value::String("… nested content omitted".into());
    }
    match v {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                if val.is_null() {
                    continue;
                }
                out.insert(k.clone(), trim_json_value(val, depth + 1, max_array));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            if arr.len() <= max_array {
                serde_json::Value::Array(
                    arr.iter()
                        .map(|x| trim_json_value(x, depth + 1, max_array))
                        .collect(),
                )
            } else {
                let mut kept: Vec<serde_json::Value> = arr
                    .iter()
                    .take(max_array)
                    .map(|x| trim_json_value(x, depth + 1, max_array))
                    .collect();
                kept.push(serde_json::json!(format!(
                    "… {} more items",
                    arr.len() - max_array
                )));
                serde_json::Value::Array(kept)
            }
        }
        serde_json::Value::String(s) => {
            if s.chars().count() > 500 {
                serde_json::Value::String(format!("{}…", s.chars().take(497).collect::<String>()))
            } else {
                v.clone()
            }
        }
        _ => v.clone(),
    }
}
