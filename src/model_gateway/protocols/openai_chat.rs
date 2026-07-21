//! OpenAI Chat Completions request adapter. It accepts only assistant function tool calls and
//! `role: tool` results; Anthropic content blocks and Responses items are not reinterpreted.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::tool_result::{PreservedField, ToolContract};

use super::{MAX_MODEL_RESULT_CHARS, MAX_PROTOCOL_ITEMS};
use crate::model_gateway::canonical::{
    CanonicalModelResult, JsonPathSegment, ModelTextLeaf, PendingCall, PendingResult,
};
use crate::model_gateway::correlate::{
    correlate, CorrelationOutcome, CoverageReason, MAX_CALL_ID_BYTES,
};

const ADAPTER_ID: &str = "openai-chat-completions-v1";

pub(super) fn inspect(platform: &str, body: &[u8]) -> CorrelationOutcome {
    let root: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return only_reason(CoverageReason::InvalidJson),
    };
    let Some(root) = root.as_object() else {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    };
    let Some(messages) = root.get("messages").and_then(Value::as_array) else {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    };
    if messages.len() > MAX_PROTOCOL_ITEMS {
        return only_reason(CoverageReason::CorrelationLimitExceeded);
    }
    if contains_anthropic_tool_blocks(messages) {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    }

    let contracts = parse_contracts(root);
    let mut calls = Vec::new();
    let mut results = Vec::new();
    let mut reasons = CorrelationOutcome::default();
    let mut position = 0usize;
    let mut protocol_items = messages.len();

    for (message_index, message) in messages.iter().enumerate() {
        position += 1;
        let Some(message) = message.as_object() else {
            continue;
        };
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            protocol_items = protocol_items.saturating_add(tool_calls.len());
            if protocol_items > MAX_PROTOCOL_ITEMS {
                return only_reason(CoverageReason::CorrelationLimitExceeded);
            }
            for tool_call in tool_calls {
                position += 1;
                let Some(tool_call) = tool_call.as_object() else {
                    reasons.reason(CoverageReason::InvalidToolInput);
                    continue;
                };
                match parse_call(position, tool_call, &contracts) {
                    Ok(call) => calls.push(call),
                    Err(reason) => reasons.reason(reason),
                }
            }
        }
        if message.get("role").and_then(Value::as_str) == Some("tool") {
            match parse_result(position, message_index, message) {
                Ok(result) => results.push(result),
                Err(reason) => reasons.reason(reason),
            }
        }
    }

    let correlated = correlate(platform, ADAPTER_ID, calls, results);
    reasons.exchanges = correlated.exchanges;
    for (reason, count) in correlated.reasons {
        *reasons.reasons.entry(reason).or_default() += count;
    }
    reasons
}

fn parse_call(
    position: usize,
    call: &Map<String, Value>,
    contracts: &BTreeMap<String, ToolContract>,
) -> Result<PendingCall, CoverageReason> {
    if call.get("type").and_then(Value::as_str) != Some("function") {
        return Err(CoverageReason::InvalidToolInput);
    }
    let call_id = bounded_id(call.get("id"))?;
    let function = call
        .get("function")
        .and_then(Value::as_object)
        .ok_or(CoverageReason::InvalidToolInput)?;
    let tool_name = function
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(CoverageReason::MissingToolName)?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or(CoverageReason::InvalidToolInput)?;
    let input: Value = serde_json::from_str(arguments)
        .ok()
        .filter(Value::is_object)
        .ok_or(CoverageReason::InvalidToolInput)?;
    Ok(PendingCall {
        position,
        correlation_scope: "chat-function",
        call_id,
        tool_name: tool_name.to_string(),
        input,
        contract: contracts.get(tool_name).cloned().unwrap_or_default(),
    })
}

fn parse_result(
    position: usize,
    message_index: usize,
    message: &Map<String, Value>,
) -> Result<PendingResult, CoverageReason> {
    let call_id = bounded_id(message.get("tool_call_id"))?;
    let base = vec![
        JsonPathSegment::Field("messages"),
        JsonPathSegment::Index(message_index),
        JsonPathSegment::Field("content"),
    ];
    let content = message
        .get("content")
        .ok_or(CoverageReason::UnsupportedResultShape)?;
    let (content_kind, text_leaves) = match content {
        Value::String(text) => (
            "text",
            vec![ModelTextLeaf {
                path: base,
                text: text.clone(),
            }],
        ),
        Value::Array(parts) if !parts.is_empty() => {
            let mut leaves = Vec::with_capacity(parts.len());
            for (part_index, part) in parts.iter().enumerate() {
                let Some(part) = part.as_object() else {
                    return Err(CoverageReason::UnsupportedResultShape);
                };
                if part.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(CoverageReason::UnsupportedResultShape);
                }
                let Some(text) = part.get("text").and_then(Value::as_str) else {
                    return Err(CoverageReason::UnsupportedResultShape);
                };
                let mut path = base.clone();
                path.extend([
                    JsonPathSegment::Index(part_index),
                    JsonPathSegment::Field("text"),
                ]);
                leaves.push(ModelTextLeaf {
                    path,
                    text: text.to_string(),
                });
            }
            ("text-parts", leaves)
        }
        _ => return Err(CoverageReason::UnsupportedResultShape),
    };
    let chars = text_leaves
        .iter()
        .map(|leaf| leaf.text.chars().count())
        .sum::<usize>();
    if chars > MAX_MODEL_RESULT_CHARS {
        return Err(CoverageReason::ResultTooLarge);
    }
    let combined = text_leaves
        .iter()
        .map(|leaf| leaf.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let already_shortened = combined.contains("[ctx trimmed this output to save context.");
    let canonical_mcp = serde_json::from_str::<Value>(&combined)
        .ok()
        .and_then(|value| crate::tool_result::parse_mcp_result(&value).ok());
    Ok(PendingResult {
        position,
        correlation_scope: "chat-function",
        call_id,
        result: CanonicalModelResult {
            source_item_type: "chat-tool-result",
            content_kind,
            text_leaves,
            is_error: None,
            already_shortened,
            canonical_mcp,
        },
    })
}

