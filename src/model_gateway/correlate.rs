//! Bounded, request-local tool call/result correlation.

use std::collections::BTreeMap;

use crate::tool_result::{CanonicalToolExchange, ToolIdentity, ToolProvenance, ToolTransport};

use super::canonical::{CanonicalModelExchange, PendingCall, PendingResult};

pub const MAX_CORRELATED_CALLS: usize = 512;
pub const MAX_CALL_ID_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoverageReason {
    InvalidJson,
    ProtocolShapeMismatch,
    CorrelationLimitExceeded,
    MissingCallId,
    CallIdTooLong,
    MissingToolName,
    InvalidToolInput,
    MissingToolCall,
    ResultPrecedesToolCall,
    DuplicateToolCall,
    DuplicateToolResult,
    UnsupportedResultShape,
    MutationToolHeld,
    ResultTooLarge,
    AlreadyShortened,
    UnsupportedContentEncoding,
}

impl CoverageReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid-json",
            Self::ProtocolShapeMismatch => "protocol-shape-mismatch",
            Self::CorrelationLimitExceeded => "correlation-limit-exceeded",
            Self::MissingCallId => "missing-call-id",
            Self::CallIdTooLong => "call-id-too-long",
            Self::MissingToolName => "missing-tool-name",
            Self::InvalidToolInput => "invalid-tool-input",
            Self::MissingToolCall => "missing-tool-call",
            Self::ResultPrecedesToolCall => "result-precedes-tool-call",
            Self::DuplicateToolCall => "duplicate-tool-call",
            Self::DuplicateToolResult => "duplicate-tool-result",
            Self::UnsupportedResultShape => "unsupported-result-shape",
            Self::MutationToolHeld => "mutation-tool-held",
            Self::ResultTooLarge => "result-too-large",
            Self::AlreadyShortened => "already-shortened",
            Self::UnsupportedContentEncoding => "unsupported-content-encoding",
        }
    }
}

#[derive(Debug, Default)]
pub struct CorrelationOutcome {
    pub exchanges: Vec<CanonicalModelExchange>,
    pub reasons: BTreeMap<CoverageReason, usize>,
}

impl CorrelationOutcome {
    pub fn reason(&mut self, reason: CoverageReason) {
        *self.reasons.entry(reason).or_default() += 1;
    }
}

pub(super) fn correlate(
    platform: &str,
    adapter: &str,
    calls: Vec<PendingCall>,
    results: Vec<PendingResult>,
) -> CorrelationOutcome {
    let mut outcome = CorrelationOutcome::default();
    if calls.len() > MAX_CORRELATED_CALLS || results.len() > MAX_CORRELATED_CALLS {
        outcome.reason(CoverageReason::CorrelationLimitExceeded);
        return outcome;
    }

    let mut calls_by_id: BTreeMap<(&str, &str), Vec<&PendingCall>> = BTreeMap::new();
    for call in &calls {
        calls_by_id
            .entry((call.correlation_scope, &call.call_id))
            .or_default()
            .push(call);
    }
    let mut result_counts: BTreeMap<(&str, String), usize> = BTreeMap::new();
    for result in &results {
        *result_counts
            .entry((result.correlation_scope, result.call_id.clone()))
            .or_default() += 1;
    }

    for result in results {
        let key = (result.correlation_scope, result.call_id.as_str());
        let Some(matches) = calls_by_id.get(&key) else {
            outcome.reason(CoverageReason::MissingToolCall);
            continue;
        };
        if matches.len() != 1 {
            outcome.reason(CoverageReason::DuplicateToolCall);
            continue;
        }
        if result_counts
            .get(&(result.correlation_scope, result.call_id.clone()))
            .copied()
            != Some(1)
        {
            outcome.reason(CoverageReason::DuplicateToolResult);
            continue;
        }
        let call = matches[0];
        if call.position >= result.position {
            outcome.reason(CoverageReason::ResultPrecedesToolCall);
            continue;
        }
        outcome.exchanges.push(CanonicalToolExchange {
            identity: ToolIdentity {
                platform: platform.to_string(),
                server: crate::compress::shadow::server_prefix_of(&call.tool_name),
                tool: call.tool_name.clone(),
                call_id: Some(call.call_id.clone()),
            },
            transport: ToolTransport::ModelGateway,
            input: call.input.clone(),
            contract: call.contract.clone(),
            result: result.result,
            provenance: ToolProvenance {
                adapter: Some(adapter.to_string()),
                verification: Some("shadow-only".into()),
                ..Default::default()
            },
        });
    }
    outcome
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model_gateway::canonical::{CanonicalModelResult, ModelTextLeaf};
    use crate::tool_result::ToolContract;

    fn call(id: &str, name: &str) -> PendingCall {
        PendingCall {
            position: 0,
            correlation_scope: "test",
            call_id: id.into(),
            tool_name: name.into(),
            input: json!({}),
            contract: ToolContract::default(),
        }
    }

    fn result(id: &str) -> PendingResult {
        PendingResult {
            position: 1,
            correlation_scope: "test",
            call_id: id.into(),
            result: CanonicalModelResult {
                source_item_type: "test-result",
                content_kind: "text",
                text_leaves: vec![ModelTextLeaf {
                    path: vec![],
                    text: "output".into(),
                }],
                is_error: None,
                already_shortened: false,
                canonical_mcp: None,
            },
        }
    }

    #[test]
    fn duplicate_and_missing_ids_never_correlate() {
        let duplicate_call = correlate(
            "test",
            "test",
            vec![call("same", "Read"), call("same", "Shell")],
            vec![result("same")],
        );
        assert!(duplicate_call.exchanges.is_empty());
        assert_eq!(
            duplicate_call.reasons[&CoverageReason::DuplicateToolCall],
            1
        );

        let duplicate_result = correlate(
            "test",
            "test",
            vec![call("same", "Read")],
            vec![result("same"), result("same")],
        );
        assert!(duplicate_result.exchanges.is_empty());
        assert_eq!(
            duplicate_result.reasons[&CoverageReason::DuplicateToolResult],
            2
        );

        let missing = correlate("test", "test", vec![], vec![result("unknown")]);
        assert!(missing.exchanges.is_empty());
        assert_eq!(missing.reasons[&CoverageReason::MissingToolCall], 1);

        let mut late_call = call("late", "Read");
        late_call.position = 2;
        let precedes = correlate("test", "test", vec![late_call], vec![result("late")]);
        assert!(precedes.exchanges.is_empty());
        assert_eq!(precedes.reasons[&CoverageReason::ResultPrecedesToolCall], 1);
    }
}
