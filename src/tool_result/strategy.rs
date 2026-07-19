use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value};

use super::{
    parse_mcp_result, validate_mcp_output_schema, CanonicalContentBlock, CanonicalMcpResult,
    McpOutputSchemaValidation, McpSchemaRejection, PreservedField, ToolContract,
};

pub const MCP_OMISSION_MARKER_FIELD: &str = "_ctxOmission";
#[deprecated(note = "use MCP_OMISSION_MARKER_FIELD")]
pub const MCP_COLLECTION_OMISSION_MARKER_FIELD: &str = MCP_OMISSION_MARKER_FIELD;
pub const MCP_MAX_RETAINED_COLLECTION_ITEMS: usize = 64;
pub const MCP_MAX_RETAINED_SEARCH_RESULTS: usize = 64;
pub const MCP_MAX_ENTITY_FIELDS: usize = 128;
pub const MCP_MAX_ENTITY_OMITTED_FIELDS: usize = 64;
const MCP_MAX_ENTITY_REQUESTED_FIELDS: usize = 64;
const MCP_MAX_ENTITY_INPUT_NODES: usize = 256;
const MCP_MAX_ENTITY_INPUT_DEPTH: usize = 8;
const MCP_MAX_ENTITY_FIELD_NAME_BYTES: usize = 128;

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

/// A structured replacement is deliberately typed by transform family. This prevents a future
/// strategy from using the proposal boundary as a generic JSON rewrite escape hatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpStructuredContentReplacement {
    pub expected: Value,
    pub replacement: Value,
    pub edit: McpStructuredContentEdit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpStructuredContentEdit {
    PaginatedCollection(McpPaginatedCollectionEdit),
    SearchResults(McpSearchResultsEdit),
    EntityDetail(McpEntityDetailEdit),
}

/// Proof inputs for one top-level collection reduction. The validator independently checks every
/// index and marker field; strategies cannot assert that an arbitrary replacement is a collection
/// trim merely by constructing this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPaginatedCollectionEdit {
    pub field: String,
    pub retained_indices: Vec<usize>,
    pub omission_marker_field: String,
}

/// Proof inputs for one schema-authorized search-result reduction. The server's source ordering is
/// treated as the ranking contract: strategies may retain only an exact prefix and may never
/// reorder or synthesize result entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSearchResultsEdit {
    pub field: String,
    pub identity_field: String,
    pub match_evidence_field: String,
    pub retained_indices: Vec<usize>,
    pub omission_marker_field: String,
}

/// Proof inputs for one schema-authorized entity/detail reduction. The protected set is
/// independently derived from the advertised schema and bounded tool input; a proposal records
/// the requested and omitted fields only so the validator can reject stale or forged context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEntityDetailEdit {
    pub identity_field: String,
    pub requested_fields: Vec<String>,
    pub omitted_fields: Vec<String>,
    pub omission_marker_field: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpEntitySchemaAuthorization {
    pub identity_field: String,
    pub requested_fields: Vec<String>,
    pub protected_fields: Vec<String>,
    pub removable_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpEntitySchemaRejection {
    SchemaMissing,
    SchemaUnsupported,
    SourceTooWide,
    IdentityEvidenceMissing,
    IdentityValueMissing,
    InputTooLarge,
    InputSelectorAmbiguous,
    InputSelectorUnsupported,
    RequestedFieldUnknown,
}

impl McpEntitySchemaRejection {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::SchemaMissing => "entity-object-schema-missing",
            Self::SchemaUnsupported => "entity-object-schema-unsupported",
            Self::SourceTooWide => "entity-object-too-wide",
            Self::IdentityEvidenceMissing => "entity-identity-evidence-missing",
            Self::IdentityValueMissing => "entity-identity-value-missing",
            Self::InputTooLarge => "entity-input-context-too-large",
            Self::InputSelectorAmbiguous => "entity-field-selector-ambiguous",
            Self::InputSelectorUnsupported => "entity-field-selector-unsupported",
            Self::RequestedFieldUnknown => "entity-requested-field-unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpSearchSchemaAuthorization {
    pub identity_field: String,
    pub match_evidence_field: String,
    pub min_results: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpSearchSchemaRejection {
    ArraySchemaMissing,
    ArraySchemaUnsupported,
    PositionalSchemaUnsupported,
    ItemSchemaUnsupported,
    IdentityEvidenceMissing,
    MatchEvidenceMissing,
}

impl McpSearchSchemaRejection {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::ArraySchemaMissing => "search-array-schema-missing",
            Self::ArraySchemaUnsupported => "search-array-schema-unsupported",
            Self::PositionalSchemaUnsupported => "search-positional-schema-unsupported",
            Self::ItemSchemaUnsupported => "search-item-schema-unsupported",
            Self::IdentityEvidenceMissing => "search-identity-evidence-missing",
            Self::MatchEvidenceMissing => "search-match-evidence-missing",
        }
    }
}

/// A contentful, in-memory proposal. T2 does not expose a renderer or an apply operation from this
/// type: the proposal exists only to exercise the validator and record content-free shadow proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTransformProposal {
    pub strategy_id: &'static str,
    pub strategy_version: &'static str,
    pub max_total_text_chars: usize,
    pub replacements: Vec<McpTextReplacement>,
    pub structured_content: Option<McpStructuredContentReplacement>,
}

