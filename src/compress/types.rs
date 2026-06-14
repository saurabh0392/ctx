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
