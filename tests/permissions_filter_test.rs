//! Soft filter: permissions.deny merge and hook without allowedMcpServers.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::process::Command;

#[test]
#[serial]
fn deny_patterns_generated_for_data_profile() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "data"
filter_mode = "soft"
"#,
    );
    let p = ctx::profiles::get("data").unwrap();
    let patterns = ctx::profiles::deny_patterns_for_profile(&p, &[], &[]);
    assert!(patterns.iter().any(|s| s == "mcp__claude_ai_Figma__*"));
    assert!(!patterns
        .iter()
        .any(|s| s == "mcp__claude_ai_Data_Shippo__*"));
}

#[test]
#[serial]
fn apply_profile_soft_writes_deny_not_allowlist() {
    let h = CtxHarness::new();
    let settings_path = h.tmp.path().join("claude_settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    std::fs::write(&settings_path, r#"{"permissions":{"deny":["Bash(rm *)"]}}"#).unwrap();

    std::env::set_var("CTX_HOME", h.tmp.path());
    std::env::set_var("HOME", h.tmp.path());

    h.write_config(
        r#"
active_profile = "all"
filter_mode = "soft"
"#,
    );

    // Point claude settings at temp home
    let claude_dir = h.tmp.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{"permissions":{"deny":["Bash(rm *)"]}}"#,
    )
    .unwrap();

    ctx::profiles::apply_profile("data", true, true).unwrap();

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(claude_dir.join("settings.json")).unwrap())
            .unwrap();
    assert!(doc.get("allowedMcpServers").is_none());
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert!(deny.iter().any(|v| v.as_str() == Some("Bash(rm *)")));
    assert!(deny.iter().any(|v| {
        v.as_str()
            .map(|s| s.starts_with("mcp__claude_ai_") && s.ends_with("__*"))
            .unwrap_or(false)
    }));

    std::env::remove_var("CTX_HOME");
    std::env::remove_var("HOME");
}

#[test]
#[serial]
fn hook_subprocess_soft_mode_no_allowed_mcp_servers() {
    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("integration tests need the ctx binary");
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "data"
filter_mode = "soft"
auto_profile_enabled = false
inject_enabled = false
coaching_enabled = false
adaptive_prefix_enabled = false
"#,
    );
    let claude_dir = h.tmp.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    std::env::set_var("CTX_HOME", h.tmp.path());
    std::env::set_var("HOME", h.tmp.path());

    ctx::profiles::apply_profile("data", true, true).unwrap();

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "fix figma export",
        "model": "claude-sonnet-4-20250514"
    })
    .to_string();

    ctx::profiles::apply_profile("data", true, true).unwrap();

    let out = Command::new(bin)
        .args(["hook", "user-prompt-submit"])
        .current_dir(h.tmp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(claude_dir.join("settings.json")).unwrap())
            .unwrap();
    assert!(doc.get("allowedMcpServers").is_none());

    std::env::remove_var("CTX_HOME");
    std::env::remove_var("HOME");
}

#[test]
#[serial]
fn strip_ctx_deny_on_uninstall_path() {
    let mut doc = serde_json::json!({
        "permissions": {
            "deny": ["Bash(x)", "mcp__claude_ai_Figma__*"]
        }
    });
    assert!(ctx::claude_settings::strip_ctx_deny_rules(&mut doc));
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert_eq!(deny.len(), 1);
    assert_eq!(deny[0], "Bash(x)");
}

#[test]
#[serial]
fn is_ctx_managed_deny_recognizes_per_tool_names() {
    assert!(ctx::profiles::is_ctx_managed_deny_pattern(
        "mcp__claude_ai_Atlassian__jira_get_issue"
    ));
    assert!(ctx::profiles::is_ctx_managed_deny_pattern(
        "mcp__claude_ai_Figma__*"
    ));
    assert!(!ctx::profiles::is_ctx_managed_deny_pattern("Bash(rm *)"));
}

#[test]
#[serial]
fn deny_patterns_tool_level_denies_individual_tools() {
    let h = CtxHarness::new();
    h.write_config("active_profile = \"all\"\n");
    let custom = h.tmp.path().join("profiles.toml");
    std::fs::write(
        &custom,
        r#"
[granular]
display = "Granular"
description = "Tool-level test"
keep = []
keep_tools = [
  "mcp__claude_ai_Atlassian__jira_get_issue",
  "mcp__claude_ai_Slack__send_message",
]
"#,
    )
    .unwrap();

    std::env::set_var("CTX_HOME", h.tmp.path());
    let p = ctx::profiles::get("granular").unwrap();
    assert!(p.uses_tool_level());
    let patterns = ctx::profiles::deny_patterns_for_profile(&p, &[], &[]);
    assert!(!patterns.iter().any(|s| s.ends_with("__*")));
    assert!(!patterns.contains(&"mcp__claude_ai_Atlassian__jira_get_issue".to_string()));
    std::env::remove_var("CTX_HOME");
}

#[test]
#[serial]
fn strip_ctx_deny_removes_per_tool_entries() {
    let mut doc = serde_json::json!({
        "permissions": {
            "deny": [
                "Bash(x)",
                "mcp__claude_ai_Figma__get_file",
                "mcp__claude_ai_Figma__*"
            ]
        }
    });
    assert!(ctx::claude_settings::strip_ctx_deny_rules(&mut doc));
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert_eq!(deny.len(), 1);
    assert_eq!(deny[0], "Bash(x)");
}
