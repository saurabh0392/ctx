//! MCP tool/server hints from CLAUDE.md, editor rules, and Claude settings.
//!
//! Standing instructions can steer agents toward MCP tools that never appear in user prompts.
//! Personal profile generation merges these signals with observed invocation history.

use crate::profiles::{mcp_prefix_to_server_display, mcp_prefix_to_server_id, SERVER_COUNTS};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleMcpSignals {
    pub tool_names: Vec<String>,
    pub server_prefixes: Vec<String>,
}

impl RuleMcpSignals {
    pub fn merge(self, other: RuleMcpSignals) -> Self {
        let mut tools: HashSet<String> = self.tool_names.into_iter().collect();
        tools.extend(other.tool_names);
        let mut prefixes: HashSet<String> = self.server_prefixes.into_iter().collect();
        prefixes.extend(other.server_prefixes);
        let mut tool_names: Vec<String> = tools.into_iter().collect();
        tool_names.sort();
        let mut server_prefixes: Vec<String> = prefixes.into_iter().collect();
        server_prefixes.sort();
        Self {
            tool_names,
            server_prefixes,
        }
    }
}

/// Collect MCP hints from global/project instruction files and settings.
pub fn collect_mcp_signals() -> RuleMcpSignals {
    let mut merged = RuleMcpSignals::default();
    for text in collect_instruction_texts() {
        merged = merged.merge(mcp_signals_from_text(&text));
    }
    merged
}

fn collect_instruction_texts() -> Vec<String> {
    let mut out = Vec::new();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    push_file_text(&mut out, &home.join(".claude").join("CLAUDE.md"));
    collect_rule_dir(&mut out, &home.join(".cursor").join("rules"));

    if let Ok(raw) = std::fs::read_to_string(crate::config::claude_settings_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            collect_json_strings(&v, &mut out, 0);
        }
    }

    for cwd in distinct_working_directories() {
        let root = PathBuf::from(&cwd);
        push_file_text(&mut out, &root.join("CLAUDE.md"));
        push_file_text(&mut out, &root.join("AGENTS.md"));
        collect_rule_dir(&mut out, &root.join(".cursor").join("rules"));
    }

    out
}

fn push_file_text(out: &mut Vec<String>, path: &Path) {
    if !path.is_file() {
        return;
    }
    if let Ok(meta) = path.metadata() {
        if meta.len() > MAX_FILE_BYTES {
            return;
        }
    }
    if let Ok(s) = std::fs::read_to_string(path) {
        let t = s.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
}

fn collect_rule_dir(out: &mut Vec<String>, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e, "mdc" | "md" | "txt"))
        })
        .collect();
    paths.sort();
    for p in paths {
        push_file_text(out, &p);
    }
}