fn bounded_id(value: Option<&Value>) -> Result<String, CoverageReason> {
    let id = value
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(CoverageReason::MissingCallId)?;
    if id.len() > MAX_CALL_ID_BYTES {
        return Err(CoverageReason::CallIdTooLong);
    }
    Ok(id.to_string())
}

fn contains_anthropic_tool_blocks(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content.iter().any(|block| {
                    matches!(
                        block.get("type").and_then(Value::as_str),
                        Some("tool_use" | "tool_result")
                    )
                })
            })
    })
}

fn parse_contracts(root: &Map<String, Value>) -> BTreeMap<String, ToolContract> {
    let Some(tools) = root.get("tools").and_then(Value::as_array) else {
        return BTreeMap::new();
    };
    let mut contracts = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for tool in tools.iter().take(MAX_PROTOCOL_ITEMS) {
        let Some(function) = tool
            .as_object()
            .filter(|tool| tool.get("type").and_then(Value::as_str) == Some("function"))
            .and_then(|tool| tool.get("function"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        let Some(name) = function.get("name").and_then(Value::as_str) else {
            continue;
        };
        if contracts.contains_key(name) {
            duplicates.insert(name.to_string());
            continue;
        }
        contracts.insert(
            name.to_string(),
            ToolContract {
                protocol_version: Some(ADAPTER_ID.into()),
                input_schema: object_field(function, "parameters"),
                output_schema: PreservedField::Absent,
                annotations: PreservedField::Absent,
                preserved: Map::new(),
            },
        );
    }
    for duplicate in duplicates {
        contracts.remove(&duplicate);
    }
    contracts
}

fn object_field(source: &Map<String, Value>, key: &str) -> PreservedField<Value> {
    match source.get(key) {
        None => PreservedField::Absent,
        Some(value) if value.is_object() => PreservedField::Value(value.clone()),
        Some(value) => PreservedField::Opaque(value.clone()),
    }
}

fn only_reason(reason: CoverageReason) -> CorrelationOutcome {
    let mut outcome = CorrelationOutcome::default();
    outcome.reason(reason);
    outcome
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn body(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn correlates_parallel_chat_calls_and_exact_text_part_paths() {
        let observed = inspect(
            "cursor",
            &body(json!({
                "tools":[{"type":"function","function":{"name":"Read","parameters":{"type":"object"}}}],
                "messages":[
                    {"role":"assistant","tool_calls":[
                        {"id":"read","type":"function","function":{"name":"Read","arguments":"{\"file_path\":\"a.rs\"}"}},
                        {"id":"shell","type":"function","function":{"name":"Shell","arguments":"{\"command\":\"cargo test\"}"}}
                    ]},
                    {"role":"tool","tool_call_id":"shell","content":"tests passed"},
                    {"role":"tool","tool_call_id":"read","content":[
                        {"type":"text","text":"line 1"},{"type":"text","text":"line 2"}
                    ]}
                ]
            })),
        );
        assert!(observed.reasons.is_empty());
        assert_eq!(observed.exchanges.len(), 2);
        assert_eq!(observed.exchanges[0].identity.tool, "Shell");
        assert_eq!(observed.exchanges[1].identity.tool, "Read");
        assert!(observed.exchanges[1].contract.input_schema.is_present());
        assert_eq!(
            observed.exchanges[1].result.text_leaves[1].path,
            vec![
                JsonPathSegment::Field("messages"),
                JsonPathSegment::Index(2),
                JsonPathSegment::Field("content"),
                JsonPathSegment::Index(1),
                JsonPathSegment::Field("text"),
            ]
        );
    }

    #[test]
    fn responses_and_anthropic_tool_shapes_do_not_activate_chat() {
        let responses = inspect(
            "cursor",
            &body(
                json!({"input":[{"type":"function_call_output","call_id":"x","output":"secret"}]}),
            ),
        );
        assert_eq!(responses.reasons[&CoverageReason::ProtocolShapeMismatch], 1);

        let anthropic = inspect(
            "cursor",
            &body(json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"x","name":"Read","input":{}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"secret"}]}
            ]})),
        );
        assert!(anthropic.exchanges.is_empty());
        assert_eq!(anthropic.reasons[&CoverageReason::ProtocolShapeMismatch], 1);
    }

    #[test]
    fn malformed_calls_mixed_results_and_duplicates_are_reasoned() {
        let observed = inspect(
            "cursor",
            &body(json!({"messages":[
                {"role":"assistant","tool_calls":[
                    {"id":"bad","type":"function","function":{"name":"Read","arguments":"not json"}},
                    {"id":"same","type":"function","function":{"name":"Read","arguments":"{}"}},
                    {"id":"same","type":"function","function":{"name":"Shell","arguments":"{}"}}
                ]},
                {"role":"tool","tool_call_id":"bad","content":[{"type":"image_url","image_url":{"url":"x"}}]},
                {"role":"tool","tool_call_id":"same","content":"secret"}
            ]})),
        );
        assert!(observed.exchanges.is_empty());
        assert_eq!(observed.reasons[&CoverageReason::InvalidToolInput], 1);
        assert_eq!(observed.reasons[&CoverageReason::UnsupportedResultShape], 1);
        assert_eq!(observed.reasons[&CoverageReason::DuplicateToolCall], 1);
    }
}
