//! Lossless, platform-neutral tool result contracts.
//!
//! The shipping compression path still uses [`crate::compress::CompressResult`]. This module is
//! the shadow-mode foundation for migrating native hooks and the future MCP gateway without
//! flattening structured results into a string first.

mod contract;
mod mcp;
mod schema;
mod strategy;
mod types;

pub use contract::{
    parse_mcp_tools_list, CanonicalMcpTool, CanonicalMcpToolEntry, CanonicalMcpToolsList,
    McpContractCacheError, McpContractCapture, McpToolContractCache, McpToolContractKey,
    McpToolsListParseError,
};
pub use mcp::{parse_mcp_result, render_mcp_result_or_original, McpParseError};
pub use schema::{validate_mcp_output_schema, McpOutputSchemaValidation, McpSchemaRejection};
#[allow(deprecated)]
pub use strategy::MCP_COLLECTION_OMISSION_MARKER_FIELD;
pub(crate) use strategy::{
    assess_mcp_entity_schema, assess_mcp_search_array_schema, assess_mcp_table_schema,
    assess_mcp_tree_listing_schema, collection_head_tail_indices, entity_detail_candidate,
    entity_detail_text_projection, search_ranked_prefix_indices, table_rows_candidate,
    table_rows_text_projection, tree_listing_candidate, tree_listing_text_projection,
    McpEntitySchemaAuthorization, McpTableSchemaAuthorization, McpTreeSchemaAuthorization,
};
pub use strategy::{
    validate_mcp_proposal, validate_mcp_proposal_with_contract,
    validate_mcp_proposal_with_contract_and_input, McpEntityDetailEdit, McpPaginatedCollectionEdit,
    McpProposalRejection, McpSearchResultsEdit, McpStrategyManifest, McpStructuredContentEdit,
    McpStructuredContentReplacement, McpTableRowsEdit, McpTextReplacement, McpTransformProposal,
    McpTreeListingEdit, ValidatedMcpProposal, MCP_MAX_ENTITY_FIELDS, MCP_MAX_ENTITY_OMITTED_FIELDS,
    MCP_MAX_RETAINED_COLLECTION_ITEMS, MCP_MAX_RETAINED_SEARCH_RESULTS,
    MCP_MAX_RETAINED_TABLE_ROWS, MCP_MAX_TABLE_COLUMNS, MCP_MAX_TABLE_ROWS, MCP_MAX_TREE_ENTRIES,
    MCP_MAX_TREE_OMITTED_ENTRIES, MCP_OMISSION_MARKER_FIELD,
};
pub use types::{
    CanonicalContentBlock, CanonicalMcpResult, CanonicalToolExchange, McpResultCoverage,
    PreservedField, ToolContract, ToolIdentity, ToolProvenance, ToolTransport,
};
