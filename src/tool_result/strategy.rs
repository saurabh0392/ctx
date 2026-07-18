use std::collections::HashSet;

use serde_json::Value;

use super::{parse_mcp_result, CanonicalContentBlock, CanonicalMcpResult, PreservedField};

/// Stable, inspectable contract for one result strategy. A version change deliberately creates a
/// new evidence identity instead of silently inheriting activation from older behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpStrategyManifest {
    pub id: &'static str,
    pub version: &'static str,
    pub eligible_shape: &'static str,
    pub invariants: &'static [&'static str],
    /// Per-target expansion ceiling expressed as a percentage of the source text. The first text
    /// strategy uses 100, so no individual block may grow even if the proposal saves overall.
    pub max_expansion_percent: u16,
}

/// One proposed replacement of a plain MCP text content block. Keeping the expected source text
/// makes a proposal stale-safe; proposals are transient and are never persisted as telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTextReplacement {
    pub block_index: usize,
    pub expected_text: String,
    pub replacement: String,
}

/// A contentful, in-memory proposal. T2 does not expose a renderer or an apply operation from this
/// type: the proposal exists only to exercise the validator and record content-free shadow proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTransformProposal {
    pub strategy_id: &'static str,
    pub strategy_version: &'static str,
    pub max_total_text_chars: usize,
    pub replacements: Vec<McpTextReplacement>,
}

/// Content-free proof emitted after an entire proposal passes structural invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedMcpProposal {
    pub replacements: usize,
    pub chars_in: usize,
    pub chars_out: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpProposalRejection {
    StrategyIdentityMismatch,
    SourceRoundTripMismatch,
    ErrorResult,
    OpaqueErrorState,
    EmptyProposal,
    DuplicateTarget,
    TargetOutOfBounds,
    TargetIsNotPlainText,
    StaleSourceText,
    ExpansionLimitExceeded,
    OutputBudgetExceeded,
    NoSavings,
    TopLevelInvariantFailed,
    ContentLengthInvariantFailed,
    NonTargetBlockChanged,
    TargetEnvelopeChanged,
    RenderedContractInvalid,
}

impl McpProposalRejection {
    pub fn code(self) -> &'static str {
        match self {
            Self::StrategyIdentityMismatch => "strategy-identity-mismatch",
            Self::SourceRoundTripMismatch => "source-round-trip-mismatch",
            Self::ErrorResult => "error-result",
            Self::OpaqueErrorState => "opaque-error-state",
            Self::EmptyProposal => "empty-proposal",
            Self::DuplicateTarget => "duplicate-target",
            Self::TargetOutOfBounds => "target-out-of-bounds",
            Self::TargetIsNotPlainText => "target-is-not-plain-text",
            Self::StaleSourceText => "stale-source-text",
            Self::ExpansionLimitExceeded => "expansion-limit-exceeded",
            Self::OutputBudgetExceeded => "output-budget-exceeded",
            Self::NoSavings => "no-savings",
            Self::TopLevelInvariantFailed => "top-level-invariant-failed",
            Self::ContentLengthInvariantFailed => "content-length-invariant-failed",
            Self::NonTargetBlockChanged => "non-target-block-changed",
            Self::TargetEnvelopeChanged => "target-envelope-changed",
            Self::RenderedContractInvalid => "rendered-contract-invalid",
        }
    }
}

