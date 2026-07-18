use std::fmt;

use serde_json::{Map, Value};

use super::types::{CanonicalContentBlock, CanonicalToolResult, PreservedField};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpParseError {
    RootNotObject,
    MissingContent,
    ContentNotArray,
}

impl fmt::Display for McpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootNotObject => f.write_str("MCP tool result is not an object"),
            Self::MissingContent => f.write_str("MCP tool result has no content field"),
            Self::ContentNotArray => f.write_str("MCP tool result content is not an array"),
        }
    }
}

impl std::error::Error for McpParseError {}

/// Parse a protocol `CallToolResult` without dropping unknown top-level fields or content blocks.
pub fn parse_mcp_result(value: &Value) -> Result<CanonicalToolResult, McpParseError> {
    let source = value.as_object().ok_or(McpParseError::RootNotObject)?;
    let content = source
        .get("content")
        .ok_or(McpParseError::MissingContent)?
        .as_array()
        .ok_or(McpParseError::ContentNotArray)?
        .iter()
        .map(parse_content_block)
        .collect();

    let structured_content = match source.get("structuredContent") {
        None => PreservedField::Absent,
        Some(value) if value.is_object() => PreservedField::Value(value.clone()),
        Some(value) => PreservedField::Opaque(value.clone()),
    };
    let is_error = match source.get("isError") {
        None => PreservedField::Absent,
        Some(value) if value.is_boolean() => {
            PreservedField::Value(value.as_bool().expect("boolean checked"))
        }
        Some(value) => PreservedField::Opaque(value.clone()),
    };
    let mut preserved = source.clone();
    preserved.remove("content");
    preserved.remove("structuredContent");
    preserved.remove("isError");

    Ok(CanonicalToolResult::new(
        content,
        structured_content,
        is_error,
        preserved,
        value.clone(),
    ))
}

/// Fail-open adapter boundary: malformed or unsupported values are returned byte-for-byte at the
/// parsed JSON value level instead of being partially rebuilt.
pub fn render_mcp_result_or_original(value: &Value) -> Value {
    parse_mcp_result(value)
        .map(|result| result.render())
        .unwrap_or_else(|_| value.clone())
}

fn parse_content_block(value: &Value) -> CanonicalContentBlock {
    let Some(block) = value.as_object() else {
        return CanonicalContentBlock::Unknown { raw: value.clone() };
    };
    match block.get("type").and_then(Value::as_str) {
        Some("text") => string_field(block, "text")
            .map(|text| CanonicalContentBlock::Text {
                text,
                preserved: block.clone(),
            })
            .unwrap_or_else(|| unknown(value)),
        Some("image") => parse_binary_block(block, value, true),
        Some("audio") => parse_binary_block(block, value, false),
        Some("resource_link") => match (string_field(block, "name"), string_field(block, "uri")) {
            (Some(name), Some(uri)) => CanonicalContentBlock::ResourceLink {
                name,
                uri,
                preserved: block.clone(),
            },
            _ => unknown(value),
        },
        Some("resource") => parse_embedded_resource(block, value),
        _ => unknown(value),
    }
}

fn parse_binary_block(
    block: &Map<String, Value>,
    raw: &Value,
    image: bool,
) -> CanonicalContentBlock {
    match (string_field(block, "data"), string_field(block, "mimeType")) {
        (Some(data), Some(mime_type)) if image => CanonicalContentBlock::Image {
            data,
            mime_type,
            preserved: block.clone(),
        },
        (Some(data), Some(mime_type)) => CanonicalContentBlock::Audio {
            data,
            mime_type,
            preserved: block.clone(),
        },
        _ => unknown(raw),
    }
}

fn parse_embedded_resource(block: &Map<String, Value>, raw: &Value) -> CanonicalContentBlock {
    let Some(resource) = block.get("resource").and_then(Value::as_object) else {
        return unknown(raw);
    };
    let Some(uri) = string_field(resource, "uri") else {
        return unknown(raw);
    };
    if let Some(text) = string_field(resource, "text") {
        return CanonicalContentBlock::EmbeddedTextResource {
            uri,
            text,
            preserved: block.clone(),
        };
    }
    if let Some(blob) = string_field(resource, "blob") {
        return CanonicalContentBlock::EmbeddedBlobResource {
            uri,
            blob,
            preserved: block.clone(),
        };
    }
    unknown(raw)
}

fn string_field(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn unknown(value: &Value) -> CanonicalContentBlock {
    CanonicalContentBlock::Unknown { raw: value.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_values_fail_open_to_the_exact_original() {
        let values = [
            Value::Null,
            json!("plain text"),
            json!([]),
            json!({}),
            json!({"content": "not-an-array", "vendor": 1}),
        ];
        for value in values {
            assert_eq!(render_mcp_result_or_original(&value), value);
        }
    }

    #[test]
    fn invalid_known_blocks_become_opaque_instead_of_being_dropped() {
        let value = json!({
            "content": [
                {"type": "text", "text": 42, "future": true},
                {"type": "image", "data": "abc"},
                7
            ],
            "isError": "future-error-state"
        });
        let result = parse_mcp_result(&value).expect("valid result envelope");
        assert_eq!(result.coverage().unknown_blocks, 3);
        assert_eq!(result.coverage().opaque_contract_fields, 1);
        assert_eq!(result.render(), value);
    }
}
