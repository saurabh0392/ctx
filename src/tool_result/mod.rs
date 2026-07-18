//! Lossless, platform-neutral tool result contracts.
//!
//! The shipping compression path still uses [`crate::compress::CompressResult`]. This module is
//! the shadow-mode foundation for migrating native hooks and the future MCP gateway without
//! flattening structured results into a string first.

mod mcp;
mod strategy;
mod types;

pub use mcp::{parse_mcp_result, render_mcp_result_or_original, McpParseError};
pub use strategy::{
    validate_mcp_proposal, McpProposalRejection, McpStrategyManifest, McpTextReplacement,
    McpTransformProposal, ValidatedMcpProposal,
};
pub use types::{
    CanonicalContentBlock, CanonicalMcpResult, CanonicalToolExchange, McpResultCoverage,
    PreservedField, ToolContract, ToolIdentity, ToolProvenance, ToolTransport,
};
