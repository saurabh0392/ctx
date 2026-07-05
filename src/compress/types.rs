use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressKind {
    Passthrough,
    Generic,
    GitStatus,
    GitDiff,
    GitLog,
    TestRunner,
    Grep,
    Read,
    Mcp,
    /// A file edit/write confirmation (Edit, Write, MultiEdit, ...). Shadow-only in practice: edit
    /// tools are not in the default `compress_tools`, so `compress_tool_output`'s `tool_allowed` gate
    /// returns None on the apply path and this strategy only ever runs inside
    /// `compute_shadow_decision` to measure what a trim would save. It never changes what the agent
    /// sees, so the agent never misreads what it just wrote (CTX-60). The controller (`agent::decide`)
    /// no longer special-cases edit tools (CTX-62): an edit decision can read `apply = true`, but the
    /// compress-tools membership, not the tool name, is what keeps the live cut from happening.
    Edit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressResult {
    pub text: String,
    pub chars_in: usize,
    pub chars_out: usize,
    pub strategy: String,
}

impl CompressResult {
    pub fn chars_saved(&self) -> usize {
        self.chars_in.saturating_sub(self.chars_out)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompressContext {
    pub cwd: String,
    pub prompt_keywords: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompressOptions {
    pub max_input_chars: usize,
    pub target_chars: usize,
    pub redact_secrets: bool,
    pub preserve_errors: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            max_input_chars: 12_000,
            target_chars: 2_500,
            redact_secrets: true,
            preserve_errors: true,
        }
    }
}
