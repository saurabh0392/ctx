use crate::tool_result::{
    validate_mcp_proposal, CanonicalContentBlock, CanonicalMcpResult, McpProposalRejection,
    McpStrategyManifest, McpTextReplacement, McpTransformProposal, PreservedField,
    ValidatedMcpProposal,
};

use super::mcp::compress_mcp_output;
use super::types::{CompressContext, CompressOptions};

const TEXT_BLOCK_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "top-level-fields-unchanged",
    "content-block-count-unchanged",
    "non-target-blocks-unchanged",
    "target-text-envelope-unchanged",
    "error-results-pass-through",
    "result-contract-reparses",
];

pub(crate) const MCP_TEXT_BLOCKS_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-text-blocks",
    version: "1",
    eligible_shape: "plain-text-content-blocks",
    invariants: TEXT_BLOCK_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) struct McpStrategyObservation {
    pub manifest: Option<&'static McpStrategyManifest>,
    pub proposal_attempted: bool,
    pub validated: Option<ValidatedMcpProposal>,
    pub rejection: Option<McpProposalRejection>,
    pub pass_through_reason: Option<&'static str>,
}

trait McpResultStrategy: Sync {
    fn manifest(&self) -> &'static McpStrategyManifest;
    fn eligible(&self, result: &CanonicalMcpResult) -> bool;
    fn propose(
        &self,
        result: &CanonicalMcpResult,
        opts: &CompressOptions,
        ctx: &CompressContext,
    ) -> Option<McpTransformProposal>;
}

struct TextBlockStrategy;

impl McpResultStrategy for TextBlockStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_TEXT_BLOCKS_V1
    }

    fn eligible(&self, result: &CanonicalMcpResult) -> bool {
        result
            .content
            .iter()
            .any(|block| matches!(block, CanonicalContentBlock::Text { .. }))
    }

    fn propose(
        &self,
        result: &CanonicalMcpResult,
        opts: &CompressOptions,
        ctx: &CompressContext,
    ) -> Option<McpTransformProposal> {
        let total_chars: usize = result
            .content
            .iter()
            .filter_map(CanonicalContentBlock::text)
            .map(|text| text.chars().count())
            .sum();
        if total_chars == 0 || total_chars <= opts.target_chars {
            return None;
        }

        let total_budget = opts.target_chars.max(1);
        let mut replacements = Vec::new();
        for (block_index, block) in result.content.iter().enumerate() {
            let CanonicalContentBlock::Text { text, .. } = block else {
                continue;
            };
            let source_chars = text.chars().count();
            let proportional_budget = ((total_budget as u128) * (source_chars as u128)
                / (total_chars as u128))
                .max(1)
                .min(usize::MAX as u128) as usize;
            let mut block_opts = opts.clone();
            block_opts.target_chars = proportional_budget;
            let compressed = compress_mcp_output(text, &block_opts, ctx);
            if compressed.chars_out >= source_chars {
                continue;
            }
            replacements.push(McpTextReplacement {
                block_index,
                expected_text: text.clone(),
                replacement: compressed.text,
            });
        }
        (!replacements.is_empty()).then_some(McpTransformProposal {
            strategy_id: self.manifest().id,
            strategy_version: self.manifest().version,
            max_total_text_chars: total_budget,
            replacements,
        })
    }
}

static TEXT_BLOCK_STRATEGY: TextBlockStrategy = TextBlockStrategy;
static STRATEGIES: [&dyn McpResultStrategy; 1] = [&TEXT_BLOCK_STRATEGY];

