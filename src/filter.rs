//! Helpers for parsing MCP tool names into server identifiers.
//!
//! These are shared by profile building, conversation analysis, and rule signals. The MITM
//! proxy that once edited the `tools` array in flight was removed in ADR 0015; MCP filtering
//! now happens entirely through Claude Code permission rules (see `filter_control`).

/// Extract a human-readable server display name from an MCP tool name.
/// `mcp__claude_ai_Atlassian__addComment` -> `Atlassian`
pub fn server_display_from_tool(name: &str) -> Option<String> {
    let rest = name.strip_prefix("mcp__claude_ai_")?;
    let end = rest.find("__")?;
    let raw = &rest[..end];
    Some(raw.replace('_', " "))
}

/// `mcp__claude_ai_Atlassian__jira_get` -> `mcp__claude_ai_Atlassian__`
pub fn server_prefix_from_tool(name: &str) -> Option<String> {
    if !name.starts_with("mcp__") {
        return None;
    }
    let parts: Vec<&str> = name.split("__").collect();
    if parts.len() < 3 {
        return None;
    }
    Some(format!("{}__{}__", parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_display_extracts_simple_name() {
        assert_eq!(
            server_display_from_tool("mcp__claude_ai_Atlassian__addComment"),
            Some("Atlassian".to_string())
        );
    }

    #[test]
    fn server_display_replaces_underscores_with_spaces() {
        assert_eq!(
            server_display_from_tool("mcp__claude_ai_Data_Shippo__query"),
            Some("Data Shippo".to_string())
        );
    }

    #[test]
    fn server_display_returns_none_for_non_mcp_tool() {
        assert_eq!(server_display_from_tool("regular_tool_name"), None);
    }

    #[test]
    fn server_prefix_extracts_mcp_server() {
        assert_eq!(
            server_prefix_from_tool("mcp__claude_ai_Atlassian__jira_get_issue"),
            Some("mcp__claude_ai_Atlassian__".into())
        );
    }

    #[test]
    fn server_prefix_returns_none_for_non_mcp() {
        assert_eq!(server_prefix_from_tool("Read"), None);
    }
}