/// Content-free proof emitted after an entire proposal passes structural invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedMcpProposal {
    pub replacements: usize,
    pub chars_in: usize,
    pub chars_out: usize,
    pub output_schema_validated: bool,
    pub structured_content_replaced: bool,
    pub collection_items_in: Option<usize>,
    pub collection_items_out: Option<usize>,
    pub collection_items_omitted: Option<usize>,
    pub search_results_in: Option<usize>,
    pub search_results_out: Option<usize>,
    pub search_results_omitted: Option<usize>,
    pub entity_fields_in: Option<usize>,
    pub entity_fields_out: Option<usize>,
    pub entity_fields_omitted: Option<usize>,
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
    StaleStructuredContent,
    StructuredContentSchemaRequired,
    ExpansionLimitExceeded,
    OutputBudgetExceeded,
    NoSavings,
    TopLevelInvariantFailed,
    ContentLengthInvariantFailed,
    NonTargetBlockChanged,
    TargetEnvelopeChanged,
    StructuredContentInvariantFailed,
    CollectionTargetInvalid,
    CollectionSelectionInvalid,
    CollectionTextMirrorInvalid,
    CollectionOmissionMarkerInvalid,
    SearchTargetInvalid,
    SearchSchemaAuthorizationInvalid,
    SearchSelectionInvalid,
    SearchTextMirrorInvalid,
    SearchOmissionMarkerInvalid,
    EntityTargetInvalid,
    EntitySchemaAuthorizationInvalid,
    EntitySelectionInvalid,
    EntityTextMirrorInvalid,
    EntityOmissionMarkerInvalid,
    RenderedContractInvalid,
    SourceSchema(McpSchemaRejection),
    CandidateSchema(McpSchemaRejection),
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
            Self::StaleStructuredContent => "stale-structured-content",
            Self::StructuredContentSchemaRequired => "structured-content-output-schema-required",
            Self::ExpansionLimitExceeded => "expansion-limit-exceeded",
            Self::OutputBudgetExceeded => "output-budget-exceeded",
            Self::NoSavings => "no-savings",
            Self::TopLevelInvariantFailed => "top-level-invariant-failed",
            Self::ContentLengthInvariantFailed => "content-length-invariant-failed",
            Self::NonTargetBlockChanged => "non-target-block-changed",
            Self::TargetEnvelopeChanged => "target-envelope-changed",
            Self::StructuredContentInvariantFailed => "structured-content-invariant-failed",
            Self::CollectionTargetInvalid => "collection-target-invalid",
            Self::CollectionSelectionInvalid => "collection-selection-invalid",
            Self::CollectionTextMirrorInvalid => "collection-text-mirror-invalid",
            Self::CollectionOmissionMarkerInvalid => "collection-omission-marker-invalid",
            Self::SearchTargetInvalid => "search-target-invalid",
            Self::SearchSchemaAuthorizationInvalid => "search-schema-authorization-invalid",
            Self::SearchSelectionInvalid => "search-selection-invalid",
            Self::SearchTextMirrorInvalid => "search-text-mirror-invalid",
            Self::SearchOmissionMarkerInvalid => "search-omission-marker-invalid",
            Self::EntityTargetInvalid => "entity-target-invalid",
            Self::EntitySchemaAuthorizationInvalid => "entity-schema-authorization-invalid",
            Self::EntitySelectionInvalid => "entity-selection-invalid",
            Self::EntityTextMirrorInvalid => "entity-text-mirror-invalid",
            Self::EntityOmissionMarkerInvalid => "entity-omission-marker-invalid",
            Self::RenderedContractInvalid => "rendered-contract-invalid",
            Self::SourceSchema(rejection) => rejection.code(),
            Self::CandidateSchema(rejection) => candidate_schema_code(rejection),
        }
    }
}

fn candidate_schema_code(rejection: McpSchemaRejection) -> &'static str {
    match rejection {
        McpSchemaRejection::StructuredContentMissing => "candidate-structured-content-missing",
        McpSchemaRejection::StructuredContentOpaque => "candidate-structured-content-not-object",
        McpSchemaRejection::StructuredContentTooLarge => "candidate-structured-content-too-large",
        McpSchemaRejection::StructuredContentTooDeep => "candidate-structured-content-too-deep",
        McpSchemaRejection::InstanceInvalid => "candidate-structured-content-schema-mismatch",
        schema_rejection => schema_rejection.code(),
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
    validate_mcp_proposal_with_contract(result, None, manifest, proposal)
}

/// Contract-aware proposal validation. The advertised schema is checked against the source and
/// rebuilt candidate while both values remain inside this boundary.
pub fn validate_mcp_proposal_with_contract(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    manifest: &McpStrategyManifest,
    proposal: &McpTransformProposal,
) -> Result<ValidatedMcpProposal, McpProposalRejection> {
    validate_mcp_proposal_with_contract_and_input(result, contract, None, manifest, proposal)
}

