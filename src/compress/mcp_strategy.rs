use crate::tool_result::{
    assess_mcp_entity_schema, assess_mcp_search_array_schema, assess_mcp_table_schema,
    assess_mcp_tree_listing_schema, collection_head_tail_indices, entity_detail_candidate,
    entity_detail_text_projection, search_ranked_prefix_indices, table_rows_candidate,
    table_rows_text_projection, tree_listing_candidate, tree_listing_text_projection,
    validate_mcp_output_schema, validate_mcp_proposal_with_contract_and_input,
    CanonicalContentBlock, CanonicalMcpResult, McpEntityDetailEdit, McpEntitySchemaAuthorization,
    McpOutputSchemaValidation, McpPaginatedCollectionEdit, McpProposalRejection,
    McpSearchResultsEdit, McpStrategyManifest, McpStructuredContentEdit,
    McpStructuredContentReplacement, McpTableRowsEdit, McpTableSchemaAuthorization,
    McpTextReplacement, McpTransformProposal, McpTreeListingEdit, McpTreeSchemaAuthorization,
    PreservedField, ToolContract, ValidatedMcpProposal, MCP_MAX_ENTITY_OMITTED_FIELDS,
    MCP_MAX_RETAINED_COLLECTION_ITEMS, MCP_MAX_RETAINED_SEARCH_RESULTS,
    MCP_MAX_RETAINED_TABLE_ROWS, MCP_MAX_TREE_OMITTED_ENTRIES, MCP_OMISSION_MARKER_FIELD,
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

const TREE_LISTING_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "advertised-output-schema-valid-before-and-after",
    "one-schema-authorized-rooted-flat-listing",
    "stable-root-path-and-entry-kind-present",
    "requested-root-and-depth-context-rederived",
    "normal-source-entries-protected",
    "only-generated-vendor-descendants-outside-requested-depth-removed",
    "retained-entries-value-identical-and-source-ordered",
    "deterministic-minimal-removal-prefix",
    "text-projection-matches-structured-candidate",
    "explicit-content-free-omitted-entry-marker",
    "error-results-pass-through",
    "result-contract-reparses",
];

const TABLE_ROWS_INVARIANTS: &[&str] = &[
    "source-round-trip-identical",
    "advertised-output-schema-valid-before-and-after",
    "one-schema-authorized-columns-and-rows-table",
    "non-empty-unique-bounded-columns-preserved",
    "rectangular-scalar-rows-only",
    "retained-rows-value-identical-and-source-ordered",
    "first-and-last-source-rows-retained",
    "largest-fitting-retained-row-count",
    "non-row-siblings-unchanged",
    "text-projection-matches-structured-candidate",
    "explicit-content-free-omitted-row-marker",
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

pub(crate) const MCP_TREE_LISTING_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-tree-listing",
    version: "1",
    eligible_shape: "schema-backed-rooted-flat-tree-listing",
    invariants: TREE_LISTING_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) const MCP_TABLE_ROWS_V1: McpStrategyManifest = McpStrategyManifest {
    id: "mcp-table-rows",
    version: "1",
    eligible_shape: "schema-backed-rectangular-scalar-table",
    invariants: TABLE_ROWS_INVARIANTS,
    max_expansion_percent: 100,
};

pub(crate) struct McpStrategyObservation {
    pub manifest: Option<&'static McpStrategyManifest>,
    /// Contentful and process-local. Shadow callers ignore it; the T3 apply transaction consumes it
    /// only after the evidence gate grants permission.
    pub proposal: Option<McpTransformProposal>,
    pub proposal_attempted: bool,
    pub validated: Option<ValidatedMcpProposal>,
    pub rejection: Option<McpProposalRejection>,
    pub pass_through_reason: Option<&'static str>,
    pub source_schema_validation: Option<McpOutputSchemaValidation>,
    pub candidate_schema_validation: Option<McpOutputSchemaValidation>,
    pub shape_authorization: Option<&'static str>,
}

pub(crate) struct McpApplyCandidate {
    pub manifest: &'static McpStrategyManifest,
    pub proposal: McpTransformProposal,
}

/// Contentful adapter entry point. It uses the exact same deterministic registry and validators as
/// shadow mode; permission remains the controller's separate responsibility.
pub(crate) fn propose_mcp_apply_candidate(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: &Value,
    cfg: &crate::config::Config,
    cwd: &str,
) -> Result<McpApplyCandidate, &'static str> {
    let options = CompressOptions {
        max_input_chars: cfg.compress_max_output_chars,
        target_chars: cfg.compress_target_chars,
        redact_secrets: cfg.compress_redact_secrets,
        preserve_errors: cfg.compress_preserve_errors,
    };
    let context = CompressContext {
        cwd: cwd.to_owned(),
        prompt_keywords: Vec::new(),
    };
    let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
        result,
        contract,
        Some(tool_input),
        &options,
        &context,
    );
    match (observation.manifest, observation.proposal) {
        (Some(manifest), Some(proposal)) => Ok(McpApplyCandidate { manifest, proposal }),
        _ => Err(observation
            .pass_through_reason
            .unwrap_or("unsupported-shape")),
    }
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

struct TreeListingStrategy;

impl McpResultStrategy for TreeListingStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_TREE_LISTING_V1
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        match tree_listing_shape(result, contract, tool_input) {
            Ok(Some(_)) => McpStrategyEligibility::Eligible(
                "output-schema-root-path-kind-and-bounded-request-context",
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
        let Ok(Some(shape)) = tree_listing_shape(result, contract, tool_input) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_tree_listing(result, &shape, opts, self.manifest())
    }
}

