//! Session context for compression: prompt keywords (v1) and TaskFrame (SGR v2).

use super::types::CompressContext;
use serde_json::Value;

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "your", "have", "will", "what",
    "when", "where", "which", "about", "please", "help", "need", "want", "make", "just", "also",
];

const CORRECTION_PHRASES: &[&str] = &[
    "no that's",
    "not that",
    "wrong",
    "instead",
    "don't",
    "do not",
    "actually",
    "i said",
    "i meant",
    "try again",
    "that's not",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgrMode {
    Normal,
    Debug,
    Scan,
}

impl SgrMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SgrMode::Normal => "normal",
            SgrMode::Debug => "debug",
            SgrMode::Scan => "scan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskFrame {
    pub prompt: String,
    pub cwd: String,
    pub profile: String,
    pub prompt_keywords: Vec<String>,
    pub correction_snippets: Vec<String>,
    pub recent_tools: Vec<String>,
    pub focus_paths: Vec<String>,
    pub focus_symbols: Vec<String>,
    pub prior_line_hashes: Vec<u64>,
    pub mode: SgrMode,
}

impl Default for TaskFrame {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            cwd: String::new(),
            profile: String::new(),
            prompt_keywords: Vec::new(),
            correction_snippets: Vec::new(),
            recent_tools: Vec::new(),
            focus_paths: Vec::new(),
            focus_symbols: Vec::new(),
            prior_line_hashes: Vec::new(),
            mode: SgrMode::Normal,
        }
    }
}

pub fn keywords_from_prompt(prompt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in prompt.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let w = word.trim().to_lowercase();
        if w.len() < 3 || STOP_WORDS.contains(&w.as_str()) {
            continue;
        }
        if !out.iter().any(|x| x == &w) {
            out.push(w);
        }
        if out.len() >= 12 {
            break;
        }
    }
    out
}

pub fn build_context(cwd: &str, prompt: &str) -> CompressContext {
    CompressContext {
        cwd: cwd.to_string(),
        prompt_keywords: keywords_from_prompt(prompt),
    }
}

pub fn build_task_frame_minimal(prompt: &str, cwd: &str) -> TaskFrame {
    TaskFrame {
        prompt: prompt.to_string(),
        cwd: cwd.to_string(),
        prompt_keywords: keywords_from_prompt(prompt),
        focus_paths: extract_paths_from_text(prompt),
        focus_symbols: extract_symbols_from_text(prompt),
        mode: detect_sgr_mode(prompt, &[], ""),
        ..Default::default()
    }
}

pub fn build_task_frame(
    session_id: Option<&str>,
    cwd: &str,
    tool_name: &str,
    tool_input: &Value,
    profile: &str,
    dedup_enabled: bool,
) -> TaskFrame {
    let prompt = load_prompt_from_session(session_id);
    let jsonl_corrections = correction_snippets_from_jsonl(session_id);
    let db_corrections = load_correction_snippets_db(session_id);
    let mut correction_snippets = jsonl_corrections;
    for c in db_corrections {
        if !correction_snippets.iter().any(|x| x == &c) {
            correction_snippets.push(c);
        }
    }
    correction_snippets.truncate(3);

    let recent_tools = load_recent_tools_db(session_id);
    let mut focus_paths = extract_paths_from_text(&prompt);
    focus_paths.extend(paths_from_jsonl_tail(session_id));
    if let Some(p) = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str())
    {
        if !focus_paths.iter().any(|x| x == p) {
            focus_paths.push(p.to_string());
        }
    }
    focus_paths.sort();
    focus_paths.dedup();

    let focus_symbols = extract_symbols_from_text(&prompt);
    let prior_line_hashes = if dedup_enabled {
        crate::db::open_db()
            .ok()
            .map(|conn| {
                let _ = crate::db::ensure_schema(&conn);
                super::session_dedup::load_prior_line_hashes(&conn, session_id)
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mode = detect_sgr_mode(&prompt, &correction_snippets, tool_name);

    TaskFrame {
        prompt: prompt.clone(),
        cwd: cwd.to_string(),
        profile: profile.to_string(),
        prompt_keywords: keywords_from_prompt(&prompt),
        correction_snippets,
        recent_tools,
        focus_paths,
        focus_symbols,
        prior_line_hashes,
        mode,
    }
}

pub fn adaptive_target_chars(base_target: usize, frame: &TaskFrame, adaptive: bool) -> usize {
    if !adaptive {
        return base_target;
    }
    match frame.mode {
        SgrMode::Debug => base_target + base_target / 2,
        SgrMode::Scan => base_target.saturating_sub(base_target / 5),
        SgrMode::Normal => base_target,
    }
}

fn detect_sgr_mode(prompt: &str, corrections: &[String], tool_name: &str) -> SgrMode {
    let lower = prompt.to_lowercase();
    if !corrections.is_empty()
        || ["fix", "debug", "error", "failing", "broken", "wrong"]
            .iter()
            .any(|w| lower.contains(w))
    {
        return SgrMode::Debug;
    }
    let tn = tool_name.to_lowercase();
    if (tn == "read" || tn == "grep" || tn == "glob") && !lower.contains("fail") {
        return SgrMode::Scan;
    }
    SgrMode::Normal
}

fn extract_paths_from_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')') {
        let t = token.trim_matches('"').trim_matches('\'');
        if (t.contains('/') || t.ends_with(".rs") || t.ends_with(".ts") || t.ends_with(".js"))
            && t.len() >= 3
            && !out.iter().any(|x| x == t)
        {
            out.push(t.to_string());
        }
    }
    out
}

