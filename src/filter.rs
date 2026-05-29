use crate::config::Config;
use crate::profiles::{load_all, Profile};

fn active_profile() -> Profile {
    let config = Config::load();
    let slug = config.active_profile.as_deref().unwrap_or("all");
    load_all().remove(slug).unwrap_or_else(|| Profile {
        display: "All".into(),
        description: String::new(),
        ..Default::default()
    })
}

pub struct FilterResult {
    pub body: Vec<u8>,
    pub tools_removed: usize,
    pub removed_servers: Vec<String>,
    pub kept_servers: Vec<String>,
}

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

/// Partition tools using a profile and return full trace info.
pub fn filter_with_trace(body: &[u8], profile: &Profile) -> FilterResult {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return FilterResult { body: body.to_vec(), tools_removed: 0, removed_servers: vec![], kept_servers: vec![] };
    };

    let Some(tools) = value.get_mut("tools").and_then(|t| t.as_array_mut()) else {
        return FilterResult { body: body.to_vec(), tools_removed: 0, removed_servers: vec![], kept_servers: vec![] };
    };

    if !profile.filtering_enabled() {
        return FilterResult { body: body.to_vec(), tools_removed: 0, removed_servers: vec![], kept_servers: vec![] };
    }

    let mut removed_servers: std::collections::HashMap<String, ()> = Default::default();
    let mut kept_servers: std::collections::HashMap<String, ()> = Default::default();

    let before = tools.len();
    tools.retain(|tool| {
        let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let filtered = profile.filters_tool(name);
        if let Some(sname) = server_display_from_tool(name) {
            if filtered {
                removed_servers.insert(sname, ());
            } else {
                kept_servers.insert(sname, ());
            }
        }
        !filtered
    });
    let tools_removed = before - tools.len();

    if tools_removed == 0 {
        return FilterResult { body: body.to_vec(), tools_removed: 0, removed_servers: vec![], kept_servers: vec![] };
    }

    let body = serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec());
    let mut removed_servers: Vec<String> = removed_servers.into_keys().collect();
    let mut kept_servers: Vec<String> = kept_servers.into_keys().collect();
    removed_servers.sort();
    kept_servers.sort();

    FilterResult { body, tools_removed, removed_servers, kept_servers }
}

/// Strip MCP tool definitions from an Anthropic API request body based on active profile.
pub fn filter_request(body: &[u8]) -> FilterResult {
    let profile = active_profile();
    filter_with_trace(body, &profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile(keep: Vec<&str>) -> Profile {
        Profile {
            display: "test".into(),
            description: String::new(),
            keep: keep.into_iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn make_tool_profile(tools: Vec<&str>) -> Profile {
        Profile {
            display: "test".into(),
            description: String::new(),
            keep_tools: tools.into_iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn body_with_tools(tools: &[&str]) -> Vec<u8> {
        let tool_arr: Vec<_> = tools
            .iter()
            .map(|name| serde_json::json!({"name": name, "input_schema": {"type": "object"}}))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "model": "claude-test",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": tool_arr
        }))
        .unwrap()
    }

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
    fn filter_passthrough_when_keep_is_empty() {
        let profile = make_profile(vec![]);
        let body = body_with_tools(&["mcp__claude_ai_Slack__send", "mcp__claude_ai_Figma__get"]);
        let result = filter_with_trace(&body, &profile);
        assert_eq!(result.tools_removed, 0);
        let parsed: serde_json::Value = serde_json::from_slice(&result.body).unwrap();
        assert_eq!(parsed["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn filter_removes_tools_not_in_keep_list() {
        let profile = make_profile(vec!["mcp__claude_ai_Slack__"]);
        let body = body_with_tools(&["mcp__claude_ai_Slack__send", "mcp__claude_ai_Figma__get"]);
        let result = filter_with_trace(&body, &profile);
        assert_eq!(result.tools_removed, 1);
        let parsed: serde_json::Value = serde_json::from_slice(&result.body).unwrap();
        let tools = parsed["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "mcp__claude_ai_Slack__send");
    }

    #[test]
    fn filter_passthrough_when_no_tools_key_in_body() {
        let profile = make_profile(vec!["mcp__claude_ai_Slack__"]);
        let body =
            serde_json::to_vec(&serde_json::json!({"model": "test", "messages": []})).unwrap();
        let result = filter_with_trace(&body, &profile);
        assert_eq!(result.tools_removed, 0);
    }

    #[test]
    fn filter_passthrough_on_invalid_json() {
        let profile = make_profile(vec!["mcp__claude_ai_Slack__"]);
        let result = filter_with_trace(b"not json", &profile);
        assert_eq!(result.tools_removed, 0);
        assert_eq!(result.body, b"not json");
    }

    #[test]
    fn filter_tracks_removed_and_kept_servers() {
        let profile = make_profile(vec!["mcp__claude_ai_Slack__"]);
        let body = body_with_tools(&["mcp__claude_ai_Slack__send", "mcp__claude_ai_Figma__get"]);
        let result = filter_with_trace(&body, &profile);
        assert!(result.removed_servers.contains(&"Figma".to_string()));
        assert!(result.kept_servers.contains(&"Slack".to_string()));
    }

    #[test]
    fn filter_zero_removed_when_all_tools_match() {
        let profile = make_profile(vec!["mcp__claude_ai_Slack__", "mcp__claude_ai_Figma__"]);
        let body = body_with_tools(&["mcp__claude_ai_Slack__send", "mcp__claude_ai_Figma__get"]);
        let result = filter_with_trace(&body, &profile);
        assert_eq!(result.tools_removed, 0);
    }

    #[test]
    fn filter_keep_tools_strips_only_unlisted_tools() {
        let profile = make_tool_profile(vec![
            "mcp__claude_ai_Atlassian__jira_get_issue",
            "mcp__claude_ai_Slack__send_message",
        ]);
        let body = body_with_tools(&[
            "mcp__claude_ai_Atlassian__jira_get_issue",
            "mcp__claude_ai_Atlassian__jira_search",
            "mcp__claude_ai_Slack__send_message",
        ]);
        let result = filter_with_trace(&body, &profile);
        assert_eq!(result.tools_removed, 1);
        let parsed: serde_json::Value = serde_json::from_slice(&result.body).unwrap();
        let names: Vec<_> = parsed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"mcp__claude_ai_Atlassian__jira_search"));
        assert!(names.contains(&"mcp__claude_ai_Atlassian__jira_get_issue"));
    }
}
