//! OpenAI Responses request adapter. Protocol item families have separate correlation scopes so a
//! matching string ID cannot cross function, custom, local-shell, or mutation contracts.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::tool_result::{PreservedField, ToolContract};

use super::{MAX_MODEL_RESULT_CHARS, MAX_PROTOCOL_ITEMS};
use crate::model_gateway::canonical::{
    CanonicalModelResult, JsonPathSegment, ModelTextLeaf, PendingCall, PendingResult,
};
use crate::model_gateway::correlate::{
    correlate, CorrelationOutcome, CoverageReason, MAX_CALL_ID_BYTES,
};

const ADAPTER_ID: &str = "openai-responses-v1";

pub(super) fn inspect(platform: &str, body: &[u8]) -> CorrelationOutcome {
    let root: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return only_reason(CoverageReason::InvalidJson),
    };
    let Some(root) = root.as_object() else {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    };
    let Some(input) = root.get("input") else {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    };
    if input.is_string() {
        return CorrelationOutcome::default();
    }
    let Some(items) = input.as_array() else {
        return only_reason(CoverageReason::ProtocolShapeMismatch);
    };
    if items.len() > MAX_PROTOCOL_ITEMS {
        return only_reason(CoverageReason::CorrelationLimitExceeded);
    }

    let contracts = parse_contracts(root);
    let mut calls = Vec::new();
    let mut results = Vec::new();
    let mut reasons = CorrelationOutcome::default();
    for (position, item) in items.iter().enumerate() {
        let Some(item) = item.as_object() else {
            continue;
        };
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => match parse_function_call(position, item, &contracts) {
                Ok(call) => calls.push(call),
                Err(reason) => reasons.reason(reason),
            },
            Some("custom_tool_call") => match parse_custom_call(position, item, &contracts) {
                Ok(call) => calls.push(call),
                Err(reason) => reasons.reason(reason),
            },
            Some("local_shell_call") => match parse_shell_call(position, item) {
                Ok(call) => calls.push(call),
                Err(reason) => reasons.reason(reason),
            },
            Some("function_call_output") => {
                match parse_text_result(position, item, "function", "function_call_output") {
                    Ok(result) => results.push(result),
                    Err(reason) => reasons.reason(reason),
                }
            }
            Some("custom_tool_call_output") => {
                match parse_text_result(position, item, "custom", "custom_tool_call_output") {
                    Ok(result) => results.push(result),
                    Err(reason) => reasons.reason(reason),
                }
            }
            Some("local_shell_call_output") => {
                match parse_text_result(position, item, "local-shell", "local_shell_call_output") {
                    Ok(result) => results.push(result),
                    Err(reason) => reasons.reason(reason),
                }
            }
            Some("apply_patch_call" | "apply_patch_call_output") => {
                reasons.reason(CoverageReason::MutationToolHeld)
            }
            _ => {}
        }
    }

    let correlated = correlate(platform, ADAPTER_ID, calls, results);
    reasons.exchanges = correlated.exchanges;
    for (reason, count) in correlated.reasons {
        *reasons.reasons.entry(reason).or_default() += count;
    }
    reasons
}

fn parse_function_call(
    position: usize,
    item: &Map<String, Value>,
    contracts: &BTreeMap<String, ToolContract>,
) -> Result<PendingCall, CoverageReason> {
    let call_id = bounded_id(item.get("call_id"))?;
    let tool_name = required_name(item.get("name"))?;
    let arguments = item
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or(CoverageReason::InvalidToolInput)?;
    let input: Value = serde_json::from_str(arguments)
        .ok()
        .filter(Value::is_object)
        .ok_or(CoverageReason::InvalidToolInput)?;
    Ok(PendingCall {
        position,
        correlation_scope: "function",
        call_id,
        tool_name: tool_name.to_string(),
        input,
        contract: contracts.get(tool_name).cloned().unwrap_or_default(),
    })
}

fn parse_custom_call(
    position: usize,
    item: &Map<String, Value>,
    contracts: &BTreeMap<String, ToolContract>,
) -> Result<PendingCall, CoverageReason> {
    let call_id = bounded_id(item.get("call_id"))?;
    let tool_name = required_name(item.get("name"))?;
    let input = item
        .get("input")
        .and_then(Value::as_str)
        .ok_or(CoverageReason::InvalidToolInput)?;
    Ok(PendingCall {
        position,
        correlation_scope: "custom",
        call_id,
        tool_name: tool_name.to_string(),
        input: json!({"input": input}),
        contract: contracts.get(tool_name).cloned().unwrap_or_default(),
    })
}

fn parse_shell_call(
    position: usize,
    item: &Map<String, Value>,
) -> Result<PendingCall, CoverageReason> {
    let call_id = bounded_id(item.get("call_id"))?;
    let action = item
        .get("action")
        .and_then(Value::as_object)
        .ok_or(CoverageReason::InvalidToolInput)?;
    let command = match action.get("command") {
        Some(Value::String(command)) => Some(command.clone()),
        Some(Value::Array(commands)) => commands
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|commands| commands.join("\n")),
        _ => None,
    }
    .ok_or(CoverageReason::InvalidToolInput)?;
    let mut input = action.clone();
    input.insert("command".into(), Value::String(command));
    Ok(PendingCall {
        position,
        correlation_scope: "local-shell",
        call_id,
        tool_name: "Shell".into(),
        input: Value::Object(input),
        contract: ToolContract {
            protocol_version: Some(ADAPTER_ID.into()),
            ..Default::default()
        },
    })
}

