use crate::tool_result::{
    collection_head_tail_indices, validate_mcp_output_schema, validate_mcp_proposal_with_contract,
    CanonicalContentBlock, CanonicalMcpResult, McpOutputSchemaValidation,
    McpPaginatedCollectionEdit, McpProposalRejection, McpStrategyManifest,
    McpStructuredContentEdit, McpStructuredContentReplacement, McpTextReplacement,
    McpTransformProposal, PreservedField, ToolContract, ValidatedMcpProposal,
    MCP_COLLECTION_OMISSION_MARKER_FIELD, MCP_MAX_RETAINED_COLLECTION_ITEMS,
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
    ) -> McpStrategyEligibility;
    fn propose(
        &self,
        result: &CanonicalMcpResult,
        contract: Option<&ToolContract>,
        opts: &CompressOptions,
        ctx: &CompressContext,
    ) -> McpProposalOutcome;
}

enum McpStrategyEligibility {
    NotApplicable,
    Eligible(&'static str),
    Rejected(&'static str),
}

enum McpProposalOutcome {
    WithinBudget,
    NoSavings,
    Proposed(McpTransformProposal),
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
            McpProposalOutcome::Proposed(McpTransformProposal {
                strategy_id: self.manifest().id,
                strategy_version: self.manifest().version,
                max_total_text_chars: total_budget,
                replacements,
                structured_content: None,
            })
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
        opts: &CompressOptions,
        _ctx: &CompressContext,
    ) -> McpProposalOutcome {
        let Ok(Some(shape)) = paginated_collection_shape(result, contract) else {
            return McpProposalOutcome::NoSavings;
        };
        propose_paginated_collection(result, &shape, opts, self.manifest())
    }
}

#[derive(Debug)]
struct PaginatedCollectionShape {
    field: String,
    text_block_index: usize,
    min_items: usize,
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
    if structured.contains_key(MCP_COLLECTION_OMISSION_MARKER_FIELD) {
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

fn is_pagination_field(field: &str) -> bool {
    let normalized: String = field
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
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
        let candidate = collection_candidate(source, &shape.field, source_items, &indices);
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

    McpProposalOutcome::Proposed(McpTransformProposal {
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
                omission_marker_field: MCP_COLLECTION_OMISSION_MARKER_FIELD.to_string(),
            }),
        }),
    })
}

fn collection_candidate(
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

fn collection_text_projection(
    candidate: &Map<String, Value>,
    field: &str,
    original_items: usize,
    retained_items: usize,
) -> Value {
    let mut projection = candidate.clone();
    projection.insert(
        MCP_COLLECTION_OMISSION_MARKER_FIELD.to_string(),
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
static STRATEGIES: [&dyn McpResultStrategy; 2] =
    [&PAGINATED_COLLECTION_STRATEGY, &TEXT_BLOCK_STRATEGY];

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

pub(crate) fn evaluate_mcp_strategies_shadow_with_contract(
    result: &CanonicalMcpResult,
    contract: Option<&ToolContract>,
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
        let shape_authorization = match strategy.eligibility(result, contract) {
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
        let proposal = match strategy.propose(result, contract, opts, ctx) {
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
        return match validate_mcp_proposal_with_contract(result, contract, manifest, &proposal) {
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

    #[test]
    fn registry_is_deterministic_and_versioned() {
        let manifests: Vec<_> = STRATEGIES
            .iter()
            .map(|strategy| strategy.manifest())
            .collect();
        assert_eq!(
            manifests,
            vec![&MCP_PAGINATED_COLLECTION_V1, &MCP_TEXT_BLOCKS_V2]
        );
        assert!(!manifests[0].invariants.is_empty());
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
            &mut reordered.structured_content.as_mut().unwrap().edit;
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
        projection[MCP_COLLECTION_OMISSION_MARKER_FIELD]["omittedItems"] = json!(999);
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
            &mut renamed_marker.structured_content.as_mut().unwrap().edit;
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
            TEXT_BLOCK_STRATEGY.propose(&result, None, &opts, &CompressContext::default())
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