struct TableRowsStrategy;

impl McpResultStrategy for TableRowsStrategy {
    fn manifest(&self) -> &'static McpStrategyManifest {
        &MCP_TABLE_ROWS_V1
    }

    fn eligibility(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        _tool_input: Option<&Value>,
    ) -> McpStrategyEligibility {
        match table_rows_shape(result, contract) {
            Ok(Some(_)) => {
                McpStrategyEligibility::Eligible("output-schema-rectangular-scalar-table")
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
        let Ok(Some(shape)) = table_rows_shape(result, contract) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_table_rows(result, &shape, opts, self.manifest())
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

#[derive(Debug)]
struct TreeListingShape {
    authorization: McpTreeSchemaAuthorization,
    text_block_index: usize,
}

#[derive(Debug)]
struct TableRowsShape {
    authorization: McpTableSchemaAuthorization,
    text_block_index: usize,
}

fn tree_listing_shape(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
    tool_input: Option<&Value>,
) -> Result<Option<TreeListingShape>, &'static str> {
    let PreservedField::Value(Value::Object(structured)) = &result.structured_content else {
        return Ok(None);
    };
    let observed_arrays: Vec<_> = structured
        .iter()
        .filter(|(_, value)| value.is_array())
        .collect();
    if observed_arrays.is_empty() {
        return Ok(None);
    }
    let tree_hint = structured.keys().any(|field| {
        matches!(
            normalized_schema_field(field).as_str(),
            "root" | "rootpath" | "base" | "basepath" | "cwd" | "directory" | "directorypath"
        )
    }) && observed_arrays.iter().any(|(field, entries)| {
        is_tree_entries_field(field)
            || entries
                .as_array()
                .and_then(|entries| entries.first())
                .and_then(Value::as_object)
                .is_some_and(|entry| {
                    let has_path = entry.keys().any(|field| {
                        matches!(
                            normalized_schema_field(field).as_str(),
                            "path" | "relativepath" | "filepath" | "name"
                        )
                    });
                    let has_kind = entry.keys().any(|field| {
                        matches!(
                            normalized_schema_field(field).as_str(),
                            "kind" | "type" | "entrytype" | "filetype"
                        )
                    });
                    has_path && has_kind
                })
    });
    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return if tree_hint {
            Err("tree-output-schema-required")
        } else {
            Ok(None)
        };
    };
    if structured.keys().any(|field| is_pagination_field(field)) {
        return Ok(None);
    }
    let authorization = match assess_mcp_tree_listing_schema(schema, structured, tool_input) {
        Ok(authorization) => authorization,
        Err(rejection)
            if !tree_hint
                && matches!(
                    rejection.code(),
                    "tree-root-evidence-missing" | "tree-entries-evidence-missing"
                ) =>
        {
            return Ok(None);
        }
        Err(rejection) => return Err(rejection.code()),
    };
    if structured.contains_key(MCP_OMISSION_MARKER_FIELD) {
        return Err("tree-omission-marker-collision");
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 {
        return Err("tree-text-mirror-required");
    }
    let parsed_text: Value =
        serde_json::from_str(text_blocks[0].1).map_err(|_| "tree-text-mirror-invalid")?;
    if parsed_text != Value::Object(structured.clone()) {
        return Err("tree-text-mirror-invalid");
    }

    Ok(Some(TreeListingShape {
        authorization,
        text_block_index: text_blocks[0].0,
    }))
}

fn table_rows_shape(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
) -> Result<Option<TableRowsShape>, &'static str> {
    let PreservedField::Value(Value::Object(structured)) = &result.structured_content else {
        return if raw_delimited_table_hint(result) {
            Err("table-raw-delimited-text-unsupported")
        } else {
            Ok(None)
        };
    };
    let table_hint = structured.keys().any(|field| {
        matches!(
            normalized_schema_field(field).as_str(),
            "columns" | "headers"
        )
    }) && structured
        .keys()
        .any(|field| matches!(normalized_schema_field(field).as_str(), "rows" | "data"));
    if !table_hint {
        return Ok(None);
    }
    let Some(schema) = contract.and_then(|contract| contract.output_schema.value()) else {
        return Err("table-output-schema-required");
    };
    if structured.keys().any(|field| is_pagination_field(field)) {
        return Ok(None);
    }
    let authorization =
        assess_mcp_table_schema(schema, structured).map_err(|rejection| rejection.code())?;
    if structured.contains_key(MCP_OMISSION_MARKER_FIELD) {
        return Err("table-omission-marker-collision");
    }

    let text_blocks: Vec<_> = result
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, block)| block.text().map(|text| (index, text)))
        .collect();
    if text_blocks.len() != 1 {
        return Err("table-text-mirror-required");
    }
    let parsed_text: Value =
        serde_json::from_str(text_blocks[0].1).map_err(|_| "table-text-mirror-invalid")?;
    if parsed_text != Value::Object(structured.clone()) {
        return Err("table-text-mirror-invalid");
    }

    Ok(Some(TableRowsShape {
        authorization,
        text_block_index: text_blocks[0].0,
    }))
}

