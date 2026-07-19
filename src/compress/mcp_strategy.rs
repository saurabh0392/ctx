use crate::tool_result::{
    assess_mcp_entity_schema, assess_mcp_search_array_schema, collection_head_tail_indices,
    entity_detail_candidate, entity_detail_text_projection, search_ranked_prefix_indices,
    validate_mcp_output_schema, validate_mcp_proposal_with_contract_and_input,
    CanonicalContentBlock, CanonicalMcpResult, McpEntityDetailEdit, McpEntitySchemaAuthorization,
    McpOutputSchemaValidation, McpPaginatedCollectionEdit, McpProposalRejection,
    McpSearchResultsEdit, McpStrategyManifest, McpStructuredContentEdit,
    McpStructuredContentReplacement, McpTextReplacement, McpTransformProposal, PreservedField,
    ToolContract, ValidatedMcpProposal, MCP_MAX_ENTITY_OMITTED_FIELDS,
    MCP_MAX_RETAINED_COLLECTION_ITEMS, MCP_MAX_RETAINED_SEARCH_RESULTS, MCP_OMISSION_MARKER_FIELD,
};
use serde_json::{Map, Value};

use super::mcp::compress_mcp_output;
use super::types::{CompressContext, CompressOptions};

const TEXT_BLOCK_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "top-level-fields-unchanged",
    "content-block-count-unchanged",
    "non-target-blocks-unchanged",
    "target-text-envelope-unchanged",
    "error-results-pass-through",
    "advertised-output-schema-valid-before-and-after",
    "result-contract-reparses",
];

const PAGINATED_COLLECTION_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "advertised-output-schema-valid-before-and-after",
    "one-schema-backed-top-level-array",
    "pagination-evidence-present",
    "retained-items-value-identical",
    "retained-items-source-ordered",
    "first-and-last-items-retained",
    "non-collection-siblings-unchanged",
    "cursor-total-and-order-fields-unchanged",
    "text-projection-matches-structured-candidate",
    "explicit-omitted-count-marker",
    "error-results-pass-through",
    "result-contract-reparses",
];

const SEARCH_RESULTS_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "advertised-output-schema-valid-before-and-after",
    "one-schema-authorized-search-results-array",
    "stable-result-identity-present",
    "match-ranking-or-location-evidence-present",
    "retained-results-value-identical",
    "retained-results-source-ranked",
    "ranked-prefix-only",
    "non-search-siblings-unchanged",
    "query-count-and-order-fields-unchanged",
    "text-projection-matches-structured-candidate",
    "explicit-omitted-count-marker",
    "error-results-pass-through",
    "result-contract-reparses",
];

const ENTITY_DETAIL_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "advertised-output-schema-valid-before-and-after",
    "one-schema-authorized-entity-object",
    "stable-entity-identity-present",
    "required-requested-status-and-link-fields-protected",
    "retained-field-values-identical",
    "only-optional-verbose-or-proven-duplicate-fields-removed",
    "deterministic-minimal-removal-prefix",
    "nested-values-unchanged",
    "text-projection-matches-structured-candidate",
    "explicit-omitted-field-marker",
    "error-results-pass-through",
    "result-contract-reparses",
];

pub(crate) const MCP_TEXT_BLOCKS_V2: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-text-blocks",
    version: "2",
    eligible_shape: "plain-text-content-blocks",
    invariants: TEXT_BLOCK_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) const MCP_PAGINATED_COLLECTION_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-paginated-collection",
    version: "1",
    eligible_shape: "schema-backed-top-level-paginated-collection",
    invariants: PAGINATED_COLLECTION_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) const MCP_SEARCH_RESULTS_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-search-results",
    version: "1",
    eligible_shape: "schema-backed-ranked-search-results",
    invariants: SEARCH_RESULTS_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) const MCP_ENTITY_DETAIL_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-entity-detail",
    version: "1",
    eligible_shape: "schema-backed-entity-detail",
    invariants: ENTITY_DETAIL_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) struct McpStrategyObservation {
    pub manifest: Option<&'static McpStrategyManifest>,
    pub proposal_attempted: bool,
    pub validated: Option<ValidatedMcpProposal>,
    pub rejection: Option<McpProposalRejection>,
    pub pass_through_reason: Option<&'static str>,
    pub source_schema_validation: Option<McpOutputSchemaValidation>,
    pub candidate_schema_validation: Option<McpOutputSchemaValidation>,
    pub shape_authorization: Option<&'static str>,
}

trait McpResultStrategy: Sync {
    fn manifest(&self) -> &'static McpStrategyManifest;
    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        tool_input: Option<&Value>,
    ) -> McpStrategyEligibility;
    fn propose(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        tool_input: Option<&Value>,
        opts: &CompressOptions,
        ctx: &CompressContext,
    ) -> McpProposalOutcome;
}

enum McpStrategyEligibility {
    NotApplicable,
    Eligible(&'static str),
    Rejected(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpProposalOutcome {
    WithinBudget,
    NoSavings,
    Proposed(Box<McpTransformProposal>),
}

struct TextBlockStrategy;

impl McpResultStrategy for TextBlockStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_TEXT_BLOCKS_V2
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        _contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        if result
            .content
            .iter()
            .any(|block| matches!(block, CanonicalContentBlock::Text { .. }))
        {
            McpStrategyEligibility::Eligible("plain-text-content-block")
        } else {
            McpStrategyEligibility::NotApplicable
        }
    }

    fn propose(
        &self,
        result: &CanonicalMcpResult,
        _contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
        opts: &CompressOptions,
        ctx: &CompressContext,
    ) -> McpProposalOutcome {
        let total_chars: usize = result
            .content
            .iter()
            .filter_map(CanonicalContentBlock::text)
            .map(|text| text.chars().count())
            .sum();
        if total_chars == 0 || total_chars <= opts.target_chars {
            return McpProposalOutcome::WithinBudget;
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
        if replacements.is_empty() {
            McpProposalOutcome::NoSavings
        } else {
            McpProposalOutcome::Proposed(Box::new(McpTransformProposal {
                strategy_id: self.manifest().id,
                strategy_version: self.manifest().version,
                max_total_text_chars: total_budget,
                replacements,
                structured_content: None,
            }))
        }
    }
}

struct PaginatedCollectionStrategy;

impl McpResultStrategy for PaginatedCollectionStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_PAGINATED_COLLECTION_V1
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        match paginated_collection_shape(result, contract) {
            Ok(Some(_)) => McpStrategyEligibility::Eligible("output-schema-and-pagination-fields"),
            Ok(None) => McpStrategyEligibility::NotApplicable,
            Err(reason) => McpStrategyEligibility::Rejected(reason),
        }
    }

    fn propose(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
        opts: &CompressOptions,
        _ctx: &CompressContext,
    ) -> McpProposalOutcome {
        let Ok(Some(shape)) = paginated_collection_shape(result, contract) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_paginated_collection(result, &shape, opts, self.manifest())
    }
}

struct SearchResultsStrategy;

impl McpResultStrategy for SearchResultsStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_SEARCH_RESULTS_V1
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        match search_results_shape(result, contract) {
            Ok(Some(_)) => {
                McpStrategyEligibility::Eligible("output-schema-stable-identity-and-match-evidence")
            }
            Ok(None) => McpStrategyEligibility::NotApplicable,
            Err(reason) => McpStrategyEligibility::Rejected(reason),
        }
    }

    fn propose(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
        opts: &CompressOptions,
        _ctx: &CompressContext,
    ) -> McpProposalOutcome {
        let Ok(Some(shape)) = search_results_shape(result, contract) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_search_results(result, &shape, opts, self.manifest())
    }
}

struct EntityDetailStrategy;

impl McpResultStrategy for EntityDetailStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_ENTITY_DETAIL_V1
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        match entity_detail_shape(result, contract, tool_input) {
            Ok(Some(_)) => McpStrategyEligibility::Eligible(
                "output-schema-identity-and-protected-field-context",
            ),
            Ok(None) => McpStrategyEligibility::NotApplicable,
            Err(reason) => McpStrategyEligibility::Rejected(reason),
        }
    }

    fn propose(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        tool_input: Option<&Value>,
        opts: &CompressOptions,
        _ctx: &CompressContext,
    ) -> McpProposalOutcome {
        let Ok(Some(shape)) = entity_detail_shape(result, contract, tool_input) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_entity_detail(result, &shape, opts, self.manifest())
    }
}

#[derive(Debug)]
struct PaginatedCollectionShape {
    field: String,
    text_block_index: usize,
    min_items: usize,
}

#[derive(Debug)]
struct SearchResultsShape {
    field: String,
    identity_field: String,
    match_evidence_field: String,
    text_block_index: usize,
    min_results: usize,
}

#[derive(Debug)]
struct EntityDetailShape {
    authorization: McpEntitySchemaAuthorization,
    text_block_index: usize,
}