/// Validate a text-block proposal without returning an apply-ready value. The candidate is rebuilt
/// only inside this boundary, checked against the raw source, and then dropped. That keeps T2
/// observation-only while proving the invariants the later atomic apply path will depend on.
pub fn validate_mcp_proposal(
    result: &CanonicalMcpResult,
    manifest: &McpStrategyManifest,
    proposal: &McpTransformProposal,
) -> Result<ValidatedMcpProposal, McpProposalRejection> {
    if proposal.strategy_id != manifest.id || proposal.strategy_version != manifest.version {
        return Err(McpProposalRejection::StrategyIdentityMismatch);
    }
    if result.render() != *result.raw() {
        return Err(McpProposalRejection::SourceRoundTripMismatch);
    }
    match &result.is_error {
        PreservedField::Value(true) => return Err(McpProposalRejection::ErrorResult),
        PreservedField::Opaque(_) => return Err(McpProposalRejection::OpaqueErrorState),
        PreservedField::Absent | PreservedField::Value(false) => {}
    }
    if proposal.replacements.is_empty() {
        return Err(McpProposalRejection::EmptyProposal);
    }

    let mut targets = HashSet::with_capacity(proposal.replacements.len());
    let mut candidate = result.clone();
    for replacement in &proposal.replacements {
        if !targets.insert(replacement.block_index) {
            return Err(McpProposalRejection::DuplicateTarget);
        }
        let block = candidate
            .content
            .get_mut(replacement.block_index)
            .ok_or(McpProposalRejection::TargetOutOfBounds)?;
        let CanonicalContentBlock::Text { text, .. } = block else {
            return Err(McpProposalRejection::TargetIsNotPlainText);
        };
        if text != &replacement.expected_text {
            return Err(McpProposalRejection::StaleSourceText);
        }
        let source_chars = text.chars().count();
        let replacement_chars = replacement.replacement.chars().count();
        if (replacement_chars as u128) * 100
            > (source_chars as u128) * (manifest.max_expansion_percent as u128)
        {
            return Err(McpProposalRejection::ExpansionLimitExceeded);
        }
        *text = replacement.replacement.clone();
    }
    let chars_in: usize = result
        .content
        .iter()
        .filter_map(CanonicalContentBlock::text)
        .map(|text| text.chars().count())
        .sum();
    let chars_out: usize = candidate
        .content
        .iter()
        .filter_map(CanonicalContentBlock::text)
        .map(|text| text.chars().count())
        .sum();
    if chars_out > proposal.max_total_text_chars {
        return Err(McpProposalRejection::OutputBudgetExceeded);
    }
    if chars_out >= chars_in {
        return Err(McpProposalRejection::NoSavings);
    }

    let original = result
        .raw()
        .as_object()
        .ok_or(McpProposalRejection::SourceRoundTripMismatch)?;
    let rendered = candidate.render();
    let rendered_object = rendered
        .as_object()
        .ok_or(McpProposalRejection::RenderedContractInvalid)?;
    if !maps_equal_except(original, rendered_object, "content") {
        return Err(McpProposalRejection::TopLevelInvariantFailed);
    }

    let original_content = original
        .get("content")
        .and_then(Value::as_array)
        .ok_or(McpProposalRejection::SourceRoundTripMismatch)?;
    let rendered_content = rendered_object
        .get("content")
        .and_then(Value::as_array)
        .ok_or(McpProposalRejection::RenderedContractInvalid)?;
    if original_content.len() != rendered_content.len() {
        return Err(McpProposalRejection::ContentLengthInvariantFailed);
    }
    for (index, (before, after)) in original_content
        .iter()
        .zip(rendered_content.iter())
        .enumerate()
    {
        if !targets.contains(&index) {
            if before != after {
                return Err(McpProposalRejection::NonTargetBlockChanged);
            }
            continue;
        }
        let replacement = proposal
            .replacements
            .iter()
            .find(|replacement| replacement.block_index == index)
            .expect("validated target has one replacement");
        if !same_text_block_envelope(before, after, &replacement.replacement) {
            return Err(McpProposalRejection::TargetEnvelopeChanged);
        }
    }

    let reparsed =
        parse_mcp_result(&rendered).map_err(|_| McpProposalRejection::RenderedContractInvalid)?;
    if reparsed.render() != rendered {
        return Err(McpProposalRejection::RenderedContractInvalid);
    }

    Ok(ValidatedMcpProposal {
        replacements: proposal.replacements.len(),
        chars_in,
        chars_out,
    })
}

fn same_text_block_envelope(before: &Value, after: &Value, replacement: &str) -> bool {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return false;
    };
    if after.get("type").and_then(Value::as_str) != Some("text")
        || after.get("text").and_then(Value::as_str) != Some(replacement)
    {
        return false;
    }
    maps_equal_except(before, after, "text")
}