fn raw_delimited_table_hint(result: &CanonicalMcpResult) -> bool {
    let mut text_blocks = result
        .content
        .iter()
        .filter_map(CanonicalContentBlock::text);
    let Some(text) = text_blocks.next() else {
        return false;
    };
    if text_blocks.next().is_some() {
        return false;
    }
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .collect();
    if lines.len() < 3 {
        return false;
    }
    [',', '\t'].iter().any(|delimiter| {
        let first_count = lines[0].matches(*delimiter).count();
        first_count > 0
            && lines[1..]
                .iter()
                .all(|line| line.matches(*delimiter).count() == first_count)
    })
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

fn is_tree_entries_field(field: &str) -> bool {
    matches!(
        normalized_schema_field(field).as_str(),
        "entries" | "files" | "nodes" | "paths" | "children" | "tree"
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

fn propose_tree_listing(
    result: &CanonicalMcpResult,
    shape: &TreeListingShape,
    opts: &CompressOptions,
    manifest: &McpStrategyManifest,
) -> McpProposalOutcome {
    let PreservedField::Value(Value::Object(source)) = &result.structured_content else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_entries) = source
        .get(&shape.authorization.entries_field)
        .and_then(Value::as_array)
    else {
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
    if shape.authorization.removable_indices.is_empty() {
        return McpProposalOutcome::NoSavings;
    }

    let maximum_by_schema = source_entries
        .len()
        .saturating_sub(shape.authorization.min_entries);
    let maximum_omitted = shape
        .authorization
        .removable_indices
        .len()
        .min(maximum_by_schema)
        .min(MCP_MAX_TREE_OMITTED_ENTRIES);
    let mut best = None;
    for omitted_count in 1..=maximum_omitted {
        let omitted_indices = shape.authorization.removable_indices[..omitted_count].to_vec();
        let candidate =
            tree_listing_candidate(source, &shape.authorization.entries_field, &omitted_indices);
        let projection = tree_listing_text_projection(
            &candidate,
            &shape.authorization.entries_field,
            source_entries.len(),
            shape.authorization.requested_depth,
        );
        let Ok(text_projection) = serde_json::to_string(&projection) else {
            return McpProposalOutcome::NoSavings;
        };
        let chars_out = text_projection.chars().count();
        if chars_out <= opts.target_chars.max(1) && chars_out < source_chars {
            best = Some((omitted_indices, Value::Object(candidate), text_projection));
            break;
        }
    }
    let Some((omitted_indices, replacement, text_projection)) = best else {
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
            edit: McpStructuredContentEdit::TreeListing(McpTreeListingEdit {
                entries_field: shape.authorization.entries_field.clone(),
                root_field: shape.authorization.root_field.clone(),
                path_field: shape.authorization.path_field.clone(),
                kind_field: shape.authorization.kind_field.clone(),
                requested_root: shape.authorization.requested_root.clone(),
                requested_depth: shape.authorization.requested_depth,
                omitted_indices,
                omission_marker_field: MCP_OMISSION_MARKER_FIELD.to_string(),
            }),
        }),
    }))
}