fn extract_symbols_from_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != ':') {
        if token.is_empty() {
            continue;
        }
        let is_pascal = token
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
            && token.len() > 2;
        let is_snake = token.contains('_') && token.len() > 3;
        let is_pathish = token.contains("::");
        if (is_pascal || is_snake || is_pathish) && !out.iter().any(|x| x == token) {
            out.push(token.to_string());
        }
        if out.len() >= 16 {
            break;
        }
    }
    out
}

fn correction_snippets_from_jsonl(session_id: Option<&str>) -> Vec<String> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Some(path) = crate::hook::find_claude_session_jsonl(sid) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in content.lines().rev().take(200) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|x| x.as_str()) != Some("user") {
            continue;
        }
        let Some(text) = crate::hook::human_text_from_user_json_line(&v) else {
            continue;
        };
        let lower = text.to_lowercase();
        if CORRECTION_PHRASES.iter().any(|p| lower.contains(p)) {
            out.push(text.chars().take(200).collect());
            if out.len() >= 3 {
                break;
            }
        }
    }
    out
}

fn paths_from_jsonl_tail(session_id: Option<&str>) -> Vec<String> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Some(path) = crate::hook::find_claude_session_jsonl(sid) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in content.lines().rev().take(200) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(input) = v.get("tool_input").or_else(|| v.get("input")) {
            for key in ["file_path", "path", "pattern"] {
                if let Some(p) = input.get(key).and_then(|x| x.as_str()) {
                    if p.len() >= 3 && !out.iter().any(|x| x == p) {
                        out.push(p.to_string());
                    }
                }
            }
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

fn load_correction_snippets_db(session_id: Option<&str>) -> Vec<String> {
    let Ok(conn) = crate::db::open_db() else {
        return Vec::new();
    };
    let _ = crate::db::ensure_schema(&conn);
    crate::db::correction_snippets_for_session(&conn, session_id, 3).unwrap_or_default()
}

fn load_recent_tools_db(session_id: Option<&str>) -> Vec<String> {
    let Ok(conn) = crate::db::open_db() else {
        return Vec::new();
    };
    let _ = crate::db::ensure_schema(&conn);
    crate::db::recent_tool_names_for_session(&conn, session_id, 10).unwrap_or_default()
}

pub fn load_prompt_from_session(session_id: Option<&str>) -> String {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let Some(path) = crate::hook::find_claude_session_jsonl(sid) else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    for line in content.lines().rev().take(200) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v.get("type").and_then(|x| x.as_str()) == Some("user") {
                if let Some(text) = crate::hook::human_text_from_user_json_line(&v) {
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
        }
    }
    String::new()
}