/// Contract- and input-aware proposal validation. Only strategies whose semantic protections
/// depend on a bounded portion of tool input need the extra context; existing callers retain the
/// narrower contract-only API above.
pub fn validate_mcp_proposal_with_contract_and_input(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
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
    let output_schema_validated = match validate_mcp_output_schema(contract, result) {
        McpOutputSchemaValidation::NotAdvertised => false,
        McpOutputSchemaValidation::Valid => true,
        McpOutputSchemaValidation::Rejected(rejection) => {
            return Err(McpProposalRejection::SourceSchema(rejection))
        }
    };
    if proposal.structured_content.is_some() && !output_schema_validated {
        return Err(McpProposalRejection::StructuredContentSchemaRequired);
    }
    if proposal.replacements.is_empty() && proposal.structured_content.is_none() {
        return Err(McpProposalRejection::EmptyProposal);
    }

    let mut replacements_by_target = HashMap::with_capacity(proposal.replacements.len());
    let mut candidate = result.clone();
    for replacement in &proposal.replacements {
        if replacements_by_target
            .insert(replacement.block_index, replacement)
            .is_some()
        {
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
    let structured_metrics = match &proposal.structured_content {
        Some(replacement) => Some(validate_and_apply_structured_content(
            result,
            &mut candidate,
            contract,
            tool_input,
            proposal.max_total_text_chars,
            replacement,
            &replacements_by_target,
        )?),
        None => None,
    };
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
    let allowed_top_level_changes: &[&str] = if proposal.structured_content.is_some() {
        &["content", "structuredContent"]
    } else {
        &["content"]
    };
    if !maps_equal_except_many(original, rendered_object, allowed_top_level_changes) {
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
        let Some(replacement) = replacements_by_target.get(&index) else {
            if before != after {
                return Err(McpProposalRejection::NonTargetBlockChanged);
            }
            continue;
        };
        if !same_text_block_envelope(before, after, &replacement.replacement) {
            return Err(McpProposalRejection::TargetEnvelopeChanged);
        }
    }

    let reparsed =
        parse_mcp_result(&rendered).map_err(|_| McpProposalRejection::RenderedContractInvalid)?;
    if reparsed.render() != rendered {
        return Err(McpProposalRejection::RenderedContractInvalid);
    }
    match validate_mcp_output_schema(contract, &reparsed) {
        McpOutputSchemaValidation::NotAdvertised => {}
        McpOutputSchemaValidation::Valid => {}
        McpOutputSchemaValidation::Rejected(rejection) => {
            return Err(McpProposalRejection::CandidateSchema(rejection))
        }
    }

    Ok(ValidatedMcpProposal {
        replacements: proposal.replacements.len(),
        chars_in,
        chars_out,
        output_schema_validated,
        structured_content_replaced: proposal.structured_content.is_some(),
        collection_items_in: structured_metrics
            .and_then(StructuredEditMetrics::collection_items_in),
        collection_items_out: structured_metrics
            .and_then(StructuredEditMetrics::collection_items_out),
        collection_items_omitted: structured_metrics
            .and_then(StructuredEditMetrics::collection_items_omitted),
        search_results_in: structured_metrics.and_then(StructuredEditMetrics::search_results_in),
        search_results_out: structured_metrics.and_then(StructuredEditMetrics::search_results_out),
        search_results_omitted: structured_metrics
            .and_then(StructuredEditMetrics::search_results_omitted),
        entity_fields_in: structured_metrics.and_then(StructuredEditMetrics::entity_fields_in),
        entity_fields_out: structured_metrics.and_then(StructuredEditMetrics::entity_fields_out),
        entity_fields_omitted: structured_metrics
            .and_then(StructuredEditMetrics::entity_fields_omitted),
    })
}

#[derive(Debug, Clone, Copy)]
struct CollectionMetrics {
    items_in: usize,
    items_out: usize,
    items_omitted: usize,
}

#[derive(Debug, Clone, Copy)]
enum StructuredEditMetrics {
    Collection(CollectionMetrics),
    Search(CollectionMetrics),
    Entity(CollectionMetrics),
}

impl StructuredEditMetrics {
    fn collection_items_in(self) -> Option<usize> {
        match self {
            Self::Collection(metrics) => Some(metrics.items_in),
            Self::Search(_) | Self::Entity(_) => None,
        }
    }

    fn collection_items_out(self) -> Option<usize> {
        match self {
            Self::Collection(metrics) => Some(metrics.items_out),
            Self::Search(_) | Self::Entity(_) => None,
        }
    }

    fn collection_items_omitted(self) -> Option<usize> {
        match self {
            Self::Collection(metrics) => Some(metrics.items_omitted),
            Self::Search(_) | Self::Entity(_) => None,
        }
    }

    fn search_results_in(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Entity(_) => None,
            Self::Search(metrics) => Some(metrics.items_in),
        }
    }

    fn search_results_out(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Entity(_) => None,
            Self::Search(metrics) => Some(metrics.items_out),
        }
    }

    fn search_results_omitted(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Entity(_) => None,
            Self::Search(metrics) => Some(metrics.items_omitted),
        }
    }

    fn entity_fields_in(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Search(_) => None,
            Self::Entity(metrics) => Some(metrics.items_in),
        }
    }

    fn entity_fields_out(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Search(_) => None,
            Self::Entity(metrics) => Some(metrics.items_out),
        }
    }

    fn entity_fields_omitted(self) -> Option<usize> {
        match self {
            Self::Collection(_) | Self::Search(_) => None,
            Self::Entity(metrics) => Some(metrics.items_omitted),
        }
    }
}

fn validate_and_apply_structured_content(
    result: &CanonicalMcpResult,
    candidate: &mut CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
    max_total_text_chars: usize,
    replacement: &McpStructuredContentReplacement,
    text_replacements: &HashMap<usize, &McpTextReplacement>,
) -> Result<StructuredEditMetrics, McpProposalRejection> {
    let PreservedField::Value(source) = &result.structured_content else {
        return Err(McpProposalRejection::StaleStructuredContent);
    };
    if source != &replacement.expected {
        return Err(McpProposalRejection::StaleStructuredContent);
    }

    let metrics = match &replacement.edit {
        McpStructuredContentEdit::PaginatedCollection(edit) => {
            StructuredEditMetrics::Collection(validate_collection_edit(
                result,
                &replacement.expected,
                &replacement.replacement,
                edit,
                text_replacements,
            )?)
        }
        McpStructuredContentEdit::SearchResults(edit) => {
            StructuredEditMetrics::Search(validate_search_results_edit(
                result,
                contract,
                &replacement.expected,
                &replacement.replacement,
                edit,
                text_replacements,
            )?)
        }
        McpStructuredContentEdit::EntityDetail(edit) => {
            StructuredEditMetrics::Entity(validate_entity_detail_edit(
                result,
                contract,
                tool_input,
                max_total_text_chars,
                &replacement.expected,
                &replacement.replacement,
                edit,
                text_replacements,
            )?)
        }
    };
    candidate.structured_content = PreservedField::Value(replacement.replacement.clone());
    Ok(metrics)
}