fn maps_equal_except(
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
    excluded: &str,
) -> bool {
    let before_len = before.len() - usize::from(before.contains_key(excluded));
    let after_len = after.len() - usize::from(after.contains_key(excluded));
    before_len == after_len
        && before
            .iter()
            .filter(|(key, _)| key.as_str() != excluded)
            .all(|(key, value)| after.get(key) == Some(value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const INVARIANTS: &[&str] = &[
        "top-level-fields-unchanged",
        "non-target-blocks-unchanged",
        "target-envelope-unchanged",
    ];
    const MANIFEST: McpStrategyManifest = McpStrategyManifest {
        id: "test-text",
        version: "1",
        eligible_shape: "plain-text-blocks",
        invariants: INVARIANTS,
        max_expansion_percent: 100,
    };

    fn source() -> CanonicalMcpResult {
        parse_mcp_result(&json!({
            "content": [
                {"type": "text", "text": "long source text", "annotations": {"priority": 1}},
                {"type": "image", "data": "abc", "mimeType": "image/png", "vendor": true}
            ],
            "structuredContent": {"count": 2},
            "isError": false,
            "_meta": {"request": "r1"}
        }))
        .expect("source result")
    }

    fn proposal(replacement: &str) -> McpTransformProposal {
        McpTransformProposal {
            strategy_id: MANIFEST.id,
            strategy_version: MANIFEST.version,
            max_total_text_chars: 100,
            replacements: vec![McpTextReplacement {
                block_index: 0,
                expected_text: "long source text".into(),
                replacement: replacement.into(),
            }],
        }
    }

    #[test]
    fn validates_a_narrow_text_only_proposal() {
        let validated = validate_mcp_proposal(&source(), &MANIFEST, &proposal("short"))
            .expect("valid proposal");
        assert_eq!(validated.replacements, 1);
        assert_eq!(validated.chars_in, 16);
        assert_eq!(validated.chars_out, 5);
    }

    #[test]
    fn rejects_error_stale_duplicate_and_non_text_targets() {
        let mut error = source();
        error.is_error = PreservedField::Value(true);
        assert_eq!(
            validate_mcp_proposal(&error, &MANIFEST, &proposal("short")),
            Err(McpProposalRejection::SourceRoundTripMismatch),
            "mutating a parsed source invalidates its raw contract before any proposal runs"
        );

        let mut stale = proposal("short");
        stale.replacements[0].expected_text = "different".into();
        assert_eq!(
            validate_mcp_proposal(&source(), &MANIFEST, &stale),
            Err(McpProposalRejection::StaleSourceText)
        );

        let mut duplicate = proposal("short");
        duplicate
            .replacements
            .push(duplicate.replacements[0].clone());
        assert_eq!(
            validate_mcp_proposal(&source(), &MANIFEST, &duplicate),
            Err(McpProposalRejection::DuplicateTarget)
        );

        let mut non_text = proposal("short");
        non_text.replacements[0].block_index = 1;
        non_text.replacements[0].expected_text = "abc".into();
        assert_eq!(
            validate_mcp_proposal(&source(), &MANIFEST, &non_text),
            Err(McpProposalRejection::TargetIsNotPlainText)
        );

        let mut over_budget = proposal("short");
        over_budget.max_total_text_chars = 4;
        assert_eq!(
            validate_mcp_proposal(&source(), &MANIFEST, &over_budget),
            Err(McpProposalRejection::OutputBudgetExceeded)
        );
    }

    #[test]
    fn rejects_protocol_error_results_without_transforming_them() {
        let error = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "actionable error details"}],
            "isError": true
        }))
        .expect("error result");
        let error_proposal = McpTransformProposal {
            strategy_id: MANIFEST.id,
            strategy_version: MANIFEST.version,
            max_total_text_chars: 100,
            replacements: vec![McpTextReplacement {
                block_index: 0,
                expected_text: "actionable error details".into(),
                replacement: "error".into(),
            }],
        };
        assert_eq!(
            validate_mcp_proposal(&error, &MANIFEST, &error_proposal),
            Err(McpProposalRejection::ErrorResult)
        );
        assert_eq!(error.render(), *error.raw());
    }
}