fn collect_json_strings(v: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match v {
        Value::String(s) => {
            if s.contains("mcp__") || s.to_lowercase().contains(" mcp") {
                out.push(s.clone());
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_json_strings(item, out, depth + 1);
            }
        }
        Value::Object(obj) => {
            for (k, val) in obj {
                if k.eq_ignore_ascii_case("rules")
                    || k.eq_ignore_ascii_case("instructions")
                    || k.eq_ignore_ascii_case("systemPrompt")
                    || k.eq_ignore_ascii_case("customInstructions")
                {
                    collect_json_strings(val, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// Parse MCP tool names and server prefixes from instruction text.
pub fn mcp_signals_from_text(text: &str) -> RuleMcpSignals {
    let mut tool_names = HashSet::new();
    let mut server_prefixes = HashSet::new();

    for token in split_tokens(text) {
        if token.starts_with("mcp__") {
            if token.ends_with("__") && token.matches("__").count() >= 2 {
                server_prefixes.insert(token);
            } else if token.matches("__").count() >= 2 {
                if let Some(prefix) = crate::filter::server_prefix_from_tool(&token) {
                    server_prefixes.insert(prefix);
                }
                tool_names.insert(token);
            }
        }
    }

    let hay = text.to_lowercase();
    for (prefix, _) in SERVER_COUNTS {
        if server_prefixes.contains(*prefix) {
            continue;
        }
        let display = mcp_prefix_to_server_display(prefix).to_lowercase();
        let id_spaced = mcp_prefix_to_server_id(prefix)
            .to_lowercase()
            .replace('_', " ");
        if hay.contains(&display)
            || hay.contains(&id_spaced)
            || hay.contains(&format!("{display} mcp"))
            || hay.contains(&format!("{id_spaced} mcp"))
        {
            server_prefixes.insert((*prefix).to_string());
        }
    }

    if let Ok(raw) = std::fs::read_to_string(crate::config::claude_settings_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(obj) = v.get("mcpServers").and_then(|x| x.as_object()) {
                for key in obj.keys() {
                    let kl = key.to_lowercase();
                    if hay.contains(&kl) || hay.contains(&format!("{kl} mcp")) {
                        server_prefixes
                            .insert(format!("mcp__claude_ai_{}__", sanitize_server_key(key)));
                    }
                }
            }
        }
    }

    let mut tool_names: Vec<String> = tool_names.into_iter().collect();
    tool_names.sort();
    let mut server_prefixes: Vec<String> = server_prefixes.into_iter().collect();
    server_prefixes.sort();
    RuleMcpSignals {
        tool_names,
        server_prefixes,
    }
}

fn sanitize_server_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn split_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Distinct non-empty working directories from indexed sessions and hook traces.
pub fn distinct_working_directories() -> Vec<String> {
    let Ok(conn) = crate::db::open_db() else {
        return Vec::new();
    };
    let _ = crate::db::ensure_schema(&conn);
    let mut seen = HashSet::new();
    for sql in [
        "SELECT DISTINCT working_directory FROM sessions WHERE TRIM(COALESCE(working_directory, '')) != ''",
        "SELECT DISTINCT working_directory FROM hook_traces WHERE TRIM(COALESCE(working_directory, '')) != ''",
        "SELECT DISTINCT working_directory FROM requests WHERE TRIM(COALESCE(working_directory, '')) != ''",
    ] {
        if let Ok(mut stmt) = conn.prepare(sql) {
            let _ = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map(|rows| {
                    for row in rows.flatten() {
                        seen.insert(row);
                    }
                });
        }
    }
    let mut out: Vec<String> = seen.into_iter().collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::CTX_ENV_LOCK;

    #[test]
    fn extracts_full_tool_name_from_text() {
        let s = mcp_signals_from_text(
            "Always call mcp__claude_ai_Notion__search before answering wiki questions.",
        );
        assert!(s
            .tool_names
            .contains(&"mcp__claude_ai_Notion__search".to_string()));
        assert!(s
            .server_prefixes
            .contains(&"mcp__claude_ai_Notion__".to_string()));
    }

    #[test]
    fn extracts_server_from_display_name_mention() {
        let s = mcp_signals_from_text(
            "Use the Linear MCP integration to file bugs at the end of every fix.",
        );
        assert!(s
            .server_prefixes
            .contains(&"mcp__claude_ai_Linear__".to_string()));
    }

    #[test]
    fn reads_project_claude_md_for_indexed_cwd() {
        let _guard = CTX_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let proj = tempfile::tempdir().unwrap();
        std::fs::write(
            proj.path().join("CLAUDE.md"),
            "Always use mcp__claude_ai_Slack__send for status updates.",
        )
        .unwrap();

        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
             VALUES ('s1', 'p', datetime('now'), 'all', ?1, 1)",
            [proj.path().to_string_lossy().as_ref()],
        )
        .unwrap();

        let signals = collect_mcp_signals();
        assert!(signals.tool_names.iter().any(|t| t.contains("Slack__send")));

        std::env::remove_var("CTX_HOME");
    }
}