fn validate_search_results_edit(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    source: &Value,
    candidate: &Value,
    edit: &McpSearchResultsEdit,
    text_replacements: &HashMap<usize, &McpTextReplacement>,
) -> Result<CollectionMetrics, McpProposalRejection> {
    let (Some(source), Some(candidate)) = (source.as_object(), candidate.as_object()) else {
        return Err(McpProposalRejection::SearchTargetInvalid);
    };
    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return Err(McpProposalRejection::SearchSchemaAuthorizationInvalid);
    };
    let Ok(authorization) = assess_mcp_search_array_schema(schema, &edit.field) else {
        return Err(McpProposalRejection::SearchSchemaAuthorizationInvalid);
    };
    if authorization.identity_field != edit.identity_field
        || authorization.match_evidence_field != edit.match_evidence_field
    {
        return Err(McpProposalRejection::SearchSchemaAuthorizationInvalid);
    }
    if !maps_equal_except(source, candidate, &edit.field) {
        return Err(McpProposalRejection::StructuredContentInvariantFailed);
    }
    let (Some(source_results), Some(candidate_results)) = (
        source.get(&edit.field).and_then(Value::as_array),
        candidate.get(&edit.field).and_then(Value::as_array),
    ) else {
        return Err(McpProposalRejection::SearchTargetInvalid);
    };
    if source_results.iter().any(|result| {
        let Some(result) = result.as_object() else {
            return true;
        };
        result.get(&edit.identity_field).is_none_or(Value::is_null)
            || result
                .get(&edit.match_evidence_field)
                .is_none_or(Value::is_null)
    }) {
        return Err(McpProposalRejection::SearchSchemaAuthorizationInvalid);
    }
    if source_results.len() <= candidate_results.len()
        || candidate_results.is_empty()
        || candidate_results.len() > MCP_MAX_RETAINED_SEARCH_RESULTS
        || candidate_results.len() != edit.retained_indices.len()
        || edit.retained_indices
            != search_ranked_prefix_indices(source_results.len(), candidate_results.len())
    {
        return Err(McpProposalRejection::SearchSelectionInvalid);
    }
    for (candidate_result, index) in candidate_results.iter().zip(&edit.retained_indices) {
        if source_results.get(*index) != Some(candidate_result) {
            return Err(McpProposalRejection::SearchSelectionInvalid);
        }
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 || text_replacements.len() != 1 {
        return Err(McpProposalRejection::SearchTextMirrorInvalid);
    }
    let (block_index, source_text) = text_blocks[0];
    let Some(text_replacement) = text_replacements.get(&block_index) else {
        return Err(McpProposalRejection::SearchTextMirrorInvalid);
    };
    let parsed_source: Value = serde_json::from_str(source_text)
        .map_err(|_| McpProposalRejection::SearchTextMirrorInvalid)?;
    if parsed_source != Value::Object(source.clone()) {
        return Err(McpProposalRejection::SearchTextMirrorInvalid);
    }
    let mut parsed_candidate: Value = serde_json::from_str(&text_replacement.replacement)
        .map_err(|_| McpProposalRejection::SearchTextMirrorInvalid)?;
    if edit.omission_marker_field != MCP_OMISSION_MARKER_FIELD {
        return Err(McpProposalRejection::SearchOmissionMarkerInvalid);
    }
    let marker = parsed_candidate
        .as_object_mut()
        .and_then(|object| object.remove(&edit.omission_marker_field))
        .ok_or(McpProposalRejection::SearchOmissionMarkerInvalid)?;
    if parsed_candidate != Value::Object(candidate.clone()) {
        return Err(McpProposalRejection::SearchTextMirrorInvalid);
    }

    let items_in = source_results.len();
    let items_out = candidate_results.len();
    let items_omitted = items_in - items_out;
    let expected_marker = serde_json::json!({
        "field": edit.field,
        "originalItems": items_in,
        "retainedItems": items_out,
        "omittedItems": items_omitted,
        "selection": "ranked-prefix"
    });
    if marker != expected_marker {
        return Err(McpProposalRejection::SearchOmissionMarkerInvalid);
    }

    Ok(CollectionMetrics {
        items_in,
        items_out,
        items_omitted,
    })
}

