//! Content-redacting offline capture harness.
//!
//! The raw envelope exists only in memory. Sanitized output keeps protocol structure, bounded
//! shapes, safe header names, and ordinal call/result correlation; it never keeps header values,
//! prompts, tool output, arbitrary JSON keys, URLs, provider hosts, or tool names.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model_gateway::route::RouteTransport;
use crate::surface::SurfaceId;

const MAX_DEPTH: usize = 16;
const MAX_ARRAY_ITEMS: usize = 64;
const MAX_OBJECT_FIELDS: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCapture {
    pub surface: SurfaceId,
    #[serde(default)]
    pub client_version: Option<String>,
    #[serde(default)]
    pub direction: CaptureDirection,
    #[serde(default)]
    pub method: Option<String>,
    pub path: String,
    #[serde(default = "unknown_transport")]
    pub transport: RouteTransport,
    #[serde(default)]
    pub content_encoding: Option<String>,
    #[serde(default)]
    pub headers: Vec<RawHeader>,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub stream_events: Vec<Value>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub restoration: Option<RestorationObservation>,
}

fn unknown_transport() -> RouteTransport {
    RouteTransport::Unknown
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureDirection {
    #[default]
    Request,
    Response,
}

#[derive(Debug, Deserialize)]
pub struct RawHeader {
    pub name: String,
    /// Accepted as arbitrary JSON so duplicate/multi-value capture tools do not have to flatten
    /// values. The sanitizer records only whether a value existed.
    #[serde(default)]
    pub value: Value,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorationObservation {
    pub attempted: bool,
    pub succeeded: bool,
    pub byte_identical: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizedCapture {
    pub schema_version: u32,
    pub surface: SurfaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    pub direction: CaptureDirection,
    pub method: &'static str,
    pub path_template: String,
    pub transport: RouteTransport,
    pub content_encoding: &'static str,
    pub headers: Vec<HeaderReceipt>,
    pub body_shape: JsonShape,
    pub stream_event_shapes: Vec<JsonShape>,
    pub signals: CaptureSignals,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restoration: Option<RestorationObservation>,
    pub content_redacted: bool,
    pub persisted_by_harness: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderReceipt {
    pub name: String,
    pub value_present: bool,
    pub sensitive: bool,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum JsonShape {
    Null,
    Boolean,
    Number,
    String {
        utf8_bytes: usize,
    },
    Array {
        length: usize,
        truncated: bool,
        items: Vec<JsonShape>,
    },
    Object {
        known_fields: BTreeMap<String, JsonShape>,
        unknown_field_count: usize,
        truncated: bool,
    },
    DepthLimit,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSignals {
    pub protocol_item_types: Vec<String>,
    pub tool_call_count: usize,
    pub tool_result_count: usize,
    pub tool_identity_count: usize,
    pub correlations: Vec<ToolCorrelation>,
    pub compaction_markers: Vec<&'static str>,
    pub error_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCorrelation {
    pub correlation_id: String,
    pub call_seen: bool,
    pub result_seen: bool,
}

#[derive(Default)]
struct SignalCollector {
    item_types: BTreeSet<String>,
    correlations: Vec<(String, bool, bool)>,
    correlation_index: HashMap<String, usize>,
    tool_call_count: usize,
    tool_result_count: usize,
    tool_identity_count: usize,
    compaction_markers: BTreeSet<&'static str>,
    error_count: usize,
}

pub fn sanitize(raw: &RawCapture) -> SanitizedCapture {
    let mut signals = SignalCollector::default();
    collect_signals(&raw.body, &mut signals, 0);
    for event in raw.stream_events.iter().take(MAX_ARRAY_ITEMS) {
        collect_signals(event, &mut signals, 0);
    }
    if raw.error.is_some() {
        signals.error_count += 1;
    }

    let headers = raw
        .headers
        .iter()
        .map(|header| {
            let normalized = header.name.trim().to_ascii_lowercase();
            let sensitive = is_sensitive_header(&normalized);
            HeaderReceipt {
                name: sanitize_header_name(&normalized),
                value_present: !header.value.is_null(),
                sensitive,
            }
        })
        .collect();

    let correlations = signals
        .correlations
        .iter()
        .enumerate()
        .map(|(index, (_, call_seen, result_seen))| ToolCorrelation {
            correlation_id: format!("correlation-{}", index + 1),
            call_seen: *call_seen,
            result_seen: *result_seen,
        })
        .collect();

    SanitizedCapture {
        schema_version: 1,
        surface: raw.surface,
        client_version: raw.client_version.as_deref().and_then(sanitize_version),
        direction: raw.direction,
        method: sanitize_method(raw.method.as_deref()),
        path_template: sanitize_path(&raw.path),
        transport: raw.transport,
        content_encoding: sanitize_encoding(raw.content_encoding.as_deref()),
        headers,
        body_shape: shape(&raw.body, 0),
        stream_event_shapes: raw
            .stream_events
            .iter()
            .take(MAX_ARRAY_ITEMS)
            .map(|event| shape(event, 0))
            .collect(),
        signals: CaptureSignals {
            protocol_item_types: signals.item_types.into_iter().collect(),
            tool_call_count: signals.tool_call_count,
            tool_result_count: signals.tool_result_count,
            tool_identity_count: signals.tool_identity_count,
            correlations,
            compaction_markers: signals.compaction_markers.into_iter().collect(),
            error_count: signals.error_count,
        },
        restoration: raw.restoration,
        content_redacted: true,
        persisted_by_harness: false,
    }
}

fn shape(value: &Value, depth: usize) -> JsonShape {
    if depth >= MAX_DEPTH {
        return JsonShape::DepthLimit;
    }
    match value {
        Value::Null => JsonShape::Null,
        Value::Bool(_) => JsonShape::Boolean,
        Value::Number(_) => JsonShape::Number,
        Value::String(value) => JsonShape::String {
            utf8_bytes: value.len(),
        },
        Value::Array(values) => JsonShape::Array {
            length: values.len(),
            truncated: values.len() > MAX_ARRAY_ITEMS,
            items: values
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|value| shape(value, depth + 1))
                .collect(),
        },
        Value::Object(object) => {
            let mut known_fields = BTreeMap::new();
            let mut unknown_field_count = 0;
            for (index, (key, value)) in object.iter().enumerate() {
                if index >= MAX_OBJECT_FIELDS {
                    break;
                }
                if is_known_protocol_field(key) {
                    known_fields.insert(key.clone(), shape(value, depth + 1));
                } else {
                    unknown_field_count += 1;
                }
            }
            JsonShape::Object {
                known_fields,
                unknown_field_count,
                truncated: object.len() > MAX_OBJECT_FIELDS,
            }
        }
    }
}

fn collect_signals(value: &Value, signals: &mut SignalCollector, depth: usize) {
    if depth >= MAX_DEPTH {
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values.iter().take(MAX_ARRAY_ITEMS) {
                collect_signals(value, signals, depth + 1);
            }
        }
        Value::Object(object) => {
            let item_type = object
                .get("type")
                .and_then(Value::as_str)
                .filter(|value| is_safe_item_type(value));
            if let Some(item_type) = item_type {
                signals.item_types.insert(item_type.to_string());
            }
            let is_call = item_type.is_some_and(is_tool_call_type);
            let chat_result = object.get("role").and_then(Value::as_str) == Some("tool")
                && object.get("tool_call_id").and_then(Value::as_str).is_some();
            let is_result = item_type.is_some_and(is_tool_result_type) || chat_result;
            if let Some(calls) = object.get("tool_calls").and_then(Value::as_array) {
                for call in calls.iter().take(MAX_ARRAY_ITEMS) {
                    let Some(call) = call.as_object() else {
                        continue;
                    };
                    signals.item_types.insert("chat-tool-call".into());
                    signals.tool_call_count += 1;
                    if call
                        .get("function")
                        .and_then(Value::as_object)
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .is_some()
                    {
                        signals.tool_identity_count += 1;
                    }
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        record_correlation(signals, id, true, false);
                    }
                }
            }
            if is_call {
                signals.tool_call_count += 1;
                if object.get("name").and_then(Value::as_str).is_some() {
                    signals.tool_identity_count += 1;
                }
            }
            if is_result {
                signals.tool_result_count += 1;
                if chat_result {
                    signals.item_types.insert("chat-tool-result".into());
                }
            }
            if is_call || is_result {
                if let Some(id) = correlation_id(object, is_result) {
                    record_correlation(signals, id, is_call, is_result);
                }
            }
            if matches!(item_type, Some("compaction" | "compaction_summary")) {
                signals
                    .compaction_markers
                    .insert("explicit-compaction-item");
            }
            if matches!(item_type, Some("summary")) {
                signals.compaction_markers.insert("summary-item");
            }
            if matches!(item_type, Some("error")) || object.contains_key("error") {
                signals.error_count += 1;
            }
            for value in object.values().take(MAX_OBJECT_FIELDS) {
                collect_signals(value, signals, depth + 1);
            }
        }
        _ => {}
    }
}

fn correlation_id(object: &serde_json::Map<String, Value>, result: bool) -> Option<&str> {
    let keys: &[&str] = if result {
        &["call_id", "tool_use_id", "tool_call_id", "id"]
    } else {
        &["call_id", "id", "tool_use_id", "tool_call_id"]
    };
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
}

fn record_correlation(signals: &mut SignalCollector, raw_id: &str, call: bool, result: bool) {
    let index = if let Some(index) = signals.correlation_index.get(raw_id) {
        *index
    } else {
        let index = signals.correlations.len();
        signals
            .correlations
            .push((raw_id.to_string(), false, false));
        signals.correlation_index.insert(raw_id.to_string(), index);
        index
    };
    signals.correlations[index].1 |= call;
    signals.correlations[index].2 |= result;
}

fn sanitize_method(method: Option<&str>) -> &'static str {
    match method
        .map(str::trim)
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("GET") => "GET",
        Some("POST") => "POST",
        Some("PUT") => "PUT",
        Some("PATCH") => "PATCH",
        Some("DELETE") => "DELETE",
        Some("OPTIONS") => "OPTIONS",
        Some("HEAD") => "HEAD",
        Some(_) => "OTHER",
        None => "UNKNOWN",
    }
}

fn sanitize_encoding(encoding: Option<&str>) -> &'static str {
    match encoding
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("") | Some("identity") => "identity",
        Some("gzip") => "gzip",
        Some("deflate") => "deflate",
        Some("br") => "br",
        Some("zstd") => "zstd",
        Some(_) => "other",
    }
}

