//! Anthropic Messages request adapter. It observes only `tool_use` -> `tool_result` pairs and never
//! rebuilds or mutates the request in M2.

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

const ADAPTER_ID: &str = "anthropic-messages-v1";

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

    let contracts = parse_contracts(root);
    let mut calls = Vec::new();
    let mut results = Vec::new();
    let mut reasons = CorrelationOutcome::default();
    let mut content_items = 0usize;
    let mut position = 0usize;

    for (message_index, message) in messages.iter().enumerate() {
        let Some(message) = message.as_object() else {
            continue;
        };
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        content_items = content_items.saturating_add(content.len());
        if content_items > MAX_PROTOCOL_ITEMS {
            return only_reason(CoverageReason::CorrelationLimitExceeded);
        }
        for (content_index, block) in content.iter().enumerate() {
            position += 1;
            let Some(block) = block.as_object() else {
                continue;
            };
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => match parse_call(position, block, &contracts) {
                    Ok(call) => calls.push(call),
                    Err(reason) => reasons.reason(reason),
                },
                Some("tool_result") => {
                    match parse_result(position, message_index, content_index, block) {
                        Ok(result) => results.push(result),
                        Err(reason) => reasons.reason(reason),
                    }
                }
                _ => {}
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
    block: &Map<String, Value>,
    contracts: &BTreeMap<String, ToolContract>,
) -> Result<PendingCall, CoverageReason> {
    let call_id = bounded_id(block.get("id"))?;
    let tool_name = block
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(CoverageReason::MissingToolName)?;
    let input = block
        .get("input")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or(CoverageReason::InvalidToolInput)?;
    Ok(PendingCall {
        position,
        call_id,
        tool_name: tool_name.to_string(),
        input,
        contract: contracts.get(tool_name).cloned().unwrap_or_default(),
    })
}

fn parse_result(
    position: usize,
    message_index: usize,
    content_index: usize,
    block: &Map<String, Value>,
) -> Result<PendingResult, CoverageReason> {
    let call_id = bounded_id(block.get("tool_use_id"))?;
    let base = vec![
        JsonPathSegment::Field("messages"),
        JsonPathSegment::Index(message_index),
        JsonPathSegment::Field("content"),
        JsonPathSegment::Index(content_index),
        JsonPathSegment::Field("content"),
    ];
    let content = block
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
        Value::Array(items) if !items.is_empty() => {
            let mut leaves = Vec::with_capacity(items.len());
            for (item_index, item) in items.iter().enumerate() {
                let Some(item) = item.as_object() else {
                    return Err(CoverageReason::UnsupportedResultShape);
                };
                if item.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(CoverageReason::UnsupportedResultShape);
                }
                let Some(text) = item.get("text").and_then(Value::as_str) else {
                    return Err(CoverageReason::UnsupportedResultShape);
                };
                let mut path = base.clone();
                path.extend([
                    JsonPathSegment::Index(item_index),
                    JsonPathSegment::Field("text"),
                ]);
                leaves.push(ModelTextLeaf {
                    path,
                    text: text.to_string(),
                });
            }
            ("text-blocks", leaves)
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
        call_id,
        result: CanonicalModelResult {
            source_item_type: "tool_result",
            content_kind,
            text_leaves,
            is_error: block.get("is_error").and_then(Value::as_bool),
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

fn parse_contracts(root: &Map<String, Value>) -> BTreeMap<String, ToolContract> {
    let Some(tools) = root.get("tools").and_then(Value::as_array) else {
        return BTreeMap::new();
    };
    let mut contracts = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for tool in tools.iter().take(MAX_PROTOCOL_ITEMS) {
        let Some(tool) = tool.as_object() else {
            continue;
        };
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
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
                input_schema: object_field(tool, "input_schema"),
                output_schema: object_field(tool, "output_schema"),
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
    use crate::compress::CompressKind;
    use crate::config::Config;

    fn body(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn correlates_parallel_calls_and_preserves_exact_result_paths() {
        let observed = inspect(
            "claude-code",
            &body(json!({
                "tools": [{"name":"Read","input_schema":{"type":"object"}}],
                "messages": [
                    {"role":"assistant","content":[
                        {"type":"tool_use","id":"one","name":"Read","input":{"file_path":"a.rs"}},
                        {"type":"tool_use","id":"two","name":"Bash","input":{"command":"cargo test"}}
                    ]},
                    {"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"two","content":"tests passed"},
                        {"type":"tool_result","tool_use_id":"one","content":[{"type":"text","text":"line 1"},{"type":"text","text":"line 2"}]}
                    ]}
                ]
            })),
        );
        assert!(observed.reasons.is_empty());
        assert_eq!(observed.exchanges.len(), 2);
        assert_eq!(observed.exchanges[0].identity.tool, "Bash");
        assert_eq!(observed.exchanges[1].identity.tool, "Read");
        assert_eq!(
            observed.exchanges[1].result.combined_text(),
            "line 1\nline 2"
        );
        assert_eq!(
            observed.exchanges[1].result.text_leaves[1].path,
            vec![
                JsonPathSegment::Field("messages"),
                JsonPathSegment::Index(1),
                JsonPathSegment::Field("content"),
                JsonPathSegment::Index(1),
                JsonPathSegment::Field("content"),
                JsonPathSegment::Index(1),
                JsonPathSegment::Field("text"),
            ]
        );
        assert!(observed.exchanges[1].contract.input_schema.is_present());

        let cfg = Config {
            compress_target_chars: 4,
            compress_max_output_chars: 1024,
            compress_shadow_enabled: true,
            ..Default::default()
        };
        let decision = crate::compress::compute_shadow_decision(
            &observed.exchanges[0].identity.tool,
            &observed.exchanges[0].input,
            &observed.exchanges[0].result.combined_text(),
            &cfg,
            None,
            "",
        )
        .unwrap();
        assert_eq!(decision.kind, CompressKind::TestRunner);
    }

    #[test]
    fn foreign_and_ambiguous_shapes_are_reasoned_not_correlated() {
        let foreign = inspect(
            "claude-code",
            &body(
                json!({"input":[{"type":"function_call_output","call_id":"x","output":"secret"}]}),
            ),
        );
        assert_eq!(foreign.reasons[&CoverageReason::ProtocolShapeMismatch], 1);

        let ambiguous = inspect(
            "claude-code",
            &body(json!({"messages":[
                {"role":"assistant","content":[
                    {"type":"tool_use","id":"same","name":"Read","input":{}},
                    {"type":"tool_use","id":"same","name":"Bash","input":{}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"same","content":"secret"}
                ]}
            ]})),
        );
        assert!(ambiguous.exchanges.is_empty());
        assert_eq!(ambiguous.reasons[&CoverageReason::DuplicateToolCall], 1);
    }

    #[test]
    fn multimodal_results_and_excessive_ids_fail_closed() {
        let mixed = inspect(
            "claude-code",
            &body(json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"one","name":"Read","input":{}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"one","content":[
                    {"type":"text","text":"secret"}, {"type":"image","source":{}}
                ]}]}
            ]})),
        );
        assert!(mixed.exchanges.is_empty());
        assert_eq!(mixed.reasons[&CoverageReason::UnsupportedResultShape], 1);

        let long_id = "x".repeat(MAX_CALL_ID_BYTES + 1);
        let oversized_id = inspect(
            "claude-code",
            &body(json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":long_id,"name":"Read","input":{}}]}
            ]})),
        );
        assert_eq!(oversized_id.reasons[&CoverageReason::CallIdTooLong], 1);
    }
}
