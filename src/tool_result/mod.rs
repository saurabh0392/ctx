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
pub(crate) use strategy::{
    assess_mcp_search_array_schema, collection_head_tail_indices, search_ranked_prefix_indices,
};
pub use strategy::{
    validate_mcp_proposal, validate_mcp_proposal_with_contract, McpPaginatedCollectionEdit,
    McpProposalRejection, McpSearchResultsEdit, McpStrategyManifest, McpStructuredContentEdit,
    McpStructuredContentReplacement, McpTextReplacement, McpTransformProposal,
    ValidatedMcpProposal, MCP_COLLECTION_OMISSION_MARKER_FIELD, MCP_MAX_RETAINED_COLLECTION_ITEMS,
    MCP_MAX_RETAINED_SEARCH_RESULTS, MCP_SEARCH_OMISSION_MARKER_FIELD,
};
pub use types::{
    CanonicalContentBlock, CanonicalMcpResult, CanonicalToolExchange, McpResultCoverage,
    PreservedField, ToolContract, ToolIdentity, ToolProvenance, ToolTransport,
};
