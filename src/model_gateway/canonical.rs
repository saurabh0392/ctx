//! Minimal in-memory model-path representation shared by independent protocol packs.

use serde_json::Value;

use crate::tool_result::{CanonicalMcpResult, CanonicalToolExchange};

pub type CanonicalModelExchange = CanonicalToolExchange<CanonicalModelResult>;

/// A structural location in the parsed request. M3 can use this to patch only the authorized text
/// leaf; M2 never writes through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathSegment {
    Field(&'static str),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTextLeaf {
    pub path: Vec<JsonPathSegment>,
    pub text: String,
}

/// The result content needed by the shared strategy layer. Raw request envelopes are intentionally
/// absent and remain bounded to the relay stack frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalModelResult {
    pub source_item_type: &'static str,
    pub content_kind: &'static str,
    pub text_leaves: Vec<ModelTextLeaf>,
    pub is_error: Option<bool>,
    pub already_shortened: bool,
    pub canonical_mcp: Option<CanonicalMcpResult>,
}

impl CanonicalModelResult {
    pub fn combined_text(&self) -> String {
        self.text_leaves
            .iter()
            .map(|leaf| leaf.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone)]
pub(super) struct PendingCall {
    pub position: usize,
    pub call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub contract: crate::tool_result::ToolContract,
}

#[derive(Debug, Clone)]
pub(super) struct PendingResult {
    pub position: usize,
    pub call_id: String,
    pub result: CanonicalModelResult,
}