fn entity_detail_shape(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
) -> Result<Option<EntityDetailShape>, &'static str> {
    let PreservedField::Value(Value::Object(structured)) = &result.structured_content else {
        return Ok(None);
    };
    if structured.is_empty() {
        return Ok(None);
    }
    let entity_hint = structured.keys().any(|field| {
        matches!(
            normalized_entity_field(field).as_str(),
            "id" | "entityid" | "recordid" | "objectid" | "key" | "issuekey" | "slug" | "number"
        )
    });
    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return if entity_hint {
            Err("entity-output-schema-required")
        } else {
            Ok(None)
        };
    };
    let authorization = match assess_mcp_entity_schema(schema, structured, tool_input) {
        Ok(authorization) => authorization,
        Err(rejection)
            if !entity_hint && rejection.code() == "entity-identity-evidence-missing" =>
        {
            return Ok(None);
        }
        Err(rejection) => return Err(rejection.code()),
    };
    if structured.contains_key(MCP_OMISSION_MARKER_FIELD) {
        return Err("entity-omission-marker-collision");
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 {
        return Err("entity-text-mirror-required");
    }
    let parsed_text: Value =
        serde_json::from_str(text_blocks[0].1).map_err(|_| "entity-text-mirror-invalid")?;
    if parsed_text != Value::Object(structured.clone()) {
        return Err("entity-text-mirror-invalid");
    }

    Ok(Some(EntityDetailShape {
        authorization,
        text_block_index: text_blocks[0].0,
    }))
}

fn normalized_entity_field(field: &str) -> String {
    field
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn search_results_shape(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
) -> Result<Option<SearchResultsShape>, &'static str> {
    let PreservedField::Value(structured) = &result.structured_content else {
        return Ok(None);
    };
    let Some(structured) = structured.as_object() else {
        return Ok(None);
    };
    let observed_arrays: Vec<_> = structured
        .iter()
        .filter(|(_, value)| value.is_array())
        .map(|(field, _)| field.as_str())
        .collect();
    if observed_arrays.is_empty() {
        return Ok(None);
    }

    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return if observed_arrays
            .iter()
            .any(|field| is_search_result_collection_field(field))
        {
            Err("search-output-schema-required")
        } else {
            Ok(None)
        };
    };
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return if observed_arrays
            .iter()
            .any(|field| is_search_result_collection_field(field))
        {
            Err("search-array-schema-missing")
        } else {
            Ok(None)
        };
    };

    let mut candidates = Vec::new();
    let mut named_rejection = None;
    for field in observed_arrays {
        let search_named = is_search_result_collection_field(field);
        match assess_mcp_search_array_schema(schema, field) {
            Ok(authorization) => candidates.push((field, authorization)),
            Err(rejection) if search_named => {
                named_rejection.get_or_insert(rejection.code());
            }
            Err(_) => {}
        }
    }

    if candidates.len() > 1 {
        return Err("search-array-ambiguous");
    }
    let Some((field, authorization)) = candidates.into_iter().next() else {
        let schema_declares_pagination = structured
            .keys()
            .any(|field| properties.contains_key(field) && is_pagination_field(field.as_str()));
        if schema_declares_pagination {
            return Ok(None);
        }
        return named_rejection.map_or(Ok(None), Err);
    };
    if structured.contains_key(MCP_OMISSION_MARKER_FIELD) {
        return Err("search-omission-marker-collision");
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 {
        return Err("search-text-mirror-required");
    }
    let parsed_text: Value =
        serde_json::from_str(text_blocks[0].1).map_err(|_| "search-text-mirror-invalid")?;
    if parsed_text != Value::Object(structured.clone()) {
        return Err("search-text-mirror-invalid");
    }

    Ok(Some(SearchResultsShape {
        field: field.to_string(),
        identity_field: authorization.identity_field,
        match_evidence_field: authorization.match_evidence_field,
        text_block_index: text_blocks[0].0,
        min_results: authorization.min_results,
    }))
}

fn paginated_collection_shape(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
) -> Result<Option<PaginatedCollectionShape>, &'static str> {
    let PreservedField::Value(structured) = &result.structured_content else {
        return Ok(None);
    };
    let Some(structured) = structured.as_object() else {
        return Ok(None);
    };
    let observed_arrays: Vec<_> = structured
        .iter()
        .filter(|(_, value)| value.is_array())
        .map(|(field, _)| field.as_str())
        .collect();
    if observed_arrays.is_empty() {
        return Ok(None);
    }
    if observed_arrays.len() != 1 {
        return Err("collection-array-ambiguous");
    }

    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return Err("collection-output-schema-required");
    };
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Err("collection-array-schema-missing");
    };
    let field = observed_arrays[0];
    let Some(property_schema) = properties.get(field) else {
        return Err("collection-array-schema-missing");
    };
    let Some(property_schema) = resolve_local_schema(schema, property_schema) else {
        return Err("collection-array-schema-unsupported");
    };
    if !schema_type_includes(property_schema, "array") {
        return Err("collection-array-schema-missing");
    }
    if property_schema.get("prefixItems").is_some() {
        return Err("collection-positional-schema-unsupported");
    }
    let has_pagination_evidence = structured.keys().any(|key| {
        key != field && properties.contains_key(key) && is_pagination_field(key.as_str())
    });
    if !has_pagination_evidence {
        return Err("collection-pagination-evidence-missing");
    }
    if structured.contains_key(MCP_OMISSION_MARKER_FIELD) {
        return Err("collection-omission-marker-collision");
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 {
        return Err("collection-text-mirror-required");
    }
    let parsed_text: Value =
        serde_json::from_str(text_blocks[0].1).map_err(|_| "collection-text-mirror-invalid")?;
    if parsed_text != Value::Object(structured.clone()) {
        return Err("collection-text-mirror-invalid");
    }

    let min_items = property_schema
        .get("minItems")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    Ok(Some(PaginatedCollectionShape {
        field: field.to_string(),
        text_block_index: text_blocks[0].0,
        min_items,
    }))
}

fn resolve_local_schema<'a>(root: &'a Value, mut schema: &'a Value) -> Option<&'a Value> {
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

fn schema_type_includes(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

fn normalized_schema_field(field: &str) -> String {
    field
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_search_result_collection_field(field: &str) -> bool {
    matches!(
        normalized_schema_field(field).as_str(),
        "results" | "searchresults" | "matches" | "hits" | "findings"
    )
}

fn is_pagination_field(field: &str) -> bool {
    let normalized = normalized_schema_field(field);
    matches!(
        normalized.as_str(),
        "nextcursor"
            | "cursor"
            | "continuationtoken"
            | "nextpagetoken"
            | "pageinfo"
            | "total"
            | "totalcount"
            | "hasmore"
            | "hasnextpage"
    )
}

fn propose_paginated_collection(
    result: &CanonicalMcpResult,
    shape: &PaginatedCollectionShape,
    opts: &CompressOptions,
    manifest: &McpStrategyManifest,
) -> McpProposalOutcome {
    let PreservedField::Value(Value::Object(source)) = &result.structured_content else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_items) = source.get(&shape.field).and_then(Value::as_array) else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_text) = result
        .content
        .get(shape.text_block_index)
        .and_then(CanonicalContentBlock::text)
    else {
        return McpProposalOutcome::NoSavings;
    };
    let source_chars = source_text.chars().count();
    if source_chars <= opts.target_chars {
        return McpProposalOutcome::WithinBudget;
    }
    if source_items.len() < 3 {
        return McpProposalOutcome::NoSavings;
    }

    let minimum_retained = shape.min_items.max(2);
    let maximum_retained = (source_items.len() - 1).min(MCP_MAX_RETAINED_COLLECTION_ITEMS);
    if minimum_retained > maximum_retained {
        return McpProposalOutcome::NoSavings;
    }

    let mut best = None;
    let mut low = minimum_retained;
    let mut high = maximum_retained;
    while low <= high {
        let retained = low + (high - low) / 2;
        let indices = collection_head_tail_indices(source_items.len(), retained);
        let candidate = structured_array_candidate(source, &shape.field, source_items, &indices);
        let text_projection =
            collection_text_projection(&candidate, &shape.field, source_items.len(), retained);
        let Ok(text_projection) = serde_json::to_string(&text_projection) else {
            return McpProposalOutcome::NoSavings;
        };
        let chars_out = text_projection.chars().count();
        if chars_out > opts.target_chars.max(1) || chars_out >= source_chars {
            if retained == 0 {
                break;
            }
            high = retained - 1;
            continue;
        }
        best = Some((indices, Value::Object(candidate), text_projection));
        low = retained + 1;
    }
    let Some((retained_indices, replacement, text_projection)) = best else {
        return McpProposalOutcome::NoSavings;
    };

    McpProposalOutcome::Proposed(Box::new(McpTransformProposal {
        strategy_id: manifest.id,
        strategy_version: manifest.version,
        max_total_text_chars: opts.target_chars.max(1),
        replacements: vec![McpTextReplacement {
            block_index: shape.text_block_index,
            expected_text: source_text.to_string(),
            replacement: text_projection,
        }],
        structured_content: Some(McpStructuredContentReplacement {
            expected: Value::Object(source.clone()),
            replacement,
            edit: McpStructuredContentEdit::PaginatedCollection(McpPaginatedCollectionEdit {
                field: shape.field.clone(),
                retained_indices,
                omission_marker_field: MCP_OMISSION_MARKER_FIELD.to_string(),
            }),
        }),
    }))
}

fn propose_search_results(
    result: &CanonicalMcpResult,
    shape: &SearchResultsShape,
    opts: &CompressOptions,
    manifest: &McpStrategyManifest,
) -> McpProposalOutcome {
    let PreservedField::Value(Value::Object(source)) = &result.structured_content else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_results) = source.get(&shape.field).and_then(Value::as_array) else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_text) = result
        .content
        .get(shape.text_block_index)
        .and_then(CanonicalContentBlock::text)
    else {
        return McpProposalOutcome::NoSavings;
    };
    let source_chars = source_text.chars().count();
    if source_chars <= opts.target_chars {
        return McpProposalOutcome::WithinBudget;
    }
    if source_results.len() < 2 {
        return McpProposalOutcome::NoSavings;
    }

    let minimum_retained = shape.min_results.max(1);
    let maximum_retained = (source_results.len() - 1).min(MCP_MAX_RETAINED_SEARCH_RESULTS);
    if minimum_retained > maximum_retained {
        return McpProposalOutcome::NoSavings;
    }

    let mut best = None;
    let mut low = minimum_retained;
    let mut high = maximum_retained;
    while low <= high {
        let retained = low + (high - low) / 2;
        let indices = search_ranked_prefix_indices(source_results.len(), retained);
        let candidate = structured_array_candidate(source, &shape.field, source_results, &indices);
        let text_projection = search_results_text_projection(
            &candidate,
            &shape.field,
            source_results.len(),
            retained,
        );
        let Ok(text_projection) = serde_json::to_string(&text_projection) else {
            return McpProposalOutcome::NoSavings;
        };
        let chars_out = text_projection.chars().count();
        if chars_out > opts.target_chars.max(1) || chars_out >= source_chars {
            if retained == 0 {
                break;
            }
            high = retained - 1;
            continue;
        }
        best = Some((indices, Value::Object(candidate), text_projection));
        low = retained + 1;
    }
    let Some((retained_indices, replacement, text_projection)) = best else {
        return McpProposalOutcome::NoSavings;
    };

    McpProposalOutcome::Proposed(Box::new(McpTransformProposal {
        strategy_id: manifest.id,
        strategy_version: manifest.version,
        max_total_text_chars: opts.target_chars.max(1),
        replacements: vec![McpTextReplacement {
            block_index: shape.text_block_index,
            expected_text: source_text.to_string(),
            replacement: text_projection,
        }],
        structured_content: Some(McpStructuredContentReplacement {
            expected: Value::Object(source.clone()),
            replacement,
            edit: McpStructuredContentEdit::SearchResults(McpSearchResultsEdit {
                field: shape.field.clone(),
                identity_field: shape.identity_field.clone(),
                match_evidence_field: shape.match_evidence_field.clone(),
                retained_indices,
                omission_marker_field: MCP_OMISSION_MARKER_FIELD.to_string(),
            }),
        }),
    }))
}