/// Evaluate the deterministic registry in shadow mode. Eligibility is intentionally recorded even
/// when no proposal is useful, and neither state grants permission to apply a trim.
pub(crate) fn evaluate_mcp_strategies_shadow(
    result: &CanonicalMcpResult,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> McpStrategyObservation {
    let pass_through_reason = match &result.is_error {
        PreservedField::Value(true) => Some(McpProposalRejection::ErrorResult.code()),
        PreservedField::Opaque(_) => Some(McpProposalRejection::OpaqueErrorState.code()),
        PreservedField::Absent | PreservedField::Value(false) => None,
    };
    if pass_through_reason.is_some() {
        return McpStrategyObservation {
            manifest: None,
            proposal_attempted: false,
            validated: None,
            rejection: None,
            pass_through_reason,
        };
    }

    for strategy in STRATEGIES {
        if !strategy.eligible(result) {
            continue;
        }
        let manifest = strategy.manifest();
        let Some(proposal) = strategy.propose(result, opts, ctx) else {
            return McpStrategyObservation {
                manifest: Some(manifest),
                proposal_attempted: false,
                validated: None,
                rejection: None,
                pass_through_reason: Some("within-budget"),
            };
        };
        return match validate_mcp_proposal(result, manifest, &proposal) {
            Ok(validated) => McpStrategyObservation {
                manifest: Some(manifest),
                proposal_attempted: true,
                validated: Some(validated),
                rejection: None,
                pass_through_reason: None,
            },
            Err(rejection) => McpStrategyObservation {
                manifest: Some(manifest),
                proposal_attempted: true,
                validated: None,
                rejection: Some(rejection),
                pass_through_reason: Some(rejection.code()),
            },
        };
    }

    McpStrategyObservation {
        manifest: None,
        proposal_attempted: false,
        validated: None,
        rejection: None,
        pass_through_reason: Some("unsupported-shape"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tool_result::parse_mcp_result;

    fn options(target_chars: usize) -> CompressOptions {
        CompressOptions {
            target_chars,
            ..Default::default()
        }
    }

    #[test]
    fn registry_is_deterministic_and_versioned() {
        let manifests: Vec<_> = STRATEGIES
            .iter()
            .map(|strategy| strategy.manifest())
            .collect();
        assert_eq!(manifests, vec![&MCP_TEXT_BLOCKS_V1]);
        assert!(!manifests[0].invariants.is_empty());
    }

    #[test]
    fn mixed_results_get_a_valid_text_block_only_proposal() {
        let result = parse_mcp_result(&json!({
            "content": [
                {"type": "text", "text": "repeat line\nrepeat line\nrepeat line\nimportant tail"},
                {"type": "image", "data": "abc", "mimeType": "image/png"},
                {"type": "future", "payload": [1, 2, 3]}
            ],
            "structuredContent": {"nextCursor": "c2"},
            "isError": false,
            "vendor": true
        }))
        .expect("mixed result");
        let observation =
            evaluate_mcp_strategies_shadow(&result, &options(12), &CompressContext::default());
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V1));
        assert!(observation.proposal_attempted);
        let validated = observation.validated.expect("validated proposal");
        assert_eq!(validated.replacements, 1);
        assert!(validated.chars_out < validated.chars_in);
        assert!(observation.rejection.is_none());
    }

    #[test]
    fn multiple_text_blocks_keep_their_own_targets_and_budgets() {
        let result = parse_mcp_result(&json!({
            "content": [
                {"type": "text", "text": "first repeated line\nfirst repeated line\nfirst tail"},
                {"type": "image", "data": "abc", "mimeType": "image/png"},
                {"type": "text", "text": "second repeated line\nsecond repeated line\nsecond tail"}
            ]
        }))
        .expect("multi-text result");
        let opts = options(20);
        let proposal = TEXT_BLOCK_STRATEGY
            .propose(&result, &opts, &CompressContext::default())
            .expect("block-aware proposal");
        let targets: Vec<_> = proposal
            .replacements
            .iter()
            .map(|replacement| replacement.block_index)
            .collect();
        assert_eq!(targets, vec![0, 2]);
        let validated = validate_mcp_proposal(&result, &MCP_TEXT_BLOCKS_V1, &proposal)
            .expect("validated block-aware proposal");
        assert_eq!(validated.replacements, 2);
        assert!(validated.chars_out <= 20);
    }

    #[test]
    fn errors_and_unsupported_shapes_pass_through_before_proposal() {
        let error = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "actionable failure"}],
            "isError": true
        }))
        .expect("error result");
        let error_observation =
            evaluate_mcp_strategies_shadow(&error, &options(1), &CompressContext::default());
        assert!(error_observation.manifest.is_none());
        assert!(!error_observation.proposal_attempted);
        assert_eq!(error_observation.pass_through_reason, Some("error-result"));

        let image = parse_mcp_result(&json!({
            "content": [{"type": "image", "data": "abc", "mimeType": "image/png"}]
        }))
        .expect("image result");
        let image_observation =
            evaluate_mcp_strategies_shadow(&image, &options(1), &CompressContext::default());
        assert!(image_observation.manifest.is_none());
        assert_eq!(
            image_observation.pass_through_reason,
            Some("unsupported-shape")
        );
    }

    #[test]
    fn eligibility_is_recorded_separately_when_no_proposal_is_needed() {
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "already small"}]
        }))
        .expect("small result");
        let observation =
            evaluate_mcp_strategies_shadow(&result, &options(100), &CompressContext::default());
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V1));
        assert!(!observation.proposal_attempted);
        assert!(observation.validated.is_none());
        assert_eq!(observation.pass_through_reason, Some("within-budget"));
    }
}
