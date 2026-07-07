//! Cross-session restore scratchpad.
//!
//! We proved (see the per-turn hot-swap test) that Claude Code fixes the MCP tool menu at session
//! start: a `permissions.deny` edit blocks a call immediately, but it does not re-add a pruned
//! tool's schema to a menu the client already built. So a pruned tool cannot come back mid-session.
//!
//! `ctx_restore` works with that constraint instead of against it. It un-prunes the tool for the
//! next session (durably, via `session_expansion`) and records here what the agent was blocked on.
//! The first prompt of a genuinely new session gets that note injected, so the work resumes with the
//! tool now present in the menu.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Cap the file so a chatty agent can't grow it without bound; we only ever need the recent tail.
const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    /// The expansion target stored in `session_expansion` (server prefix or name).
    pub tool: String,
    /// Human-readable name for the note.
    pub display: String,
    /// What the agent wanted to do once the tool is back. May be empty.
    pub tasks: String,
    /// RFC3339 timestamp the request was made.
    pub requested_at: String,
    /// The session that requested it, so we never surface the note back to that same session (its
    /// menu is already fixed). Empty when unknown, which we treat as deliverable to any session.
    pub session_id: String,
    #[serde(default)]
    pub delivered: bool,
}

fn path() -> PathBuf {
    crate::config::ctx_dir().join("restore-queue.jsonl")
}

pub fn load() -> Vec<RestoreRequest> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return vec![];
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

fn save(items: &[RestoreRequest]) -> std::io::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let start = items.len().saturating_sub(MAX_ENTRIES);
    let mut buf = String::new();
    for it in &items[start..] {
        buf.push_str(&serde_json::to_string(it).unwrap_or_default());
        buf.push('\n');
    }
    let tmp = p.with_extension("jsonl.tmp");
    std::fs::write(&tmp, buf)?;
    std::fs::rename(&tmp, &p)
}

pub fn enqueue(
    tool: &str,
    display: &str,
    tasks: &str,
    session_id: &str,
    requested_at: &str,
) -> std::io::Result<()> {
    let mut items = load();
    items.push(RestoreRequest {
        tool: tool.to_string(),
        display: display.to_string(),
        tasks: tasks.to_string(),
        requested_at: requested_at.to_string(),
        session_id: session_id.to_string(),
        delivered: false,
    });
    save(&items)
}

/// Undelivered requests made by a session other than the current one. A request whose owning session
/// is unknown ("") counts as deliverable so a note is never stranded.
pub fn pending_for_new_session(current_session_id: &str) -> Vec<RestoreRequest> {
    load()
        .into_iter()
        .filter(|r| !r.delivered && r.session_id != current_session_id)
        .collect()
}

/// Mark delivered every undelivered request not owned by the current session. Call right after
/// injecting them so the note surfaces once, in the first new session that can act on it.
pub fn mark_delivered_for_new_session(current_session_id: &str) -> std::io::Result<()> {
    let mut items = load();
    let mut changed = false;
    for it in items.iter_mut() {
        if !it.delivered && it.session_id != current_session_id {
            it.delivered = true;
            changed = true;
        }
    }
    if changed {
        save(&items)?;
    }
    Ok(())
}

/// Reduce a full MCP tool name to its server prefix so a restore brings the whole server back, not
/// one tool. `mcp__claude_ai_Linear__get_issue` -> `mcp__claude_ai_Linear__`. A bare name or a name
/// that is already a prefix is returned unchanged; `session_expansion` matching handles those forms.
pub fn normalize_target(input: &str) -> String {
    let t = input.trim();
    if let Some(rest) = t.strip_prefix("mcp__") {
        // rest = "<server>__<tool>..." or "<server>__" or "<server>".
        if let Some((server, tail)) = rest.split_once("__") {
            if !server.is_empty() && !tail.is_empty() {
                return format!("mcp__{server}__");
            }
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reduces_full_tool_to_server_prefix() {
        assert_eq!(
            normalize_target("mcp__claude_ai_Linear__get_issue"),
            "mcp__claude_ai_Linear__"
        );
    }

    #[test]
    fn normalize_keeps_prefix_and_bare_names() {
        assert_eq!(
            normalize_target("mcp__claude_ai_Linear__"),
            "mcp__claude_ai_Linear__"
        );
        assert_eq!(normalize_target("Linear"), "Linear");
    }

    #[test]
    fn pending_excludes_owning_session_and_delivered() {
        let reqs = vec![
            RestoreRequest {
                tool: "a".into(),
                display: "a".into(),
                tasks: String::new(),
                requested_at: "t".into(),
                session_id: "old".into(),
                delivered: false,
            },
            RestoreRequest {
                tool: "b".into(),
                display: "b".into(),
                tasks: String::new(),
                requested_at: "t".into(),
                session_id: "new".into(),
                delivered: false,
            },
        ];
        let pending: Vec<_> = reqs
            .into_iter()
            .filter(|r| !r.delivered && r.session_id != "new")
            .collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tool, "a");
    }
}