fn propose_entity_detail(
    result: &CanonicalMcpResult,
    shape: &EntityDetailShape,
    opts: &CompressOptions,
    manifest: &McpStrategyManifest,
) -> McpProposalOutcome {
    let PreservedField::Value(Value::Object(source)) = &result.structured_content else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_text) = result
        .content
        .get(shape.text_block_index)
        .and_then(CanonicalContentBlock::text)
    else {
        return McpProposalOutcome::NoSavings;
    };
    let source_chars = source_text.chars().count();
    if source_chars <= opts.target_chars {
        return McpProposalOutcome::WithinBudget;
    }
    if shape.authorization.removable_fields.is_empty() {
        return McpProposalOutcome::NoSavings;
    }

    let maximum_omitted = shape
        .authorization
        .removable_fields
        .len()
        .min(MCP_MAX_ENTITY_OMITTED_FIELDS);
    let mut best = None;
    for omitted_count in 1..=maximum_omitted {
        let omitted_fields = shape.authorization.removable_fields[..omitted_count].to_vec();
        let candidate = entity_detail_candidate(source, &omitted_fields);
        let projection = entity_detail_text_projection(&candidate, source.len(), &omitted_fields);
        let Ok(text_projection) = serde_json::to_string(&projection) else {
            return McpProposalOutcome::NoSavings;
        };
        let chars_out = text_projection.chars().count();
        if chars_out <= opts.target_chars.max(1) && chars_out < source_chars {
            best = Some((omitted_fields, Value::Object(candidate), text_projection));
            break;
        }
    }
    let Some((omitted_fields, replacement, text_projection)) = best else {
        return McpProposalOutcome::NoSavings;
    };

    McpProposalOutcome::Proposed(Box::new(McpTransformProposal {
        strategy_id: manifest.id,
        strategy_version: manifest.version,
        max_total_text_chars: opts.target_chars.max(1),
        replacements: vec![McpTextReplacement {
            block_index: shape.text_block_index,
            expected_text: source_text.to_string(),
            replacement: text_projection,
        }],
        structured_content: Some(McpStructuredContentReplacement {
            expected: Value::Object(source.clone()),
            replacement,
            edit: McpStructuredContentEdit::EntityDetail(McpEntityDetailEdit {
                identity_field: shape.authorization.identity_field.clone(),
                requested_fields: shape.authorization.requested_fields.clone(),
                omitted_fields,
                omission_marker_field: MCP_OMISSION_MARKER_FIELD.to_string(),
            }),
        }),
    }))
}