fn sanitize_path(raw: &str) -> String {
    let without_authority = if let Some(scheme) = raw.find("://") {
        let after_scheme = &raw[scheme + 3..];
        after_scheme
            .find('/')
            .map_or("/", |slash| &after_scheme[slash..])
    } else {
        raw
    };
    let path = without_authority
        .split(['?', '#'])
        .next()
        .filter(|value| value.starts_with('/'))
        .unwrap_or("/");
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return "/".into();
    }
    let safe = segments
        .into_iter()
        .map(|segment| match segment {
            "v1" | "responses" | "chat" | "completions" | "messages" | "backend-api" | "codex" => {
                segment
            }
            _ => "{segment}",
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("/{safe}")
}

fn sanitize_header_name(name: &str) -> String {
    match name {
        "accept" | "accept-encoding" | "anthropic-beta" | "anthropic-version" | "authorization"
        | "content-encoding" | "content-length" | "content-type" | "host" | "openai-beta"
        | "user-agent" | "x-api-key" | "x-request-id" => name.to_string(),
        _ => "other-header".into(),
    }
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name,
        "authorization" | "proxy-authorization" | "x-api-key" | "cookie" | "set-cookie"
    ) || name.contains("token")
        || name.contains("secret")
        || name.contains("credential")
}

fn is_known_protocol_field(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "id"
            | "call_id"
            | "tool_use_id"
            | "tool_call_id"
            | "name"
            | "role"
            | "content"
            | "input"
            | "output"
            | "arguments"
            | "messages"
            | "tool_calls"
            | "function"
            | "choices"
            | "delta"
            | "tools"
            | "model"
            | "stream"
            | "input_items"
            | "previous_response_id"
            | "status"
            | "error"
            | "usage"
            | "stop_reason"
            | "stop_sequence"
            | "system"
            | "instructions"
    )
}