fn validate_entity_detail_edit(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
    max_total_text_chars: usize,
    source: &Value,
    candidate: &Value,
    edit: &McpEntityDetailEdit,
    text_replacements: &HashMap<usize, &McpTextReplacement>,
) -> Result<CollectionMetrics, McpProposalRejection> {
    let (Some(source), Some(candidate)) = (source.as_object(), candidate.as_object()) else {
        return Err(McpProposalRejection::EntityTargetInvalid);
    };
    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return Err(McpProposalRejection::EntitySchemaAuthorizationInvalid);
    };
    let authorization = assess_mcp_entity_schema(schema, source, tool_input)
        .map_err(|_| McpProposalRejection::EntitySchemaAuthorizationInvalid)?;
    if edit.identity_field != authorization.identity_field
        || edit.requested_fields != authorization.requested_fields
        || edit.omitted_fields.is_empty()
        || edit.omitted_fields.len() > MCP_MAX_ENTITY_OMITTED_FIELDS
        || edit.omitted_fields.len() > authorization.removable_fields.len()
        || edit.omitted_fields != authorization.removable_fields[..edit.omitted_fields.len()]
    {
        return Err(McpProposalRejection::EntitySelectionInvalid);
    }

    let rebuilt = entity_detail_candidate(source, &edit.omitted_fields);
    if candidate != &rebuilt {
        return Err(McpProposalRejection::EntitySelectionInvalid);
    }
    if authorization
        .protected_fields
        .iter()
        .any(|field| candidate.get(field) != source.get(field))
    {
        return Err(McpProposalRejection::EntitySelectionInvalid);
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 || text_replacements.len() != 1 {
        return Err(McpProposalRejection::EntityTextMirrorInvalid);
    }
    let (block_index, source_text) = text_blocks[0];
    let Some(text_replacement) = text_replacements.get(&block_index) else {
        return Err(McpProposalRejection::EntityTextMirrorInvalid);
    };
    let parsed_source: Value = serde_json::from_str(source_text)
        .map_err(|_| McpProposalRejection::EntityTextMirrorInvalid)?;
    if parsed_source != Value::Object(source.clone()) {
        return Err(McpProposalRejection::EntityTextMirrorInvalid);
    }
    let mut parsed_candidate: Value = serde_json::from_str(&text_replacement.replacement)
        .map_err(|_| McpProposalRejection::EntityTextMirrorInvalid)?;
    if edit.omission_marker_field != MCP_OMISSION_MARKER_FIELD {
        return Err(McpProposalRejection::EntityOmissionMarkerInvalid);
    }
    let marker = parsed_candidate
        .as_object_mut()
        .and_then(|object| object.remove(&edit.omission_marker_field))
        .ok_or(McpProposalRejection::EntityOmissionMarkerInvalid)?;
    if parsed_candidate != Value::Object(candidate.clone()) {
        return Err(McpProposalRejection::EntityTextMirrorInvalid);
    }

    let fields_in = source.len();
    let fields_out = candidate.len();
    let fields_omitted = edit.omitted_fields.len();
    let expected_marker = serde_json::json!({
        "originalFields": fields_in,
        "retainedFields": fields_out,
        "omittedFields": fields_omitted,
        "fields": edit.omitted_fields,
        "selection": "schema-protected-entity-fields"
    });
    if marker != expected_marker || fields_in - fields_out != fields_omitted {
        return Err(McpProposalRejection::EntityOmissionMarkerInvalid);
    }

    let previous_projection_chars = if edit.omitted_fields.len() == 1 {
        source_text.chars().count()
    } else {
        let previous_omitted = &edit.omitted_fields[..edit.omitted_fields.len() - 1];
        let previous_candidate = entity_detail_candidate(source, previous_omitted);
        serde_json::to_string(&entity_detail_text_projection(
            &previous_candidate,
            source.len(),
            previous_omitted,
        ))
        .map_err(|_| McpProposalRejection::EntityTextMirrorInvalid)?
        .chars()
        .count()
    };
    if previous_projection_chars <= max_total_text_chars {
        return Err(McpProposalRejection::EntitySelectionInvalid);
    }

    Ok(CollectionMetrics {
        items_in: fields_in,
        items_out: fields_out,
        items_omitted: fields_omitted,
    })
}

pub(crate) fn entity_detail_candidate(
    source: &Map<String, Value>,
    omitted_fields: &[String],
) -> Map<String, Value> {
    let omitted: BTreeSet<_> = omitted_fields.iter().map(String::as_str).collect();
    source
        .iter()
        .filter(|(field, _)| !omitted.contains(field.as_str()))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}

pub(crate) fn entity_detail_text_projection(
    candidate: &Map<String, Value>,
    original_fields: usize,
    omitted_fields: &[String],
) -> Value {
    let mut projection = candidate.clone();
    projection.insert(
        MCP_OMISSION_MARKER_FIELD.to_string(),
        serde_json::json!({
            "originalFields": original_fields,
            "retainedFields": candidate.len(),
            "omittedFields": omitted_fields.len(),
            "fields": omitted_fields,
            "selection": "schema-protected-entity-fields"
        }),
    );
    Value::Object(projection)
}