fn structured_array_candidate(
    source: &Map<String, Value>,
    field: &str,
    source_items: &[Value],
    retained_indices: &[usize],
) -> Map<String, Value> {
    source
        .iter()
        .map(|(key, value)| {
            if key == field {
                (
                    key.clone(),
                    Value::Array(
                        retained_indices
                            .iter()
                            .map(|index| source_items[*index].clone())
                            .collect(),
                    ),
                )
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

fn search_results_text_projection(
    candidate: &Map<String, Value>,
    field: &str,
    original_results: usize,
    retained_results: usize,
) -> Value {
    let mut projection = candidate.clone();
    projection.insert(
        MCP_OMISSION_MARKER_FIELD.to_string(),
        serde_json::json!({
            "field": field,
            "originalItems": original_results,
            "retainedItems": retained_results,
            "omittedItems": original_results - retained_results,
            "selection": "ranked-prefix"
        }),
    );
    Value::Object(projection)
}

fn collection_text_projection(
    candidate: &Map<String, Value>,
    field: &str,
    original_items: usize,
    retained_items: usize,
) -> Value {
    let mut projection = candidate.clone();
    projection.insert(
        MCP_OMISSION_MARKER_FIELD.to_string(),
        serde_json::json!({
            "field": field,
            "originalItems": original_items,
            "retainedItems": retained_items,
            "omittedItems": original_items - retained_items,
            "selection": "first-and-last"
        }),
    );
    Value::Object(projection)
}

static TEXT_BLOCK_STRATEGY: TextBlockStrategy = TextBlockStrategy;
static PAGINATED_COLLECTION_STRATEGY: PaginatedCollectionStrategy = PaginatedCollectionStrategy;
static SEARCH_RESULTS_STRATEGY: SearchResultsStrategy = SearchResultsStrategy;
static ENTITY_DETAIL_STRATEGY: EntityDetailStrategy = EntityDetailStrategy;
static STRATEGIES: [&dyn McpResultStrategy; 4] = [
    &SEARCH_RESULTS_STRATEGY,
    &PAGINATED_COLLECTION_STRATEGY,
    &ENTITY_DETAIL_STRATEGY,
    &TEXT_BLOCK_STRATEGY,
];

/// Evaluate the deterministic registry in shadow mode. Eligibility is intentionally recorded even
/// when no proposal is useful, and neither state grants permission to apply a trim.
#[cfg(test)]
pub(crate) fn evaluate_mcp_strategies_shadow(
    result: &CanonicalMcpResult,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> McpStrategyObservation {
    evaluate_mcp_strategies_shadow_with_contract(result, None, opts, ctx)
}

#[cfg(test)]
pub(crate) fn evaluate_mcp_strategies_shadow_with_contract(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    opts: &CompressOptions,
    ctx: &CompressContext,
) -> McpStrategyObservation {
    evaluate_mcp_strategies_shadow_with_contract_and_input(result, contract, None, opts, ctx)
}

pub(crate) fn evaluate_mcp_strategies_shadow_with_contract_and_input(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
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
            source_schema_validation: None,
            candidate_schema_validation: None,
            shape_authorization: None,
        };
    }

    let source_schema_validation = validate_mcp_output_schema(contract, result);
    if let McpOutputSchemaValidation::Rejected(rejection) = source_schema_validation {
        return McpStrategyObservation {
            manifest: None,
            proposal_attempted: false,
            validated: None,
            rejection: None,
            pass_through_reason: Some(rejection.code()),
            source_schema_validation: Some(source_schema_validation),
            candidate_schema_validation: None,
            shape_authorization: None,
        };
    }

    for strategy in STRATEGIES {
        let shape_authorization = match strategy.eligibility(result, contract, tool_input) {
            McpStrategyEligibility::NotApplicable => continue,
            McpStrategyEligibility::Rejected(reason) => {
                return McpStrategyObservation {
                    manifest: None,
                    proposal_attempted: false,
                    validated: None,
                    rejection: None,
                    pass_through_reason: Some(reason),
                    source_schema_validation: Some(source_schema_validation),
                    candidate_schema_validation: None,
                    shape_authorization: None,
                }
            }
            McpStrategyEligibility::Eligible(authorization) => authorization,
        };
        let manifest = strategy.manifest();
        let proposal = match strategy.propose(result, contract, tool_input, opts, ctx) {
            McpProposalOutcome::WithinBudget => {
                return McpStrategyObservation {
                    manifest: Some(manifest),
                    proposal_attempted: false,
                    validated: None,
                    rejection: None,
                    pass_through_reason: Some("within-budget"),
                    source_schema_validation: Some(source_schema_validation),
                    candidate_schema_validation: None,
                    shape_authorization: Some(shape_authorization),
                };
            }
            McpProposalOutcome::NoSavings => {
                return McpStrategyObservation {
                    manifest: Some(manifest),
                    proposal_attempted: true,
                    validated: None,
                    rejection: None,
                    pass_through_reason: Some(McpProposalRejection::NoSavings.code()),
                    source_schema_validation: Some(source_schema_validation),
                    candidate_schema_validation: None,
                    shape_authorization: Some(shape_authorization),
                };
            }
            McpProposalOutcome::Proposed(proposal) => proposal,
        };
        return match validate_mcp_proposal_with_contract_and_input(
            result, contract, tool_input, manifest, &proposal,
        ) {
            Ok(validated) => McpStrategyObservation {
                manifest: Some(manifest),
                proposal_attempted: true,
                candidate_schema_validation: Some(if validated.output_schema_validated {
                    McpOutputSchemaValidation::Valid
                } else {
                    McpOutputSchemaValidation::NotAdvertised
                }),
                validated: Some(validated),
                rejection: None,
                pass_through_reason: None,
                source_schema_validation: Some(source_schema_validation),
                shape_authorization: Some(shape_authorization),
            },
            Err(rejection) => {
                let candidate_schema_validation = match rejection {
                    McpProposalRejection::CandidateSchema(schema) => {
                        Some(McpOutputSchemaValidation::Rejected(schema))
                    }
                    _ => None,
                };
                McpStrategyObservation {
                    manifest: Some(manifest),
                    proposal_attempted: true,
                    validated: None,
                    rejection: Some(rejection),
                    pass_through_reason: Some(rejection.code()),
                    source_schema_validation: Some(source_schema_validation),
                    candidate_schema_validation,
                    shape_authorization: Some(shape_authorization),
                }
            }
        };
    }

    McpStrategyObservation {
        manifest: None,
        proposal_attempted: false,
        validated: None,
        rejection: None,
        pass_through_reason: Some("unsupported-shape"),
        source_schema_validation: Some(source_schema_validation),
        candidate_schema_validation: None,
        shape_authorization: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tool_result::{
        parse_mcp_result, validate_mcp_proposal, validate_mcp_proposal_with_contract,
    };

    fn options(target_chars: usize) -> CompressOptions {
        CompressOptions {
            target_chars,
            ..Default::default()
        }
    }

    fn paginated_result(item_count: usize) -> (CanonicalMcpResult, ToolContract) {
        let items: Vec<_> = (0..item_count)
            .map(|index| {
                json!({
                    "id": format!("issue-{index}"),
                    "title": format!("Issue {index}"),
                    "summary": format!(
                        "deterministic payload {index} {}",
                        "repeated context ".repeat(10)
                    )
                })
            })
            .collect();
        let structured = json!({
            "issues": items,
            "nextCursor": "page-2",
            "totalCount": item_count + 20,
            "order": "createdAt desc"
        });
        let text = serde_json::to_string(&structured).expect("serialize fixture");
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false,
            "_meta": {"request": "r1"}
        }))
        .expect("paginated result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {
                    "issues": {
                        "type": "array",
                        "minItems": 2,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "title": {"type": "string"},
                                "summary": {"type": "string"}
                            },
                            "required": ["id", "title", "summary"],
                            "additionalProperties": false
                        }
                    },
                    "nextCursor": {"type": ["string", "null"]},
                    "totalCount": {"type": "integer"},
                    "order": {"type": "string"}
                },
                "required": ["issues", "nextCursor", "totalCount", "order"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        (result, contract)
    }

    fn search_result(result_count: usize) -> (CanonicalMcpResult, ToolContract) {
        let matches: Vec<_> = (0..result_count)
            .map(|index| {
                json!({
                    "path": format!("src/module-{index}.rs"),
                    "line": index + 10,
                    "score": 1_000 - index,
                    "snippet": format!(
                        "ranked source match {index} {}",
                        "relevant surrounding context ".repeat(10)
                    )
                })
            })
            .collect();
        let structured = json!({
            "query": "schema aware trimming",
            "matches": matches,
            "totalMatches": result_count + 50,
            "ranking": "server relevance order"
        });
        let text = serde_json::to_string(&structured).expect("serialize fixture");
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false,
            "_meta": {"request": "search-r1"}
        }))
        .expect("search result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "matches": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"},
                                "line": {"type": "integer"},
                                "score": {"type": "integer"},
                                "snippet": {"type": "string"}
                            },
                            "required": ["path", "line", "score", "snippet"],
                            "additionalProperties": false
                        }
                    },
                    "totalMatches": {"type": "integer"},
                    "ranking": {"type": "string"}
                },
                "required": ["query", "matches", "totalMatches", "ranking"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        (result, contract)
    }

    fn entity_result() -> (CanonicalMcpResult, ToolContract, Value) {
        let structured = json!({
            "id": "entity-42",
            "title": "Schema-aware entity",
            "status": "active",
            "url": "https://example.invalid/entities/42",
            "body": format!("explicitly requested body {}", "requested context ".repeat(16)),
            "description": format!("redundant long description {}", "verbose detail ".repeat(40)),
            "notes": format!("secondary prose {}", "historical note ".repeat(28)),
            "canonicalId": "entity-42",
            "summary": "short unknown optional value stays intact",
            "metadata": {"owner": "team-a", "revision": 7}
        });
        let text = serde_json::to_string(&structured).expect("serialize entity fixture");
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false,
            "_meta": {"request": "entity-r1"}
        }))
        .expect("entity result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "title": {"type": "string"},
                    "status": {"type": "string"},
                    "url": {"type": "string"},
                    "body": {"type": "string"},
                    "description": {"type": "string"},
                    "notes": {"type": "string"},
                    "canonicalId": {"type": "string"},
                    "summary": {"type": "string"},
                    "metadata": {
                        "type": "object",
                        "properties": {
                            "owner": {"type": "string"},
                            "revision": {"type": "integer"}
                        },
                        "required": ["owner", "revision"],
                        "additionalProperties": false
                    }
                },
                "required": ["id", "title", "status", "url"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        let input = json!({"id": "entity-42", "fields": ["title", "body"]});
        (result, contract, input)
    }

    #[test]
    fn registry_is_deterministic_and_versioned() {
        let manifests: Vec<_> = STRATEGIES
            .iter()
            .map(|strategy| strategy.manifest())
            .collect();
        assert_eq!(
            manifests,
            vec![
                &MCP_SEARCH_RESULTS_V1,
                &MCP_PAGINATED_COLLECTION_V1,
                &MCP_ENTITY_DETAIL_V1,
                &MCP_TEXT_BLOCKS_V2
            ]
        );
        assert!(!manifests[0].invariants.is_empty());
    }

    #[test]
    fn entity_proposal_preserves_schema_input_and_semantic_protections() {
        let (result, contract, input) = entity_result();
        let shape = entity_detail_shape(&result, Some(&contract), Some(&input))
            .expect("shape assessment")
            .expect("entity shape");
        assert_eq!(shape.authorization.identity_field, "id");
        assert_eq!(shape.authorization.requested_fields, ["body", "title"]);
        for protected in ["id", "title", "status", "url", "body"] {
            assert!(
                shape
                    .authorization
                    .protected_fields
                    .contains(&protected.to_string()),
                "{protected} should be protected"
            );
        }

        let McpProposalOutcome::Proposed(proposal) =
            propose_entity_detail(&result, &shape, &options(900), &MCP_ENTITY_DETAIL_V1)
        else {
            panic!("expected entity proposal");
        };
        let replacement = proposal
            .structured_content
            .as_ref()
            .expect("structured replacement");
        let source = replacement.expected.as_object().expect("source object");
        let candidate = replacement
            .replacement
            .as_object()
            .expect("candidate object");
        for protected in ["id", "title", "status", "url", "body"] {
            assert_eq!(candidate.get(protected), source.get(protected));
        }
        assert_eq!(candidate.get("metadata"), source.get("metadata"));
        assert_eq!(candidate.get("summary"), source.get("summary"));

        let McpStructuredContentEdit::EntityDetail(edit) = &replacement.edit else {
            panic!("expected entity edit");
        };
        assert!(!edit.omitted_fields.is_empty());
        assert!(edit.omitted_fields.iter().all(|field| {
            ![
                "id", "title", "status", "url", "body", "metadata", "summary",
            ]
            .contains(&field.as_str())
        }));
        let projection: Value =
            serde_json::from_str(&proposal.replacements[0].replacement).expect("JSON projection");
        assert_eq!(
            projection.pointer("/_ctxOmission/selection"),
            Some(&json!("schema-protected-entity-fields"))
        );
        assert_eq!(
            projection.pointer("/_ctxOmission/fields"),
            Some(&json!(edit.omitted_fields))
        );

        let validated = validate_mcp_proposal_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &MCP_ENTITY_DETAIL_V1,
            &proposal,
        )
        .expect("validated entity proposal");
        assert_eq!(validated.entity_fields_in, Some(source.len()));
        assert_eq!(validated.entity_fields_out, Some(candidate.len()));
        assert_eq!(
            validated.entity_fields_omitted,
            Some(source.len() - candidate.len())
        );
        assert_eq!(validated.collection_items_in, None);
        assert_eq!(validated.search_results_in, None);
    }

    #[test]
    fn entity_input_selectors_are_bounded_and_fail_open_when_ambiguous() {
        let (result, contract, _) = entity_result();
        let ambiguous = json!({
            "fields": ["title"],
            "select": ["body"]
        });
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&ambiguous),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, None);
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-field-selector-ambiguous")
        );
        assert!(!observation.proposal_attempted);

        let nested = json!({"request": {"fields": ["title"]}});
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&nested),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-field-selector-unsupported")
        );

        let unknown = json!({"fields": ["secretProjection"]});
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&unknown),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-requested-field-unknown")
        );
    }

    #[test]
    fn stale_or_forged_entity_proposals_are_rejected_inside_the_boundary() {
        let (result, contract, input) = entity_result();
        let shape = entity_detail_shape(&result, Some(&contract), Some(&input))
            .expect("shape assessment")
            .expect("entity shape");
        let McpProposalOutcome::Proposed(proposal) =
            propose_entity_detail(&result, &shape, &options(900), &MCP_ENTITY_DETAIL_V1)
        else {
            panic!("expected entity proposal");
        };

        let mut forged_protected = (*proposal).clone();
        let McpStructuredContentEdit::EntityDetail(edit) = &mut forged_protected
            .structured_content
            .as_mut()
            .expect("structured replacement")
            .edit
        else {
            panic!("expected entity edit");
        };
        edit.omitted_fields = vec!["body".into()];
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_ENTITY_DETAIL_V1,
                &forged_protected,
            ),
            Err(McpProposalRejection::EntitySelectionInvalid)
        );

        let different_input = json!({"fields": ["description"]});
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&different_input),
                &MCP_ENTITY_DETAIL_V1,
                &proposal,
            ),
            Err(McpProposalRejection::EntitySelectionInvalid)
        );

        let mut forged_marker = (*proposal).clone();
        let projection: &mut Value =
            &mut serde_json::from_str(&forged_marker.replacements[0].replacement)
                .expect("projection");
        projection["_ctxOmission"]["omittedFields"] = json!(999);
        forged_marker.replacements[0].replacement =
            serde_json::to_string(projection).expect("serialize projection");
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_ENTITY_DETAIL_V1,
                &forged_marker,
            ),
            Err(McpProposalRejection::EntityOmissionMarkerInvalid)
        );

        let mut changed_identity = (*proposal).clone();
        changed_identity
            .structured_content
            .as_mut()
            .expect("structured replacement")
            .replacement["id"] = json!("forged-id");
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_ENTITY_DETAIL_V1,
                &changed_identity,
            ),
            Err(McpProposalRejection::EntitySelectionInvalid)
        );
    }

    #[test]
    fn entity_marker_collisions_and_nonmirrored_text_fail_open() {
        let (result, contract, input) = entity_result();
        let mut colliding = result.structured_content.value().unwrap().clone();
        colliding["_ctxOmission"] = json!({"server": "owned"});
        let colliding_text = serde_json::to_string(&colliding).unwrap();
        let colliding = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": colliding_text}],
            "structuredContent": colliding,
            "isError": false
        }))
        .unwrap();
        let mut colliding_contract = contract.clone();
        let schema = colliding_contract.output_schema.value().unwrap().clone();
        let mut schema = schema;
        schema["properties"]["_ctxOmission"] = json!({"type": "object"});
        colliding_contract.output_schema = PreservedField::Value(schema);
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &colliding,
            Some(&colliding_contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-omission-marker-collision")
        );

        let structured = result.structured_content.value().unwrap().clone();
        let nonmirrored = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "not the structured entity"}],
            "structuredContent": structured,
            "isError": false
        }))
        .unwrap();
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &nonmirrored,
            Some(&contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-text-mirror-invalid")
        );
    }

    #[test]
    fn generated_entity_budgets_are_deterministic_bounded_and_validated() {
        let (result, contract, input) = entity_result();
        let shape = entity_detail_shape(&result, Some(&contract), Some(&input))
            .expect("shape assessment")
            .expect("entity shape");
        let source_chars = result.content[0].text().unwrap().chars().count();
        for target in (1..source_chars).step_by(37) {
            let first =
                propose_entity_detail(&result, &shape, &options(target), &MCP_ENTITY_DETAIL_V1);
            let second =
                propose_entity_detail(&result, &shape, &options(target), &MCP_ENTITY_DETAIL_V1);
            assert_eq!(first, second, "target {target} must be deterministic");
            let McpProposalOutcome::Proposed(proposal) = first else {
                continue;
            };
            let McpStructuredContentEdit::EntityDetail(edit) = &proposal
                .structured_content
                .as_ref()
                .expect("structured replacement")
                .edit
            else {
                panic!("expected entity edit");
            };
            assert!(edit.omitted_fields.len() <= MCP_MAX_ENTITY_OMITTED_FIELDS);
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_ENTITY_DETAIL_V1,
                &proposal,
            )
            .unwrap_or_else(|error| panic!("target {target} rejected: {error:?}"));
        }
    }

    #[test]
    fn entity_schema_dependencies_and_missing_contracts_fail_open() {
        let (result, mut contract, input) = entity_result();
        let schema = contract.output_schema.value().unwrap().clone();
        let mut dependent_schema = schema;
        dependent_schema["dependentRequired"] = json!({"description": ["notes"]});
        contract.output_schema = PreservedField::Value(dependent_schema);
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("entity-object-schema-unsupported")
        );

        let schema_less = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            None,
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            schema_less.pass_through_reason,
            Some("entity-output-schema-required")
        );
    }

    #[test]
    fn a_url_alone_does_not_block_the_generic_text_fallback() {
        let structured = json!({
            "url": "https://example.invalid/status",
            "description": "ordinary non-entity payload ".repeat(80)
        });
        let text = serde_json::to_string(&structured).unwrap();
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false
        }))
        .unwrap();
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            None,
            Some(&json!({})),
            &options(400),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V2));
        assert_ne!(
            observation.pass_through_reason,
            Some("entity-output-schema-required")
        );
    }

    #[test]
    fn entity_shape_follows_bounded_local_property_references() {
        let (result, mut contract, input) = entity_result();
        let mut schema = contract.output_schema.value().unwrap().clone();
        schema["properties"]["id"] = json!({"$ref": "#/$defs/stableId"});
        schema["properties"]["description"] = json!({"$ref": "#/$defs/prose"});
        schema["$defs"] = json!({
            "stableId": {"type": "string"},
            "prose": {"type": "string"}
        });
        contract.output_schema = PreservedField::Value(schema);
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(observation.pass_through_reason, None);
        assert_eq!(observation.manifest, Some(&MCP_ENTITY_DETAIL_V1));
        assert!(observation.validated.is_some());
        assert_eq!(
            observation.candidate_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
    }

    #[test]
    fn entity_within_budget_and_untrimmable_cases_are_distinct() {
        let (result, contract, input) = entity_result();
        let source_chars = result.content[0].text().unwrap().chars().count();
        let within = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &options(source_chars),
            &CompressContext::default(),
        );
        assert_eq!(within.manifest, Some(&MCP_ENTITY_DETAIL_V1));
        assert_eq!(within.pass_through_reason, Some("within-budget"));
        assert!(!within.proposal_attempted);

        let no_savings = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &options(1),
            &CompressContext::default(),
        );
        assert_eq!(no_savings.manifest, Some(&MCP_ENTITY_DETAIL_V1));
        assert_eq!(no_savings.pass_through_reason, Some("no-savings"));
        assert!(no_savings.proposal_attempted);
    }

    #[test]
    fn search_proposal_preserves_ranked_prefix_identity_and_siblings() {
        let (result, contract) = search_result(10);
        let shape = search_results_shape(&result, Some(&contract))
            .expect("shape assessment")
            .expect("search shape");
        let McpProposalOutcome::Proposed(proposal) =
            propose_search_results(&result, &shape, &options(900), &MCP_SEARCH_RESULTS_V1)
        else {
            panic!("expected search proposal");
        };
        let replacement = proposal
            .structured_content
            .as_ref()
            .expect("structured replacement");
        let source = replacement.expected.as_object().expect("source object");
        let candidate = replacement
            .replacement
            .as_object()
            .expect("candidate object");
        assert_eq!(candidate.get("query"), source.get("query"));
        assert_eq!(candidate.get("totalMatches"), source.get("totalMatches"));
        assert_eq!(candidate.get("ranking"), source.get("ranking"));
        let retained = candidate["matches"].as_array().expect("candidate matches");
        assert!(!retained.is_empty());
        assert!(retained.len() < 10);
        for (index, result) in retained.iter().enumerate() {
            assert_eq!(result, &source["matches"][index]);
        }

        let projection: Value =
            serde_json::from_str(&proposal.replacements[0].replacement).expect("JSON projection");
        assert_eq!(
            projection.pointer("/_ctxOmission/selection"),
            Some(&json!("ranked-prefix"))
        );
        assert_eq!(
            projection.pointer("/_ctxOmission/omittedItems"),
            Some(&json!(10 - retained.len()))
        );
        let validated = validate_mcp_proposal_with_contract(
            &result,
            Some(&contract),
            &MCP_SEARCH_RESULTS_V1,
            &proposal,
        )
        .expect("validated search proposal");
        assert!(validated.output_schema_validated);
        assert!(validated.structured_content_replaced);
        assert_eq!(validated.search_results_in, Some(10));
        assert_eq!(validated.search_results_out, Some(retained.len()));
        assert_eq!(validated.search_results_omitted, Some(10 - retained.len()));
        assert_eq!(validated.collection_items_in, None);
    }

    #[test]
    fn search_shadow_records_schema_authorization_and_content_free_counts() {
        let (result, contract) = search_result(10);
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_SEARCH_RESULTS_V1));
        assert_eq!(
            observation.shape_authorization,
            Some("output-schema-stable-identity-and-match-evidence")
        );
        assert_eq!(
            observation.source_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
        assert_eq!(
            observation.candidate_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
        let validated = observation.validated.expect("validated evidence");
        assert_eq!(validated.search_results_in, Some(10));
        assert!(validated.search_results_omitted.unwrap() > 0);
    }

    #[test]
    fn search_specificity_does_not_block_a_valid_paginated_results_collection() {
        let (result, mut contract) = paginated_result(10);
        let mut structured = result.structured_content.value().unwrap().clone();
        let issues = structured
            .as_object_mut()
            .unwrap()
            .remove("issues")
            .unwrap();
        structured
            .as_object_mut()
            .unwrap()
            .insert("results".into(), issues);
        let text = serde_json::to_string(&structured).unwrap();
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured
        }))
        .unwrap();
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            let issues = schema["properties"]
                .as_object_mut()
                .unwrap()
                .remove("issues")
                .unwrap();
            schema["properties"]
                .as_object_mut()
                .unwrap()
                .insert("results".into(), issues);
            let required = schema["required"].as_array_mut().unwrap();
            let position = required.iter().position(|field| field == "issues").unwrap();
            required[position] = json!("results");
        }
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_PAGINATED_COLLECTION_V1));
        assert!(observation.validated.is_some());
    }

    #[test]
    fn search_shape_follows_local_refs_and_respects_min_items() {
        let (result, mut contract) = search_result(10);
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            let item_schema = schema["properties"]["matches"]["items"].clone();
            schema["$defs"] = json!({"searchResult": item_schema});
            schema["properties"]["matches"]["items"] = json!({"$ref": "#/$defs/searchResult"});
            schema["properties"]["matches"]["minItems"] = json!(4);
        }
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(2_400),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_SEARCH_RESULTS_V1));
        let validated = observation.validated.unwrap_or_else(|| {
            panic!(
                "validated local-ref proposal: pass-through={:?}, rejection={:?}",
                observation.pass_through_reason, observation.rejection
            )
        });
        assert!(validated.search_results_out.unwrap() >= 4);
        assert_eq!(
            observation.candidate_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
    }

    #[test]
    fn large_search_results_keep_bounded_ranked_prefix_evidence() {
        let (result, contract) = search_result(1_000);
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(100_000),
            &CompressContext::default(),
        );
        let validated = observation.validated.expect("bounded large proposal");
        assert_eq!(validated.search_results_in, Some(1_000));
        assert!(validated.search_results_out.unwrap() <= MCP_MAX_RETAINED_SEARCH_RESULTS);
        assert_eq!(
            validated.search_results_omitted,
            Some(1_000 - validated.search_results_out.unwrap())
        );
    }

    #[test]
    fn schema_less_ambiguous_and_unproven_search_shapes_fail_open() {
        let (result, contract) = search_result(10);
        let schema_less = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            None,
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            schema_less.pass_through_reason,
            Some("search-output-schema-required")
        );
        assert!(schema_less.manifest.is_none());

        let mut missing_identity_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut missing_identity_contract.output_schema {
            schema["properties"]["matches"]["items"]["required"] =
                json!(["line", "score", "snippet"]);
        }
        let missing_identity = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&missing_identity_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            missing_identity.pass_through_reason,
            Some("search-identity-evidence-missing")
        );

        let mut missing_match_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut missing_match_contract.output_schema {
            schema["properties"]["matches"]["items"]["required"] = json!(["path"]);
        }
        let missing_match = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&missing_match_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            missing_match.pass_through_reason,
            Some("search-match-evidence-missing")
        );

        let mut nullable_identity_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut nullable_identity_contract.output_schema {
            schema["properties"]["matches"]["items"]["properties"]["path"]["type"] =
                json!(["string", "null"]);
        }
        let nullable_identity = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&nullable_identity_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            nullable_identity.pass_through_reason,
            Some("search-identity-evidence-missing")
        );

        let mut ambiguous_score_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut ambiguous_score_contract.output_schema {
            schema["properties"]["matches"]["items"]["required"] = json!(["path", "score"]);
            schema["properties"]["matches"]["items"]["properties"]["score"]["type"] =
                json!(["integer", "string"]);
        }
        let ambiguous_score = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&ambiguous_score_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            ambiguous_score.pass_through_reason,
            Some("search-match-evidence-missing")
        );

        let mut positional_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut positional_contract.output_schema {
            let item_schema = schema["properties"]["matches"]["items"].clone();
            schema["properties"]["matches"]["prefixItems"] = json!([item_schema]);
        }
        let positional = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&positional_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            positional.pass_through_reason,
            Some("search-positional-schema-unsupported")
        );

        let mut ambiguous_structured = result.structured_content.value().unwrap().clone();
        let duplicate_results = ambiguous_structured["matches"].clone();
        ambiguous_structured
            .as_object_mut()
            .unwrap()
            .insert("hits".into(), duplicate_results);
        let ambiguous_text = serde_json::to_string(&ambiguous_structured).unwrap();
        let ambiguous = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": ambiguous_text}],
            "structuredContent": ambiguous_structured
        }))
        .unwrap();
        let mut ambiguous_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut ambiguous_contract.output_schema {
            schema["properties"]["hits"] = schema["properties"]["matches"].clone();
            schema["required"]
                .as_array_mut()
                .unwrap()
                .push(json!("hits"));
        }
        let ambiguous = evaluate_mcp_strategies_shadow_with_contract(
            &ambiguous,
            Some(&ambiguous_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            ambiguous.pass_through_reason,
            Some("search-array-ambiguous")
        );
    }

    #[test]
    fn stale_or_forged_search_proposals_are_rejected_inside_the_boundary() {
        let (result, contract) = search_result(10);
        let shape = search_results_shape(&result, Some(&contract))
            .unwrap()
            .unwrap();
        let McpProposalOutcome::Proposed(proposal) =
            propose_search_results(&result, &shape, &options(900), &MCP_SEARCH_RESULTS_V1)
        else {
            panic!("expected search proposal");
        };

        let mut stale = proposal.clone();
        stale.structured_content.as_mut().unwrap().expected["query"] = json!("changed");
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &stale,
            ),
            Err(McpProposalRejection::StaleStructuredContent)
        );

        let mut forged_authorization = proposal.clone();
        let McpStructuredContentEdit::SearchResults(edit) = &mut forged_authorization
            .structured_content
            .as_mut()
            .unwrap()
            .edit
        else {
            panic!("expected search edit");
        };
        edit.identity_field = "snippet".into();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &forged_authorization,
            ),
            Err(McpProposalRejection::SearchSchemaAuthorizationInvalid)
        );

        let mut sibling_change = proposal.clone();
        sibling_change
            .structured_content
            .as_mut()
            .unwrap()
            .replacement["totalMatches"] = json!(0);
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &sibling_change,
            ),
            Err(McpProposalRejection::StructuredContentInvariantFailed)
        );

        let mut skipped_rank = proposal.clone();
        let McpStructuredContentEdit::SearchResults(edit) =
            &mut skipped_rank.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected search edit");
        };
        if edit.retained_indices.len() > 1 {
            edit.retained_indices[1] += 1;
        } else {
            edit.retained_indices[0] = 1;
        }
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &skipped_rank,
            ),
            Err(McpProposalRejection::SearchSelectionInvalid)
        );

        let mut forged_marker = proposal.clone();
        let mut projection: Value =
            serde_json::from_str(&forged_marker.replacements[0].replacement).unwrap();
        projection[MCP_OMISSION_MARKER_FIELD]["omittedItems"] = json!(999);
        forged_marker.replacements[0].replacement = serde_json::to_string(&projection).unwrap();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &forged_marker,
            ),
            Err(McpProposalRejection::SearchOmissionMarkerInvalid)
        );

        let mut renamed_marker = proposal.clone();
        let McpStructuredContentEdit::SearchResults(edit) =
            &mut renamed_marker.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected search edit");
        };
        edit.omission_marker_field = "_differentMarker".into();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_SEARCH_RESULTS_V1,
                &renamed_marker,
            ),
            Err(McpProposalRejection::SearchOmissionMarkerInvalid)
        );
    }

    #[test]
    fn search_marker_collisions_and_nonmirrored_text_fail_open() {
        let (result, contract) = search_result(10);
        let nonmirrored = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "prose search summary"}],
            "structuredContent": result.structured_content.value().unwrap().clone()
        }))
        .unwrap();
        let nonmirrored = evaluate_mcp_strategies_shadow_with_contract(
            &nonmirrored,
            Some(&contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            nonmirrored.pass_through_reason,
            Some("search-text-mirror-invalid")
        );

        let mut colliding_structured = result.structured_content.value().unwrap().clone();
        colliding_structured
            .as_object_mut()
            .unwrap()
            .insert(MCP_OMISSION_MARKER_FIELD.into(), json!({"server": true}));
        let colliding_text = serde_json::to_string(&colliding_structured).unwrap();
        let colliding = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": colliding_text}],
            "structuredContent": colliding_structured
        }))
        .unwrap();
        let mut colliding_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut colliding_contract.output_schema {
            schema["properties"][MCP_OMISSION_MARKER_FIELD] = json!({"type": "object"});
            schema["required"]
                .as_array_mut()
                .unwrap()
                .push(json!(MCP_OMISSION_MARKER_FIELD));
        }
        let colliding = evaluate_mcp_strategies_shadow_with_contract(
            &colliding,
            Some(&colliding_contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            colliding.pass_through_reason,
            Some("search-omission-marker-collision")
        );
    }

    #[test]
    fn search_candidate_schema_constraints_fail_open_after_structural_validation() {
        let (result, mut contract) = search_result(10);
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            schema["properties"]["matches"]["contains"] = json!({
                "type": "object",
                "properties": {"path": {"const": "src/module-5.rs"}},
                "required": ["path"]
            });
        }
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.rejection,
            Some(McpProposalRejection::CandidateSchema(
                crate::tool_result::McpSchemaRejection::InstanceInvalid
            ))
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("candidate-structured-content-schema-mismatch")
        );
        assert!(observation.validated.is_none());
    }

    #[test]
    fn search_within_budget_and_untrimmable_cases_are_distinct() {
        let (large, contract) = search_result(10);
        let source_chars = large.content[0].text().unwrap().chars().count();
        let within = evaluate_mcp_strategies_shadow_with_contract(
            &large,
            Some(&contract),
            &options(source_chars),
            &CompressContext::default(),
        );
        assert_eq!(within.manifest, Some(&MCP_SEARCH_RESULTS_V1));
        assert!(!within.proposal_attempted);
        assert_eq!(within.pass_through_reason, Some("within-budget"));

        let (one_result, one_result_contract) = search_result(1);
        let no_savings = evaluate_mcp_strategies_shadow_with_contract(
            &one_result,
            Some(&one_result_contract),
            &options(1),
            &CompressContext::default(),
        );
        assert_eq!(no_savings.manifest, Some(&MCP_SEARCH_RESULTS_V1));
        assert!(no_savings.proposal_attempted);
        assert_eq!(no_savings.pass_through_reason, Some("no-savings"));
    }

    #[test]
    fn generated_ranked_prefix_selections_are_deterministic_ordered_and_bounded() {
        for total in 2_usize..=512 {
            for retained in 1..=total.saturating_sub(1).min(MCP_MAX_RETAINED_SEARCH_RESULTS) {
                let first = search_ranked_prefix_indices(total, retained);
                let second = search_ranked_prefix_indices(total, retained);
                assert_eq!(first, second);
                assert_eq!(first.len(), retained);
                assert_eq!(first.first(), Some(&0));
                assert_eq!(first.last(), Some(&(retained - 1)));
                assert!(first.windows(2).all(|pair| pair[0] < pair[1]));
            }
        }
    }

    #[test]
    fn paginated_collection_proposal_preserves_items_order_and_siblings() {
        let (result, contract) = paginated_result(10);
        let shape = paginated_collection_shape(&result, Some(&contract))
            .expect("shape assessment")
            .expect("paginated shape");
        let McpProposalOutcome::Proposed(proposal) = propose_paginated_collection(
            &result,
            &shape,
            &options(850),
            &MCP_PAGINATED_COLLECTION_V1,
        ) else {
            panic!("expected collection proposal");
        };
        let replacement = proposal
            .structured_content
            .as_ref()
            .expect("structured replacement");
        let source = replacement.expected.as_object().expect("source object");
        let candidate = replacement
            .replacement
            .as_object()
            .expect("candidate object");
        assert_eq!(candidate.get("nextCursor"), source.get("nextCursor"));
        assert_eq!(candidate.get("totalCount"), source.get("totalCount"));
        assert_eq!(candidate.get("order"), source.get("order"));
        let retained = candidate["issues"].as_array().expect("candidate issues");
        assert!(retained.len() >= 2);
        assert!(retained.len() < 10);
        assert_eq!(retained.first().unwrap()["id"], "issue-0");
        assert_eq!(retained.last().unwrap()["id"], "issue-9");

        let projection: Value =
            serde_json::from_str(&proposal.replacements[0].replacement).expect("JSON projection");
        assert_eq!(
            projection.pointer("/_ctxOmission/originalItems"),
            Some(&json!(10))
        );
        assert_eq!(
            projection.pointer("/_ctxOmission/omittedItems"),
            Some(&json!(10 - retained.len()))
        );
        let validated = validate_mcp_proposal_with_contract(
            &result,
            Some(&contract),
            &MCP_PAGINATED_COLLECTION_V1,
            &proposal,
        )
        .expect("validated collection proposal");
        assert!(validated.output_schema_validated);
        assert!(validated.structured_content_replaced);
        assert_eq!(validated.collection_items_in, Some(10));
        assert_eq!(validated.collection_items_out, Some(retained.len()));
        assert_eq!(
            validated.collection_items_omitted,
            Some(10 - retained.len())
        );
    }

    #[test]
    fn collection_shadow_records_schema_authorization_and_content_free_counts() {
        let (result, contract) = paginated_result(10);
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_PAGINATED_COLLECTION_V1));
        assert_eq!(
            observation.shape_authorization,
            Some("output-schema-and-pagination-fields")
        );
        assert_eq!(
            observation.source_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
        assert_eq!(
            observation.candidate_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
        let validated = observation.validated.expect("validated evidence");
        assert_eq!(validated.collection_items_in, Some(10));
        assert!(validated.collection_items_omitted.unwrap() > 0);
    }

    #[test]
    fn collection_shape_follows_local_schema_refs_and_respects_min_items() {
        let (result, mut contract) = paginated_result(10);
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            let collection_schema = schema["properties"]["issues"].clone();
            schema["$defs"] = json!({"collection": collection_schema});
            schema["properties"]["issues"] = json!({"$ref": "#/$defs/collection"});
            schema["$defs"]["collection"]["minItems"] = json!(4);
        }
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(1_400),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_PAGINATED_COLLECTION_V1));
        let validated = observation.validated.expect("validated local-ref proposal");
        assert!(validated.collection_items_out.unwrap() >= 4);
        assert_eq!(
            observation.candidate_schema_validation,
            Some(McpOutputSchemaValidation::Valid)
        );
    }

    #[test]
    fn large_collections_keep_bounded_evidence_and_selection_work() {
        let (result, contract) = paginated_result(1_000);
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(100_000),
            &CompressContext::default(),
        );
        let validated = observation.validated.expect("bounded large proposal");
        assert_eq!(validated.collection_items_in, Some(1_000));
        assert!(validated.collection_items_out.unwrap() <= MCP_MAX_RETAINED_COLLECTION_ITEMS);
        assert_eq!(
            validated.collection_items_omitted,
            Some(1_000 - validated.collection_items_out.unwrap())
        );
    }

    #[test]
    fn collection_within_budget_and_untrimmable_cases_are_distinct() {
        let (large, large_contract) = paginated_result(10);
        let source_chars = large.content[0].text().unwrap().chars().count();
        let within = evaluate_mcp_strategies_shadow_with_contract(
            &large,
            Some(&large_contract),
            &options(source_chars),
            &CompressContext::default(),
        );
        assert_eq!(within.manifest, Some(&MCP_PAGINATED_COLLECTION_V1));
        assert!(!within.proposal_attempted);
        assert_eq!(within.pass_through_reason, Some("within-budget"));

        let (two_items, two_item_contract) = paginated_result(2);
        let no_savings = evaluate_mcp_strategies_shadow_with_contract(
            &two_items,
            Some(&two_item_contract),
            &options(1),
            &CompressContext::default(),
        );
        assert_eq!(no_savings.manifest, Some(&MCP_PAGINATED_COLLECTION_V1));
        assert!(no_savings.proposal_attempted);
        assert_eq!(no_savings.pass_through_reason, Some("no-savings"));
    }

    #[test]
    fn ambiguous_schema_less_and_unpaginated_collections_fail_open() {
        let (result, contract) = paginated_result(10);
        let schema_less = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            None,
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(
            schema_less.pass_through_reason,
            Some("collection-output-schema-required")
        );
        assert!(schema_less.manifest.is_none());

        let mut ambiguous_structured = result
            .structured_content
            .value()
            .expect("structured")
            .clone();
        ambiguous_structured
            .as_object_mut()
            .unwrap()
            .insert("warnings".into(), json!(["one", "two"]));
        let ambiguous_text = serde_json::to_string(&ambiguous_structured).unwrap();
        let ambiguous = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": ambiguous_text}],
            "structuredContent": ambiguous_structured
        }))
        .unwrap();
        let mut ambiguous_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut ambiguous_contract.output_schema {
            schema["properties"]["warnings"] =
                json!({"type": "array", "items": {"type": "string"}});
        }
        let ambiguous = evaluate_mcp_strategies_shadow_with_contract(
            &ambiguous,
            Some(&ambiguous_contract),
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(
            ambiguous.pass_through_reason,
            Some("collection-array-ambiguous")
        );

        let (unpaginated, mut unpaginated_contract) = paginated_result(10);
        let mut structured = unpaginated.structured_content.value().unwrap().clone();
        let object = structured.as_object_mut().unwrap();
        object.remove("nextCursor");
        object.remove("totalCount");
        let text = serde_json::to_string(&structured).unwrap();
        let unpaginated = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured
        }))
        .unwrap();
        if let PreservedField::Value(schema) = &mut unpaginated_contract.output_schema {
            let properties = schema["properties"].as_object_mut().unwrap();
            properties.remove("nextCursor");
            properties.remove("totalCount");
            schema["required"] = json!(["issues", "order"]);
        }
        let unpaginated = evaluate_mcp_strategies_shadow_with_contract(
            &unpaginated,
            Some(&unpaginated_contract),
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(
            unpaginated.pass_through_reason,
            Some("collection-pagination-evidence-missing")
        );

        let (positional, mut positional_contract) = paginated_result(10);
        if let PreservedField::Value(schema) = &mut positional_contract.output_schema {
            let item_schema = schema["properties"]["issues"]["items"].clone();
            schema["properties"]["issues"]["prefixItems"] = json!([item_schema]);
        }
        let positional = evaluate_mcp_strategies_shadow_with_contract(
            &positional,
            Some(&positional_contract),
            &options(850),
            &CompressContext::default(),
        );
        assert_eq!(
            positional.pass_through_reason,
            Some("collection-positional-schema-unsupported")
        );
    }

    #[test]
    fn stale_or_forged_collection_proposals_are_rejected_inside_the_boundary() {
        let (result, contract) = paginated_result(10);
        let shape = paginated_collection_shape(&result, Some(&contract))
            .unwrap()
            .unwrap();
        let McpProposalOutcome::Proposed(proposal) = propose_paginated_collection(
            &result,
            &shape,
            &options(850),
            &MCP_PAGINATED_COLLECTION_V1,
        ) else {
            panic!("expected collection proposal");
        };

        let mut stale = proposal.clone();
        stale.structured_content.as_mut().unwrap().expected["order"] = json!("changed");
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_PAGINATED_COLLECTION_V1,
                &stale,
            ),
            Err(McpProposalRejection::StaleStructuredContent)
        );
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                None,
                &MCP_PAGINATED_COLLECTION_V1,
                &proposal,
            ),
            Err(McpProposalRejection::StructuredContentSchemaRequired)
        );

        let mut sibling_change = proposal.clone();
        sibling_change
            .structured_content
            .as_mut()
            .unwrap()
            .replacement["nextCursor"] = json!("forged");
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_PAGINATED_COLLECTION_V1,
                &sibling_change,
            ),
            Err(McpProposalRejection::StructuredContentInvariantFailed)
        );

        let mut reordered = proposal.clone();
        let McpStructuredContentEdit::PaginatedCollection(edit) =
            &mut reordered.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected paginated collection edit");
        };
        edit.retained_indices.reverse();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_PAGINATED_COLLECTION_V1,
                &reordered,
            ),
            Err(McpProposalRejection::CollectionSelectionInvalid)
        );

        let mut forged_marker = proposal.clone();
        let mut projection: Value =
            serde_json::from_str(&forged_marker.replacements[0].replacement).unwrap();
        projection[MCP_OMISSION_MARKER_FIELD]["omittedItems"] = json!(999);
        forged_marker.replacements[0].replacement = serde_json::to_string(&projection).unwrap();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_PAGINATED_COLLECTION_V1,
                &forged_marker,
            ),
            Err(McpProposalRejection::CollectionOmissionMarkerInvalid)
        );

        let mut renamed_marker = proposal.clone();
        let McpStructuredContentEdit::PaginatedCollection(edit) =
            &mut renamed_marker.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected paginated collection edit");
        };
        edit.omission_marker_field = "_differentMarker".into();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_PAGINATED_COLLECTION_V1,
                &renamed_marker,
            ),
            Err(McpProposalRejection::CollectionOmissionMarkerInvalid)
        );
    }

    #[test]
    fn candidate_schema_constraints_fail_open_after_structural_validation() {
        let (result, mut contract) = paginated_result(10);
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            schema["properties"]["issues"]["contains"] = json!({
                "type": "object",
                "properties": {"id": {"const": "issue-5"}},
                "required": ["id"]
            });
        }
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.rejection,
            Some(McpProposalRejection::CandidateSchema(
                crate::tool_result::McpSchemaRejection::InstanceInvalid
            ))
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("candidate-structured-content-schema-mismatch")
        );
        assert!(observation.validated.is_none());
    }

    #[test]
    fn generated_head_tail_selections_are_deterministic_ordered_and_bounded() {
        for total in 3_usize..=512 {
            for retained in 2..=total
                .saturating_sub(1)
                .min(MCP_MAX_RETAINED_COLLECTION_ITEMS)
            {
                let first = collection_head_tail_indices(total, retained);
                let second = collection_head_tail_indices(total, retained);
                assert_eq!(first, second);
                assert_eq!(first.len(), retained);
                assert_eq!(first.first(), Some(&0));
                assert_eq!(first.last(), Some(&(total - 1)));
                assert!(first.windows(2).all(|pair| pair[0] < pair[1]));
            }
        }
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
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V2));
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
        let McpProposalOutcome::Proposed(proposal) =
            TEXT_BLOCK_STRATEGY.propose(&result, None, None, &opts, &CompressContext::default())
        else {
            panic!("expected block-aware proposal");
        };
        let targets: Vec<_> = proposal
            .replacements
            .iter()
            .map(|replacement| replacement.block_index)
            .collect();
        assert_eq!(targets, vec![0, 2]);
        let validated = validate_mcp_proposal(&result, &MCP_TEXT_BLOCKS_V2, &proposal)
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
    fn error_results_outrank_even_a_malformed_advertised_schema() {
        let error = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "actionable failure"}],
            "isError": true
        }))
        .expect("error result");
        let malformed_contract = ToolContract {
            output_schema: PreservedField::Opaque(json!("future-schema")),
            ..Default::default()
        };
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &error,
            Some(&malformed_contract),
            &options(1),
            &CompressContext::default(),
        );
        assert_eq!(observation.pass_through_reason, Some("error-result"));
        assert!(observation.source_schema_validation.is_none());
        assert!(observation.manifest.is_none());
        assert!(!observation.proposal_attempted);
    }

    #[test]
    fn schema_invalid_sources_pass_through_before_strategy_selection() {
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "long repeated text long repeated text"}],
            "structuredContent": {"items": [1, 2, 3]}
        }))
        .expect("result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {"items": {"type": "array", "items": {"type": "string"}}},
                "required": ["items"]
            })),
            ..Default::default()
        };
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(1),
            &CompressContext::default(),
        );
        assert_eq!(
            observation.source_schema_validation,
            Some(McpOutputSchemaValidation::Rejected(
                crate::tool_result::McpSchemaRejection::InstanceInvalid
            ))
        );
        assert_eq!(
            observation.pass_through_reason,
            Some("structured-content-schema-mismatch")
        );
        assert!(observation.manifest.is_none());
        assert!(!observation.proposal_attempted);
    }

    #[test]
    fn eligibility_is_recorded_separately_when_no_proposal_is_needed() {
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "already small"}]
        }))
        .expect("small result");
        let observation =
            evaluate_mcp_strategies_shadow(&result, &options(100), &CompressContext::default());
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V2));
        assert!(!observation.proposal_attempted);
        assert!(observation.validated.is_none());
        assert_eq!(observation.pass_through_reason, Some("within-budget"));
    }

    #[test]
    fn over_budget_result_without_savings_is_not_labeled_within_budget() {
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "x"}]
        }))
        .expect("one-character result");
        let observation =
            evaluate_mcp_strategies_shadow(&result, &options(0), &CompressContext::default());
        assert_eq!(observation.manifest, Some(&MCP_TEXT_BLOCKS_V2));
        assert!(observation.proposal_attempted);
        assert!(observation.validated.is_none());
        assert!(observation.rejection.is_none());
        assert_eq!(observation.pass_through_reason, Some("no-savings"));
    }
}