fn parse_text_result(
    position: usize,
    item: &Map<String, Value>,
    correlation_scope: &'static str,
    source_item_type: &'static str,
) -> Result<PendingResult, CoverageReason> {
    let call_id = bounded_id(item.get("call_id"))?;
    let text = item
        .get("output")
        .and_then(Value::as_str)
        .ok_or(CoverageReason::UnsupportedResultShape)?;
    if text.chars().count() > MAX_MODEL_RESULT_CHARS {
        return Err(CoverageReason::ResultTooLarge);
    }
    let already_shortened = text.contains("[ctx trimmed this output to save context.");
    let canonical_mcp = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| crate::tool_result::parse_mcp_result(&value).ok());
    Ok(PendingResult {
        position,
        correlation_scope,
        call_id,
        result: CanonicalModelResult {
            source_item_type,
            content_kind: "text",
            text_leaves: vec![ModelTextLeaf {
                path: vec![
                    JsonPathSegment::Field("input"),
                    JsonPathSegment::Index(position),
                    JsonPathSegment::Field("output"),
                ],
                text: text.to_string(),
            }],
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

fn required_name(value: Option<&Value>) -> Result<&str, CoverageReason> {
    value
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(CoverageReason::MissingToolName)
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
                input_schema: object_field(tool, "parameters"),
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
    use crate::compress::CompressKind;

    fn body(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn correlates_function_custom_and_local_shell_families() {
        let observed = inspect(
            "codex",
            &body(json!({
                "tools":[{"type":"function","name":"Read","parameters":{"type":"object"}}],
                "input":[
                    {"type":"function_call","call_id":"fn","name":"Read","arguments":"{\"file_path\":\"a.rs\"}"},
                    {"type":"function_call_output","call_id":"fn","output":"read output"},
                    {"type":"custom_tool_call","call_id":"custom","name":"search","input":"needle"},
                    {"type":"custom_tool_call_output","call_id":"custom","output":"search output"},
                    {"type":"local_shell_call","call_id":"shell","action":{"command":["cargo test","echo done"],"timeout_ms":1000}},
                    {"type":"local_shell_call_output","call_id":"shell","output":"test output"}
                ]
            })),
        );
        assert!(observed.reasons.is_empty());
        assert_eq!(observed.exchanges.len(), 3);
        assert_eq!(observed.exchanges[0].identity.tool, "Read");
        assert!(observed.exchanges[0].contract.input_schema.is_present());
        assert_eq!(observed.exchanges[1].input, json!({"input":"needle"}));
        assert_eq!(observed.exchanges[2].identity.tool, "Shell");
        assert_eq!(
            observed.exchanges[2].input["command"],
            "cargo test\necho done"
        );
        assert_eq!(
            observed.exchanges[2].result.text_leaves[0].path,
            vec![
                JsonPathSegment::Field("input"),
                JsonPathSegment::Index(5),
                JsonPathSegment::Field("output"),
            ]
        );
        let cfg = crate::config::Config {
            compress_target_chars: 4,
            compress_max_output_chars: 1024,
            ..Default::default()
        };
        let decision = crate::compress::compute_shadow_decision(
            &observed.exchanges[2].identity.tool,
            &observed.exchanges[2].input,
            &observed.exchanges[2].result.combined_text(),
            &cfg,
            None,
            "",
        )
        .unwrap();
        assert_eq!(decision.kind, CompressKind::TestRunner);
    }

    #[test]
    fn item_families_cannot_cross_correlate_and_order_is_enforced() {
        let wrong_family = inspect(
            "codex",
            &body(json!({"input":[
                {"type":"function_call","call_id":"same","name":"Read","arguments":"{}"},
                {"type":"custom_tool_call_output","call_id":"same","output":"secret"}
            ]})),
        );
        assert!(wrong_family.exchanges.is_empty());
        assert_eq!(wrong_family.reasons[&CoverageReason::MissingToolCall], 1);

        let reversed = inspect(
            "codex",
            &body(json!({"input":[
                {"type":"function_call_output","call_id":"same","output":"secret"},
                {"type":"function_call","call_id":"same","name":"Read","arguments":"{}"}
            ]})),
        );
        assert!(reversed.exchanges.is_empty());
        assert_eq!(reversed.reasons[&CoverageReason::ResultPrecedesToolCall], 1);
    }

    #[test]
    fn mutation_and_unsupported_shapes_are_held_with_reasons() {
        let held = inspect(
            "codex",
            &body(json!({"input":[
                {"type":"apply_patch_call","call_id":"patch","operation":{}},
                {"type":"apply_patch_call_output","call_id":"patch","output":"done"},
                {"type":"function_call","call_id":"bad","name":"Read","arguments":"not json"},
                {"type":"function_call_output","call_id":"bad","output":[{"type":"text","text":"secret"}]}
            ]})),
        );
        assert!(held.exchanges.is_empty());
        assert_eq!(held.reasons[&CoverageReason::MutationToolHeld], 2);
        assert_eq!(held.reasons[&CoverageReason::InvalidToolInput], 1);
        assert_eq!(held.reasons[&CoverageReason::UnsupportedResultShape], 1);
    }

    #[test]
    fn chat_and_anthropic_bodies_do_not_match_responses() {
        for foreign in [
            json!({"messages":[{"role":"tool","tool_call_id":"x","content":"secret"}]}),
            json!({"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"secret"}]}]}),
        ] {
            let observed = inspect("codex", &body(foreign));
            assert!(observed.exchanges.is_empty());
            assert_eq!(observed.reasons[&CoverageReason::ProtocolShapeMismatch], 1);
        }
    }
}