fn is_safe_item_type(value: &str) -> bool {
    matches!(
        value,
        "message"
            | "input_text"
            | "output_text"
            | "text"
            | "reasoning"
            | "function_call"
            | "function_call_output"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "computer_call"
            | "computer_call_output"
            | "web_search_call"
            | "mcp_call"
            | "mcp_call_output"
            | "tool_use"
            | "tool_result"
            | "compaction"
            | "compaction_summary"
            | "summary"
            | "error"
    )
}

fn is_tool_call_type(value: &str) -> bool {
    matches!(
        value,
        "function_call"
            | "custom_tool_call"
            | "computer_call"
            | "web_search_call"
            | "mcp_call"
            | "tool_use"
    )
}

fn is_tool_result_type(value: &str) -> bool {
    matches!(
        value,
        "function_call_output"
            | "custom_tool_call_output"
            | "computer_call_output"
            | "mcp_call_output"
            | "tool_result"
    )
}

fn sanitize_version(raw: &str) -> Option<String> {
    raw.split_whitespace()
        .find(|part| {
            part.len() <= 64
                && part.starts_with(|c: char| c.is_ascii_digit())
                && part.contains('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        })
        .map(str::to_string)
        .or_else(|| Some("detected".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_never_keeps_authority_query_or_unknown_segments() {
        assert_eq!(
            sanitize_path("https://secret.example/v1/responses/customer-123?token=secret"),
            "/v1/responses/{segment}"
        );
    }

    #[test]
    fn call_ids_are_replaced_by_ordinals_but_still_correlate() {
        let raw: RawCapture = serde_json::from_value(serde_json::json!({
            "surface": "codex",
            "path": "/v1/responses",
            "body": {"input": [
                {"type": "function_call", "call_id": "sensitive-call-id", "name": "secret_tool"},
                {"type": "function_call_output", "call_id": "sensitive-call-id", "output": "secret output"}
            ]}
        }))
        .unwrap();
        let output = serde_json::to_string(&sanitize(&raw)).unwrap();
        assert!(!output.contains("sensitive-call-id"));
        assert!(!output.contains("secret_tool"));
        assert!(!output.contains("secret output"));
        assert!(output.contains("correlation-1"));
        assert!(output.contains("\"callSeen\":true"));
        assert!(output.contains("\"resultSeen\":true"));
    }
}