fn propose_table_rows(
    result: &CanonicalMcpResult,
    shape: &TableRowsShape,
    opts: &CompressOptions,
    manifest: &McpStrategyManifest,
) -> McpProposalOutcome {
    let PreservedField::Value(Value::Object(source)) = &result.structured_content else {
        return McpProposalOutcome::NoSavings;
    };
    let Some(source_rows) = source
        .get(&shape.authorization.rows_field)
        .and_then(Value::as_array)
    else {
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
    if source_rows.len() < 3 {
        return McpProposalOutcome::NoSavings;
    }

    let minimum_retained = shape.authorization.min_rows.max(2);
    let maximum_retained = (source_rows.len() - 1).min(MCP_MAX_RETAINED_TABLE_ROWS);
    if minimum_retained > maximum_retained {
        return McpProposalOutcome::NoSavings;
    }

    let mut best = None;
    let mut low = minimum_retained;
    let mut high = maximum_retained;
    while low <= high {
        let retained = low + (high - low) / 2;
        let retained_indices = collection_head_tail_indices(source_rows.len(), retained);
        let candidate =
            table_rows_candidate(source, &shape.authorization.rows_field, &retained_indices);
        let projection = table_rows_text_projection(
            &candidate,
            &shape.authorization.columns_field,
            &shape.authorization.rows_field,
            shape.authorization.column_count,
            source_rows.len(),
        );
        let Ok(text_projection) = serde_json::to_string(&projection) else {
            return McpProposalOutcome::NoSavings;
        };
        let chars_out = text_projection.chars().count();
        if chars_out > opts.target_chars.max(1) || chars_out >= source_chars {
            high = retained - 1;
            continue;
        }
        best = Some((retained_indices, Value::Object(candidate), text_projection));
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
            edit: McpStructuredContentEdit::TableRows(McpTableRowsEdit {
                columns_field: shape.authorization.columns_field.clone(),
                rows_field: shape.authorization.rows_field.clone(),
                retained_indices,
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
static TREE_LISTING_STRATEGY: TreeListingStrategy = TreeListingStrategy;
static TABLE_ROWS_STRATEGY: TableRowsStrategy = TableRowsStrategy;
static STRATEGIES: [&dyn McpResultStrategy; 6] = [
    &SEARCH_RESULTS_STRATEGY,
    &TREE_LISTING_STRATEGY,
    &TABLE_ROWS_STRATEGY,
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
            proposal: None,
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
            proposal: None,
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
                    proposal: None,
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
                    proposal: None,
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
                    proposal: None,
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
                proposal: Some(*proposal),
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
                    proposal: None,
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
        proposal: None,
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

    fn tree_result() -> (CanonicalMcpResult, ToolContract, Value) {
        let entries = json!([
            {"path": "src", "kind": "directory", "bytes": 0},
            {"path": "src/lib.rs", "kind": "file", "bytes": 420},
            {"path": "node_modules", "kind": "directory", "bytes": 0},
            {"path": "node_modules/pkg-a", "kind": "directory", "bytes": 0, "detail": "package metadata ".repeat(18)},
            {"path": "node_modules/pkg-a/index.js", "kind": "file", "bytes": 9000, "detail": "generated dependency source ".repeat(24)},
            {"path": "node_modules/pkg-b", "kind": "directory", "bytes": 0, "detail": "package metadata ".repeat(18)},
            {"path": "node_modules/pkg-b/index.js", "kind": "file", "bytes": 8000, "detail": "generated dependency source ".repeat(24)},
            {"path": "target", "kind": "directory", "bytes": 0},
            {"path": "target/debug", "kind": "directory", "bytes": 0, "detail": "compiler artifact metadata ".repeat(18)},
            {"path": "target/debug/ctx", "kind": "file", "bytes": 5000000, "detail": "compiled binary artifact ".repeat(24)},
            {"path": "README.md", "kind": "file", "bytes": 1200}
        ]);
        let structured = json!({
            "root": "/workspace/project",
            "entries": entries,
            "order": "path-ascending"
        });
        let text = serde_json::to_string(&structured).expect("serialize tree fixture");
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false,
            "_meta": {"request": "tree-r1"}
        }))
        .expect("tree result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {
                    "root": {"type": "string"},
                    "entries": {
                        "type": "array",
                        "minItems": 3,
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"},
                                "kind": {"type": "string", "enum": ["file", "directory"]},
                                "bytes": {"type": "integer", "minimum": 0},
                                "detail": {"type": "string"}
                            },
                            "required": ["path", "kind", "bytes"],
                            "additionalProperties": false
                        }
                    },
                    "order": {"type": "string"}
                },
                "required": ["root", "entries", "order"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        let input = json!({"root": "/workspace/project"});
        (result, contract, input)
    }

    fn table_result(row_count: usize) -> (CanonicalMcpResult, ToolContract) {
        let rows: Vec<_> = (0..row_count)
            .map(|index| {
                json!([
                    format!("row-{index}"),
                    index,
                    format!(
                        "sanitized table cell {index} {}",
                        "repeated tabular context ".repeat(10)
                    )
                ])
            })
            .collect();
        let structured = json!({
            "columns": ["record", "ordinal", "summary"],
            "rows": rows,
            "rowCount": row_count,
            "order": "source-order"
        });
        let text = serde_json::to_string(&structured).expect("serialize table fixture");
        let result = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured,
            "isError": false,
            "_meta": {"request": "table-r1"}
        }))
        .expect("table result");
        let contract = ToolContract {
            output_schema: PreservedField::Value(json!({
                "type": "object",
                "properties": {
                    "columns": {
                        "type": "array",
                        "minItems": 3,
                        "maxItems": 3,
                        "items": {"type": "string"}
                    },
                    "rows": {
                        "type": "array",
                        "minItems": 2,
                        "items": {
                            "type": "array",
                            "minItems": 3,
                            "maxItems": 3,
                            "items": {"type": ["string", "integer"]}
                        }
                    },
                    "rowCount": {"type": "integer", "minimum": 0},
                    "order": {"type": "string"}
                },
                "required": ["columns", "rows", "rowCount", "order"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        (result, contract)
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
                &MCP_TREE_LISTING_V1,
                &MCP_TABLE_ROWS_V1,
                &MCP_PAGINATED_COLLECTION_V1,
                &MCP_ENTITY_DETAIL_V1,
                &MCP_TEXT_BLOCKS_V2
            ]
        );
        assert!(!manifests[0].invariants.is_empty());
    }

    #[test]
    fn table_proposal_preserves_header_rows_order_and_largest_fitting_sample() {
        let (result, contract) = table_result(12);
        let shape = table_rows_shape(&result, Some(&contract))
            .expect("shape assessment")
            .expect("table shape");
        assert_eq!(shape.authorization.columns_field, "columns");
        assert_eq!(shape.authorization.rows_field, "rows");
        assert_eq!(shape.authorization.column_count, 3);
        assert_eq!(shape.authorization.min_rows, 2);

        let source = result
            .structured_content
            .value()
            .unwrap()
            .as_object()
            .unwrap();
        let source_rows = source["rows"].as_array().unwrap();
        let retained_indices = collection_head_tail_indices(source_rows.len(), 4);
        let expected_candidate = table_rows_candidate(source, "rows", &retained_indices);
        let target = serde_json::to_string(&table_rows_text_projection(
            &expected_candidate,
            "columns",
            "rows",
            3,
            source_rows.len(),
        ))
        .unwrap()
        .chars()
        .count();
        let McpProposalOutcome::Proposed(proposal) =
            propose_table_rows(&result, &shape, &options(target), &MCP_TABLE_ROWS_V1)
        else {
            panic!("expected table proposal");
        };
        let replacement = proposal.structured_content.as_ref().unwrap();
        assert_eq!(replacement.replacement, Value::Object(expected_candidate));
        let McpStructuredContentEdit::TableRows(edit) = &replacement.edit else {
            panic!("expected table edit");
        };
        assert_eq!(edit.retained_indices, retained_indices);
        let candidate = replacement.replacement.as_object().unwrap();
        assert_eq!(candidate["columns"], source["columns"]);
        assert_eq!(candidate["rowCount"], source["rowCount"]);
        assert_eq!(candidate["order"], source["order"]);
        for (row, index) in candidate["rows"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&retained_indices)
        {
            assert_eq!(row, &source_rows[*index]);
        }

        let validated = validate_mcp_proposal_with_contract_and_input(
            &result,
            Some(&contract),
            None,
            &MCP_TABLE_ROWS_V1,
            &proposal,
        )
        .expect("table proposal validates");
        assert_eq!(validated.table_columns, Some(3));
        assert_eq!(validated.table_rows_in, Some(12));
        assert_eq!(validated.table_rows_out, Some(4));
        assert_eq!(validated.table_rows_omitted, Some(8));
        assert_eq!(result.render(), *result.raw());
    }

    #[test]
    fn stale_forged_and_overtrimmed_table_proposals_are_rejected() {
        let (result, contract) = table_result(12);
        let shape = table_rows_shape(&result, Some(&contract)).unwrap().unwrap();
        let source = result
            .structured_content
            .value()
            .unwrap()
            .as_object()
            .unwrap();
        let rows = source["rows"].as_array().unwrap();
        let four_indices = collection_head_tail_indices(rows.len(), 4);
        let four_candidate = table_rows_candidate(source, "rows", &four_indices);
        let target = serde_json::to_string(&table_rows_text_projection(
            &four_candidate,
            "columns",
            "rows",
            3,
            rows.len(),
        ))
        .unwrap()
        .chars()
        .count();
        let McpProposalOutcome::Proposed(proposal) =
            propose_table_rows(&result, &shape, &options(target), &MCP_TABLE_ROWS_V1)
        else {
            panic!("expected table proposal");
        };

        let mut forged_selection = (*proposal).clone();
        let McpStructuredContentEdit::TableRows(edit) =
            &mut forged_selection.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected table edit");
        };
        edit.retained_indices = vec![0, 1, 2, 11];
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_TABLE_ROWS_V1,
                &forged_selection,
            ),
            Err(McpProposalRejection::TableSelectionInvalid)
        );

        let mut forged_marker = (*proposal).clone();
        let mut projection: Value =
            serde_json::from_str(&forged_marker.replacements[0].replacement).unwrap();
        projection["_ctxOmission"]["columns"] = json!(999);
        forged_marker.replacements[0].replacement = serde_json::to_string(&projection).unwrap();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_TABLE_ROWS_V1,
                &forged_marker,
            ),
            Err(McpProposalRejection::TableOmissionMarkerInvalid)
        );

        let mut overtrimmed = (*proposal).clone();
        let replacement = overtrimmed.structured_content.as_mut().unwrap();
        let three_indices = collection_head_tail_indices(rows.len(), 3);
        let three_candidate = table_rows_candidate(source, "rows", &three_indices);
        replacement.replacement = Value::Object(three_candidate.clone());
        let McpStructuredContentEdit::TableRows(edit) = &mut replacement.edit else {
            panic!("expected table edit");
        };
        edit.retained_indices = three_indices;
        overtrimmed.replacements[0].replacement = serde_json::to_string(
            &table_rows_text_projection(&three_candidate, "columns", "rows", 3, rows.len()),
        )
        .unwrap();
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_TABLE_ROWS_V1,
                &overtrimmed,
            ),
            Err(McpProposalRejection::TableSelectionInvalid)
        );

        let mut stale = (*proposal).clone();
        stale.structured_content.as_mut().unwrap().expected["order"] = json!("changed");
        assert_eq!(
            validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_TABLE_ROWS_V1,
                &stale,
            ),
            Err(McpProposalRejection::StaleStructuredContent)
        );
    }

    #[test]
    fn table_strategy_rejects_raw_delimited_text_and_yields_on_pagination() {
        for raw_table in ["id,name\n1,Ada\n2,Grace\n", "id\tname\n1\tAda\n2\tGrace\n"] {
            let raw_table = parse_mcp_result(&json!({
                "content": [{"type": "text", "text": raw_table}],
                "isError": false
            }))
            .unwrap();
            let observation = evaluate_mcp_strategies_shadow(
                &raw_table,
                &options(20),
                &CompressContext::default(),
            );
            assert_eq!(observation.manifest, None);
            assert_eq!(
                observation.pass_through_reason,
                Some("table-raw-delimited-text-unsupported")
            );
            assert!(!observation.proposal_attempted);
        }
        for two_line_text in [
            "event,first\nevent,second\n",
            "event\tfirst\nevent\tsecond\n",
        ] {
            let result = parse_mcp_result(&json!({
                "content": [{"type": "text", "text": two_line_text}],
                "isError": false
            }))
            .unwrap();
            assert!(!raw_delimited_table_hint(&result));
            assert!(table_rows_shape(&result, None).unwrap().is_none());
        }

        let (table, mut contract) = table_result(8);
        let mut raw = table.raw().clone();
        raw["structuredContent"]["nextCursor"] = json!("page-2");
        raw["content"][0]["text"] =
            json!(serde_json::to_string(&raw["structuredContent"]).unwrap());
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            schema["additionalProperties"] = json!(true);
        }
        let paginated = parse_mcp_result(&raw).unwrap();
        assert!(table_rows_shape(&paginated, Some(&contract))
            .expect("shape assessment")
            .is_none());
    }

    #[test]
    fn table_schema_marker_mirror_and_error_boundaries_fail_open() {
        let (result, contract) = table_result(8);
        let schema_less = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            None,
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(schema_less.manifest, None);
        assert_eq!(
            schema_less.pass_through_reason,
            Some("table-output-schema-required")
        );

        let mut collision_raw = result.raw().clone();
        collision_raw["structuredContent"][MCP_OMISSION_MARKER_FIELD] = json!({"server": true});
        collision_raw["content"][0]["text"] =
            json!(serde_json::to_string(&collision_raw["structuredContent"]).unwrap());
        let mut collision_contract = contract.clone();
        if let PreservedField::Value(schema) = &mut collision_contract.output_schema {
            schema["properties"][MCP_OMISSION_MARKER_FIELD] = json!({"type": "object"});
        }
        let collision = parse_mcp_result(&collision_raw).unwrap();
        let collision_observation = evaluate_mcp_strategies_shadow_with_contract(
            &collision,
            Some(&collision_contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(collision_observation.manifest, None);
        assert_eq!(
            collision_observation.pass_through_reason,
            Some("table-omission-marker-collision")
        );

        let mut nonmirror_raw = result.raw().clone();
        nonmirror_raw["content"][0]["text"] = json!("{\"columns\":[\"wrong\"],\"rows\":[]}");
        let nonmirror = parse_mcp_result(&nonmirror_raw).unwrap();
        let nonmirror_observation = evaluate_mcp_strategies_shadow_with_contract(
            &nonmirror,
            Some(&contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(nonmirror_observation.manifest, None);
        assert_eq!(
            nonmirror_observation.pass_through_reason,
            Some("table-text-mirror-invalid")
        );

        let mut error_raw = result.raw().clone();
        error_raw["isError"] = json!(true);
        let error = parse_mcp_result(&error_raw).unwrap();
        let error_observation = evaluate_mcp_strategies_shadow_with_contract(
            &error,
            Some(&contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(error_observation.manifest, None);
        assert_eq!(error_observation.pass_through_reason, Some("error-result"));
        assert!(!error_observation.proposal_attempted);
    }

    #[test]
    fn table_minimum_rows_can_make_an_over_budget_source_untrimmable() {
        let (result, contract) = table_result(2);
        let observation = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(100),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_TABLE_ROWS_V1));
        assert!(observation.proposal_attempted);
        assert_eq!(observation.pass_through_reason, Some("no-savings"));
    }

    #[test]
    fn table_generated_budgets_are_deterministic_and_within_budget_is_distinct() {
        let (result, contract) = table_result(16);
        let shape = table_rows_shape(&result, Some(&contract)).unwrap().unwrap();
        let source_chars = result.content[0].text().unwrap().chars().count();
        let within = evaluate_mcp_strategies_shadow_with_contract(
            &result,
            Some(&contract),
            &options(source_chars),
            &CompressContext::default(),
        );
        assert_eq!(within.manifest, Some(&MCP_TABLE_ROWS_V1));
        assert!(!within.proposal_attempted);
        assert_eq!(within.pass_through_reason, Some("within-budget"));

        let mut validated_budgets = 0;
        for target in (700..source_chars).step_by(150) {
            let first = propose_table_rows(&result, &shape, &options(target), &MCP_TABLE_ROWS_V1);
            let second = propose_table_rows(&result, &shape, &options(target), &MCP_TABLE_ROWS_V1);
            assert_eq!(first, second);
            let McpProposalOutcome::Proposed(proposal) = first else {
                continue;
            };
            let validated = validate_mcp_proposal_with_contract(
                &result,
                Some(&contract),
                &MCP_TABLE_ROWS_V1,
                &proposal,
            )
            .expect("generated table proposal validates");
            assert!(validated.chars_out <= target);
            validated_budgets += 1;
        }
        assert!(validated_budgets >= 3);
    }

    #[test]
    fn tree_proposal_omits_only_generated_descendants_and_preserves_order() {
        let (result, contract, input) = tree_result();
        let shape = tree_listing_shape(&result, Some(&contract), Some(&input))
            .expect("shape assessment")
            .expect("tree shape");
        assert_eq!(shape.authorization.entries_field, "entries");
        assert_eq!(shape.authorization.root_field, "root");
        assert_eq!(shape.authorization.path_field, "path");
        assert_eq!(shape.authorization.kind_field, "kind");
        assert_eq!(
            shape.authorization.requested_root.as_deref(),
            Some("/workspace/project")
        );
        assert_eq!(shape.authorization.requested_depth, None);
        assert_eq!(shape.authorization.removable_indices, [4, 6, 9, 3, 5, 8]);

        let McpProposalOutcome::Proposed(proposal) =
            propose_tree_listing(&result, &shape, &options(900), &MCP_TREE_LISTING_V1)
        else {
            panic!("expected tree proposal");
        };
        let replacement = proposal
            .structured_content
            .as_ref()
            .expect("structured replacement");
        let McpStructuredContentEdit::TreeListing(edit) = &replacement.edit else {
            panic!("expected tree edit");
        };
        assert!(!edit.omitted_indices.is_empty());
        assert!(edit
            .omitted_indices
            .iter()
            .all(|index| [3, 4, 5, 6, 8, 9].contains(index)));

        let source_entries = replacement.expected["entries"].as_array().unwrap();
        let candidate_entries = replacement.replacement["entries"].as_array().unwrap();
        let omitted: std::collections::BTreeSet<_> = edit.omitted_indices.iter().copied().collect();
        let expected_retained: Vec<_> = source_entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !omitted.contains(index))
            .map(|(_, entry)| entry.clone())
            .collect();
        assert_eq!(candidate_entries, &expected_retained);
        for protected in ["src", "src/lib.rs", "node_modules", "target", "README.md"] {
            assert!(candidate_entries
                .iter()
                .any(|entry| entry["path"] == protected));
        }

        let projection: Value =
            serde_json::from_str(&proposal.replacements[0].replacement).expect("JSON projection");
        assert_eq!(
            projection.pointer("/_ctxOmission/selection"),
            Some(&json!(
                "generated-vendor-descendants-outside-requested-depth"
            ))
        );
        assert_eq!(
            projection.pointer("/_ctxOmission/omittedEntries"),
            Some(&json!(edit.omitted_indices.len()))
        );
        let projection_text = &proposal.replacements[0].replacement;
        for omitted_path in [
            "node_modules/pkg-a/index.js",
            "node_modules/pkg-b/index.js",
            "target/debug/ctx",
        ] {
            assert!(!projection_text.contains(omitted_path));
        }

        let validated = validate_mcp_proposal_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &MCP_TREE_LISTING_V1,
            &proposal,
        )
        .expect("validated tree proposal");
        assert_eq!(validated.tree_entries_in, Some(source_entries.len()));
        assert_eq!(validated.tree_entries_out, Some(candidate_entries.len()));
        assert_eq!(
            validated.tree_entries_omitted,
            Some(edit.omitted_indices.len())
        );
        assert_eq!(validated.tree_requested_root_present, Some(true));
        assert_eq!(validated.tree_requested_depth_present, Some(false));
        assert_eq!(validated.collection_items_in, None);
        assert_eq!(validated.search_results_in, None);
        assert_eq!(validated.entity_fields_in, None);
    }

    #[test]
    fn tree_requested_depth_and_input_context_fail_open_conservatively() {
        let (result, contract, input) = tree_result();
        let depth_protected = json!({"root": "/workspace/project", "maxDepth": 3});
        let shape = tree_listing_shape(&result, Some(&contract), Some(&depth_protected))
            .expect("shape assessment")
            .expect("tree shape");
        assert!(shape.authorization.removable_indices.is_empty());
        let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&depth_protected),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(observation.manifest, Some(&MCP_TREE_LISTING_V1));
        assert_eq!(observation.pass_through_reason, Some("no-savings"));

        for (hostile_input, reason) in [
            (
                json!({"root": "/different/project"}),
                "tree-requested-root-mismatch",
            ),
            (
                json!({"root": "/workspace/project", "path": "/workspace/project"}),
                "tree-input-selector-ambiguous",
            ),
            (
                json!({"request": {"depth": 1}}),
                "tree-input-selector-unsupported",
            ),
            (
                json!({"root": "/workspace/project", "depth": 65}),
                "tree-input-selector-unsupported",
            ),
        ] {
            let observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&hostile_input),
                &options(900),
                &CompressContext::default(),
            );
            assert_eq!(observation.manifest, None);
            assert_eq!(observation.pass_through_reason, Some(reason));
            assert!(!observation.proposal_attempted);
        }

        let schema_less = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            None,
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            schema_less.pass_through_reason,
            Some("tree-output-schema-required")
        );
    }

    #[test]
    fn stale_forged_and_overtrimmed_tree_proposals_are_rejected() {
        let (result, contract, input) = tree_result();
        let shape = tree_listing_shape(&result, Some(&contract), Some(&input))
            .unwrap()
            .unwrap();
        let McpProposalOutcome::Proposed(proposal) =
            propose_tree_listing(&result, &shape, &options(900), &MCP_TREE_LISTING_V1)
        else {
            panic!("expected tree proposal");
        };

        let mut forged_selection = (*proposal).clone();
        let McpStructuredContentEdit::TreeListing(edit) =
            &mut forged_selection.structured_content.as_mut().unwrap().edit
        else {
            panic!("expected tree edit");
        };
        edit.omitted_indices = vec![1];
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_TREE_LISTING_V1,
                &forged_selection,
            ),
            Err(McpProposalRejection::TreeSelectionInvalid)
        );

        let mut forged_marker = (*proposal).clone();
        let mut projection: Value =
            serde_json::from_str(&forged_marker.replacements[0].replacement).unwrap();
        projection["_ctxOmission"]["omittedEntries"] = json!(999);
        forged_marker.replacements[0].replacement = serde_json::to_string(&projection).unwrap();
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_TREE_LISTING_V1,
                &forged_marker,
            ),
            Err(McpProposalRejection::TreeOmissionMarkerInvalid)
        );

        let different_input = json!({"root": "/workspace/project", "depth": 3});
        assert_eq!(
            validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&different_input),
                &MCP_TREE_LISTING_V1,
                &proposal,
            ),
            Err(McpProposalRejection::TreeSelectionInvalid)
        );

        let mut overtrimmed = (*proposal).clone();
        let replacement = overtrimmed.structured_content.as_mut().unwrap();
        let McpStructuredContentEdit::TreeListing(edit) = &mut replacement.edit else {
            panic!("expected tree edit");
        };
        if edit.omitted_indices.len() < shape.authorization.removable_indices.len() {
            edit.omitted_indices = shape.authorization.removable_indices.clone();
            let source = replacement.expected.as_object().unwrap();
            let candidate = tree_listing_candidate(source, "entries", &edit.omitted_indices);
            replacement.replacement = Value::Object(candidate.clone());
            overtrimmed.replacements[0].replacement =
                serde_json::to_string(&tree_listing_text_projection(
                    &candidate,
                    "entries",
                    source["entries"].as_array().unwrap().len(),
                    None,
                ))
                .unwrap();
            assert_eq!(
                validate_mcp_proposal_with_contract_and_input(
                    &result,
                    Some(&contract),
                    Some(&input),
                    &MCP_TREE_LISTING_V1,
                    &overtrimmed,
                ),
                Err(McpProposalRejection::TreeSelectionInvalid)
            );
        }
    }

    #[test]
    fn tree_strategy_does_not_steal_search_or_paginated_shapes() {
        let (search, search_contract) = search_result(8);
        let search_observation = evaluate_mcp_strategies_shadow_with_contract(
            &search,
            Some(&search_contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(search_observation.manifest, Some(&MCP_SEARCH_RESULTS_V1));

        let (collection, collection_contract) = paginated_result(8);
        let collection_observation = evaluate_mcp_strategies_shadow_with_contract(
            &collection,
            Some(&collection_contract),
            &options(700),
            &CompressContext::default(),
        );
        assert_eq!(
            collection_observation.manifest,
            Some(&MCP_PAGINATED_COLLECTION_V1)
        );

        let (tree, mut tree_contract, input) = tree_result();
        let mut raw = tree.raw().clone();
        raw["structuredContent"]["nextCursor"] = json!("page-2");
        raw["content"][0]["text"] =
            json!(serde_json::to_string(&raw["structuredContent"]).unwrap());
        if let PreservedField::Value(schema) = &mut tree_contract.output_schema {
            schema["additionalProperties"] = json!(true);
        }
        let payload_only_pagination = parse_mcp_result(&raw).unwrap();
        assert!(
            tree_listing_shape(
                &payload_only_pagination,
                Some(&tree_contract),
                Some(&input),
            )
            .expect("shape assessment")
            .is_none(),
            "payload pagination evidence must make the tree strategy yield even when the schema allows it implicitly"
        );
    }

    #[test]
    fn tree_generated_budgets_are_deterministic_and_within_budget_is_distinct() {
        let (result, contract, input) = tree_result();
        let shape = tree_listing_shape(&result, Some(&contract), Some(&input))
            .unwrap()
            .unwrap();
        let source_chars = result.content[0].text().unwrap().chars().count();
        let within = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &result,
            Some(&contract),
            Some(&input),
            &options(source_chars),
            &CompressContext::default(),
        );
        assert_eq!(within.manifest, Some(&MCP_TREE_LISTING_V1));
        assert!(!within.proposal_attempted);
        assert_eq!(within.pass_through_reason, Some("within-budget"));

        let mut validated_budgets = 0;
        for target in (500..source_chars).step_by(100) {
            let first =
                propose_tree_listing(&result, &shape, &options(target), &MCP_TREE_LISTING_V1);
            let second =
                propose_tree_listing(&result, &shape, &options(target), &MCP_TREE_LISTING_V1);
            assert_eq!(first, second);
            let McpProposalOutcome::Proposed(proposal) = first else {
                continue;
            };
            let validated = validate_mcp_proposal_with_contract_and_input(
                &result,
                Some(&contract),
                Some(&input),
                &MCP_TREE_LISTING_V1,
                &proposal,
            )
            .expect("generated budget proposal validates");
            assert!(validated.chars_out <= target);
            validated_budgets += 1;
        }
        assert!(validated_budgets >= 3);
    }

    #[test]
    fn tree_marker_collisions_and_error_results_fail_before_proposal() {
        let (result, mut contract, input) = tree_result();
        let mut raw = result.raw().clone();
        let structured = raw["structuredContent"].as_object_mut().unwrap();
        structured.insert(MCP_OMISSION_MARKER_FIELD.into(), json!({"server": true}));
        raw["content"][0]["text"] = json!(serde_json::to_string(structured).unwrap());
        if let PreservedField::Value(schema) = &mut contract.output_schema {
            schema["properties"][MCP_OMISSION_MARKER_FIELD] = json!({"type": "object"});
        }
        let collision = parse_mcp_result(&raw).unwrap();
        let collision_observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &collision,
            Some(&contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(
            collision_observation.pass_through_reason,
            Some("tree-omission-marker-collision")
        );
        assert!(!collision_observation.proposal_attempted);

        raw["isError"] = json!(true);
        let error = parse_mcp_result(&raw).unwrap();
        let error_observation = evaluate_mcp_strategies_shadow_with_contract_and_input(
            &error,
            Some(&contract),
            Some(&input),
            &options(900),
            &CompressContext::default(),
        );
        assert_eq!(error_observation.pass_through_reason, Some("error-result"));
        assert!(error_observation.manifest.is_none());
        assert!(!error_observation.proposal_attempted);
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