fn validate_collection_edit(
    result: &CanonicalMcpResult,
    source: &Value,
    candidate: &Value,
    edit: &McpPaginatedCollectionEdit,
    text_replacements: &HashMap<usize, &McpTextReplacement>,
) -> Result<CollectionMetrics, McpProposalRejection> {
    let (Some(source), Some(candidate)) = (source.as_object(), candidate.as_object()) else {
        return Err(McpProposalRejection::CollectionTargetInvalid);
    };
    if !maps_equal_except(source, candidate, &edit.field) {
        return Err(McpProposalRejection::StructuredContentInvariantFailed);
    }
    let (Some(source_items), Some(candidate_items)) = (
        source.get(&edit.field).and_then(Value::as_array),
        candidate.get(&edit.field).and_then(Value::as_array),
    ) else {
        return Err(McpProposalRejection::CollectionTargetInvalid);
    };
    if source_items.len() <= candidate_items.len()
        || candidate_items.len() > MCP_MAX_RETAINED_COLLECTION_ITEMS
        || candidate_items.len() != edit.retained_indices.len()
        || edit.retained_indices.first() != Some(&0)
        || edit.retained_indices.last() != Some(&(source_items.len() - 1))
        || edit.retained_indices
            != collection_head_tail_indices(source_items.len(), candidate_items.len())
    {
        return Err(McpProposalRejection::CollectionSelectionInvalid);
    }
    let mut previous = None;
    for (candidate_item, index) in candidate_items.iter().zip(&edit.retained_indices) {
        if *index >= source_items.len()
            || previous.is_some_and(|previous| *index <= previous)
            || source_items.get(*index) != Some(candidate_item)
        {
            return Err(McpProposalRejection::CollectionSelectionInvalid);
        }
        previous = Some(*index);
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 || text_replacements.len() != 1 {
        return Err(McpProposalRejection::CollectionTextMirrorInvalid);
    }
    let (block_index, source_text) = text_blocks[0];
    let Some(text_replacement) = text_replacements.get(&block_index) else {
        return Err(McpProposalRejection::CollectionTextMirrorInvalid);
    };
    let parsed_source: Value = serde_json::from_str(source_text)
        .map_err(|_| McpProposalRejection::CollectionTextMirrorInvalid)?;
    if parsed_source != Value::Object(source.clone()) {
        return Err(McpProposalRejection::CollectionTextMirrorInvalid);
    }
    let mut parsed_candidate: Value = serde_json::from_str(&text_replacement.replacement)
        .map_err(|_| McpProposalRejection::CollectionTextMirrorInvalid)?;
    if edit.omission_marker_field != MCP_OMISSION_MARKER_FIELD {
        return Err(McpProposalRejection::CollectionOmissionMarkerInvalid);
    }
    let marker = parsed_candidate
        .as_object_mut()
        .and_then(|object| object.remove(&edit.omission_marker_field))
        .ok_or(McpProposalRejection::CollectionOmissionMarkerInvalid)?;
    if parsed_candidate != Value::Object(candidate.clone()) {
        return Err(McpProposalRejection::CollectionTextMirrorInvalid);
    }

    let items_in = source_items.len();
    let items_out = candidate_items.len();
    let items_omitted = items_in - items_out;
    let expected_marker = serde_json::json!({
        "field": edit.field,
        "originalItems": items_in,
        "retainedItems": items_out,
        "omittedItems": items_omitted,
        "selection": "first-and-last"
    });
    if marker != expected_marker {
        return Err(McpProposalRejection::CollectionOmissionMarkerInvalid);
    }

    Ok(CollectionMetrics {
        items_in,
        items_out,
        items_omitted,
    })
}

pub(crate) fn collection_head_tail_indices(total: usize, retained: usize) -> Vec<usize> {
    let head = retained.div_ceil(2);
    let tail = retained / 2;
    (0..head).chain((total - tail)..total).collect()
}

pub(crate) fn assess_mcp_entity_schema(
    root: &Value,
    source: &Map<String, Value>,
    tool_input: Option<&Value>,
) -> Result<McpEntitySchemaAuthorization, McpEntitySchemaRejection> {
    if source.len() > MCP_MAX_ENTITY_FIELDS {
        return Err(McpEntitySchemaRejection::SourceTooWide);
    }
    let schema = resolve_search_local_schema(root, root)
        .ok_or(McpEntitySchemaRejection::SchemaUnsupported)?;
    if !search_schema_types_are_subset_of(schema, &["object"])
        || entity_schema_has_removal_dependencies(schema)
    {
        return Err(McpEntitySchemaRejection::SchemaUnsupported);
    }
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(McpEntitySchemaRejection::SchemaMissing)?;
    if source.keys().any(|field| !properties.contains_key(field)) {
        return Err(McpEntitySchemaRejection::SchemaUnsupported);
    }

    let required = match schema.get("required") {
        None => Vec::new(),
        Some(Value::Array(fields)) => fields
            .iter()
            .map(|field| {
                field
                    .as_str()
                    .map(str::to_string)
                    .ok_or(McpEntitySchemaRejection::SchemaUnsupported)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(McpEntitySchemaRejection::SchemaUnsupported),
    };
    let required: BTreeSet<_> = required.into_iter().collect();
    let identity_field = required
        .iter()
        .find(|field| entity_identity_schema_is_supported(root, properties, field))
        .cloned()
        .ok_or(McpEntitySchemaRejection::IdentityEvidenceMissing)?;
    if source.get(&identity_field).is_none_or(Value::is_null) {
        return Err(McpEntitySchemaRejection::IdentityValueMissing);
    }

    let requested_fields = assess_mcp_entity_requested_fields(tool_input)?;
    if requested_fields
        .iter()
        .any(|field| !properties.contains_key(field) || !source.contains_key(field))
    {
        return Err(McpEntitySchemaRejection::RequestedFieldUnknown);
    }

    let mut protected_fields = required;
    protected_fields.extend(requested_fields.iter().cloned());
    protected_fields.insert(identity_field.clone());
    protected_fields.extend(source.keys().filter_map(|field| {
        let normalized = search_schema_normalized_field(field);
        (entity_status_field(&normalized) || entity_link_field(&normalized)).then(|| field.clone())
    }));

    let mut removable_fields: Vec<_> = source
        .iter()
        .filter_map(|(field, value)| {
            if protected_fields.contains(field)
                || value.is_null()
                || value.is_array()
                || value.is_object()
            {
                return None;
            }
            let normalized = search_schema_normalized_field(field);
            let verbose = value.is_string() && entity_verbose_field(&normalized);
            let protected_duplicate = protected_fields.iter().any(|protected| {
                protected != field && source.get(protected).is_some_and(|other| other == value)
            });
            (verbose || protected_duplicate).then(|| {
                let serialized_chars = serde_json::to_string(value)
                    .map(|value| value.chars().count())
                    .unwrap_or(0)
                    + field.chars().count();
                (field.clone(), serialized_chars, protected_duplicate)
            })
        })
        .collect();
    removable_fields.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });

    Ok(McpEntitySchemaAuthorization {
        identity_field,
        requested_fields,
        protected_fields: protected_fields.into_iter().collect(),
        removable_fields: removable_fields
            .into_iter()
            .map(|(field, _, _)| field)
            .collect(),
    })
}

fn assess_mcp_entity_requested_fields(
    tool_input: Option<&Value>,
) -> Result<Vec<String>, McpEntitySchemaRejection> {
    let Some(tool_input) = tool_input else {
        return Ok(Vec::new());
    };
    if !tool_input.is_object() {
        return Err(McpEntitySchemaRejection::InputSelectorUnsupported);
    }
    let mut nodes = 0usize;
    let mut selectors = Vec::new();
    inspect_entity_input(tool_input, 0, &mut nodes, &mut selectors)?;
    if selectors.len() > 1 {
        return Err(McpEntitySchemaRejection::InputSelectorAmbiguous);
    }
    let Some((_, selector)) = selectors.into_iter().next() else {
        return Ok(Vec::new());
    };
    let raw_fields: Vec<&str> = match selector {
        Value::String(value) => value.split(',').map(str::trim).collect(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or(McpEntitySchemaRejection::InputSelectorUnsupported)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(McpEntitySchemaRejection::InputSelectorUnsupported),
    };
    if raw_fields.is_empty() || raw_fields.len() > MCP_MAX_ENTITY_REQUESTED_FIELDS {
        return Err(McpEntitySchemaRejection::InputSelectorUnsupported);
    }
    let mut fields = BTreeSet::new();
    for field in raw_fields {
        if field.is_empty()
            || field.len() > MCP_MAX_ENTITY_FIELD_NAME_BYTES
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(McpEntitySchemaRejection::InputSelectorUnsupported);
        }
        fields.insert(field.to_string());
    }
    if fields.is_empty() {
        return Err(McpEntitySchemaRejection::InputSelectorUnsupported);
    }
    Ok(fields.into_iter().collect())
}

fn inspect_entity_input<'a>(
    value: &'a Value,
    depth: usize,
    nodes: &mut usize,
    selectors: &mut Vec<(&'a str, &'a Value)>,
) -> Result<(), McpEntitySchemaRejection> {
    *nodes += 1;
    if *nodes > MCP_MAX_ENTITY_INPUT_NODES || depth > MCP_MAX_ENTITY_INPUT_DEPTH {
        return Err(McpEntitySchemaRejection::InputTooLarge);
    }
    match value {
        Value::Object(object) => {
            for (field, child) in object {
                let normalized = search_schema_normalized_field(field);
                if entity_field_selector(&normalized) {
                    if depth != 0 {
                        return Err(McpEntitySchemaRejection::InputSelectorUnsupported);
                    }
                    selectors.push((field, child));
                }
                inspect_entity_input(child, depth + 1, nodes, selectors)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                inspect_entity_input(child, depth + 1, nodes, selectors)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn entity_schema_has_removal_dependencies(schema: &Value) -> bool {
    [
        "allOf",
        "anyOf",
        "oneOf",
        "not",
        "if",
        "then",
        "else",
        "dependentRequired",
        "dependentSchemas",
        "dependencies",
        "minProperties",
        "patternProperties",
        "unevaluatedProperties",
        "propertyNames",
    ]
    .iter()
    .any(|keyword| schema.get(*keyword).is_some())
}

fn entity_identity_schema_is_supported(
    root: &Value,
    properties: &Map<String, Value>,
    field: &str,
) -> bool {
    let normalized = search_schema_normalized_field(field);
    if !matches!(
        normalized.as_str(),
        "id" | "entityid"
            | "recordid"
            | "objectid"
            | "key"
            | "issuekey"
            | "slug"
            | "number"
            | "uri"
            | "url"
    ) {
        return false;
    }
    properties
        .get(field)
        .and_then(|schema| resolve_search_local_schema(root, schema))
        .is_some_and(|schema| search_schema_types_are_subset_of(schema, &["string", "integer"]))
}

fn entity_field_selector(normalized: &str) -> bool {
    matches!(
        normalized,
        "field"
            | "fields"
            | "properties"
            | "select"
            | "include"
            | "columns"
            | "projection"
            | "returnfields"
            | "requestedfields"
    )
}

fn entity_status_field(normalized: &str) -> bool {
    matches!(
        normalized,
        "status" | "state" | "phase" | "lifecycle" | "condition" | "resolution"
    )
}

fn entity_link_field(normalized: &str) -> bool {
    matches!(
        normalized,
        "uri" | "url" | "link" | "permalink" | "htmlurl" | "weburl" | "self" | "selflink"
    )
}

fn entity_verbose_field(normalized: &str) -> bool {
    matches!(
        normalized,
        "description"
            | "longdescription"
            | "body"
            | "content"
            | "text"
            | "details"
            | "notes"
            | "commentary"
            | "markdown"
            | "html"
            | "rendered"
            | "rendereddescription"
            | "renderedbody"
            | "plaintext"
    )
}

pub(crate) fn assess_mcp_search_array_schema(
    root: &Value,
    field: &str,
) -> Result<McpSearchSchemaAuthorization, McpSearchSchemaRejection> {
    let properties = root
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(McpSearchSchemaRejection::ArraySchemaMissing)?;
    let property_schema = properties
        .get(field)
        .ok_or(McpSearchSchemaRejection::ArraySchemaMissing)?;
    let property_schema = resolve_search_local_schema(root, property_schema)
        .ok_or(McpSearchSchemaRejection::ArraySchemaUnsupported)?;
    if !search_schema_types_are_subset_of(property_schema, &["array"]) {
        return Err(McpSearchSchemaRejection::ArraySchemaUnsupported);
    }
    if property_schema.get("prefixItems").is_some() {
        return Err(McpSearchSchemaRejection::PositionalSchemaUnsupported);
    }
    let item_schema = property_schema
        .get("items")
        .and_then(|schema| resolve_search_local_schema(root, schema))
        .ok_or(McpSearchSchemaRejection::ItemSchemaUnsupported)?;
    if !search_schema_types_are_subset_of(item_schema, &["object"]) {
        return Err(McpSearchSchemaRejection::ItemSchemaUnsupported);
    }
    let item_properties = item_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(McpSearchSchemaRejection::ItemSchemaUnsupported)?;
    let mut required: Vec<_> = item_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    required.sort_unstable();
    let identity_field = required
        .iter()
        .find(|field| search_identity_schema_is_supported(root, item_properties, field))
        .ok_or(McpSearchSchemaRejection::IdentityEvidenceMissing)?;
    let match_evidence_field = required
        .iter()
        .find(|field| search_match_schema_is_supported(root, item_properties, field))
        .ok_or(McpSearchSchemaRejection::MatchEvidenceMissing)?;
    let min_results = property_schema
        .get("minItems")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);

    Ok(McpSearchSchemaAuthorization {
        identity_field: (*identity_field).to_string(),
        match_evidence_field: (*match_evidence_field).to_string(),
        min_results,
    })
}

fn resolve_search_local_schema<'a>(root: &'a Value, mut schema: &'a Value) -> Option<&'a Value> {
    for _ in 0..16 {
        let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
            return Some(schema);
        };
        if !reference.starts_with('#') {
            return None;
        }
        schema = root.pointer(&reference[1..])?;
    }
    None
}

fn search_schema_normalized_field(field: &str) -> String {
    field
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn search_identity_schema_is_supported(
    root: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
) -> bool {
    let normalized = search_schema_normalized_field(field);
    if !matches!(
        normalized.as_str(),
        "id" | "resultid"
            | "documentid"
            | "entityid"
            | "uri"
            | "url"
            | "path"
            | "filepath"
            | "filename"
            | "key"
            | "reference"
    ) {
        return false;
    }
    let Some(schema) = properties
        .get(field)
        .and_then(|schema| resolve_search_local_schema(root, schema))
    else {
        return false;
    };
    match normalized.as_str() {
        "uri" | "url" | "path" | "filepath" | "filename" => {
            search_schema_types_are_subset_of(schema, &["string"])
        }
        _ => search_schema_types_are_subset_of(schema, &["string", "integer"]),
    }
}

fn search_match_schema_is_supported(
    root: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
) -> bool {
    let normalized = search_schema_normalized_field(field);
    let allowed: &[&str] = match normalized.as_str() {
        "rank" | "score" | "relevance" | "relevancescore" | "similarity" | "distance" => {
            &["number", "integer"]
        }
        "line" | "linenumber" | "startline" | "endline" | "offset" => &["integer"],
        "snippet" | "match" | "matchedtext" | "highlight" => &["string"],
        "matches" | "highlights" => &["array"],
        "location" | "range" => &["object", "string"],
        _ => return false,
    };
    properties
        .get(field)
        .and_then(|schema| resolve_search_local_schema(root, schema))
        .is_some_and(|schema| search_schema_types_are_subset_of(schema, allowed))
}

fn search_schema_types_are_subset_of(schema: &Value, allowed: &[&str]) -> bool {
    match schema.get("type") {
        Some(Value::String(value)) => allowed.contains(&value.as_str()),
        Some(Value::Array(values)) if !values.is_empty() => values
            .iter()
            .all(|value| value.as_str().is_some_and(|value| allowed.contains(&value))),
        _ => false,
    }
}

pub(crate) fn search_ranked_prefix_indices(total: usize, retained: usize) -> Vec<usize> {
    (0..retained.min(total)).collect()
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

fn maps_equal_except_many(
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
    excluded: &[&str],
) -> bool {
    let is_excluded = |key: &str| excluded.contains(&key);
    let before_len = before
        .keys()
        .filter(|key| !is_excluded(key.as_str()))
        .count();
    let after_len = after
        .keys()
        .filter(|key| !is_excluded(key.as_str()))
        .count();
    before_len == after_len
        && before
            .iter()
            .filter(|(key, _)| !is_excluded(key.as_str()))
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
            structured_content: None,
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
            structured_content: None,
        };
        assert_eq!(
            validate_mcp_proposal(&error, &MANIFEST, &error_proposal),
            Err(McpProposalRejection::ErrorResult)
        );
        assert_eq!(error.render(), *error.raw());
    }

    #[test]
    fn search_schema_assessment_distinguishes_missing_from_incompatible_arrays() {
        let incompatible = json!({
            "type": "object",
            "properties": {
                "results": {"type": "object"}
            }
        });
        assert_eq!(
            assess_mcp_search_array_schema(&incompatible, "results"),
            Err(McpSearchSchemaRejection::ArraySchemaUnsupported)
        );

        let missing = json!({"type": "object", "properties": {}});
        assert_eq!(
            assess_mcp_search_array_schema(&missing, "results"),
            Err(McpSearchSchemaRejection::ArraySchemaMissing)
        );
    }

    #[test]
    fn entity_schema_assessment_protects_required_requested_status_and_links() {
        let schema = json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "title": {"type": "string"},
                "status": {"type": ["string", "null"]},
                "url": {"type": "string"},
                "body": {"type": "string"},
                "description": {"type": "string"},
                "copy": {"type": "string"},
                "nullableCopy": {"type": ["string", "null"]}
            },
            "required": ["id", "title"],
            "additionalProperties": false
        });
        let source = json!({
            "id": "entity-1",
            "title": "Entity",
            "status": null,
            "url": "https://example.invalid/1",
            "body": "requested body",
            "description": "long optional prose",
            "copy": "entity-1",
            "nullableCopy": null
        });
        let authorization = assess_mcp_entity_schema(
            &schema,
            source.as_object().unwrap(),
            Some(&json!({"fields": ["body"]})),
        )
        .expect("entity authorization");
        assert_eq!(authorization.identity_field, "id");
        assert_eq!(authorization.requested_fields, ["body"]);
        for protected in ["id", "title", "status", "url", "body"] {
            assert!(authorization.protected_fields.contains(&protected.into()));
            assert!(!authorization.removable_fields.contains(&protected.into()));
        }
        assert!(authorization
            .removable_fields
            .contains(&"description".into()));
        assert!(authorization.removable_fields.contains(&"copy".into()));
        assert!(!authorization
            .removable_fields
            .contains(&"nullableCopy".into()));
    }

    #[test]
    fn entity_input_and_source_bounds_reject_hostile_shapes() {
        let mut deep = json!({"fields": ["title"]});
        for _ in 0..=MCP_MAX_ENTITY_INPUT_DEPTH {
            deep = json!({"nested": deep});
        }
        let schema = json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "title": {"type": "string"}},
            "required": ["id"],
            "additionalProperties": false
        });
        let source = json!({"id": "entity-1", "title": "Entity"});
        assert_eq!(
            assess_mcp_entity_schema(&schema, source.as_object().unwrap(), Some(&deep)),
            Err(McpEntitySchemaRejection::InputTooLarge)
        );

        let mut wide_source = Map::new();
        wide_source.insert("id".into(), json!("entity-1"));
        for index in 0..MCP_MAX_ENTITY_FIELDS {
            wide_source.insert(format!("field{index}"), json!(index));
        }
        assert_eq!(
            assess_mcp_entity_schema(&schema, &wide_source, None),
            Err(McpEntitySchemaRejection::SourceTooWide)
        );
    }
}
