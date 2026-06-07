//! Golden fixture tests for output compression.

use ctx::compress::{
    classify::classify_bash_command, compress_tool_output, extract_tool_output,
    tool_response_value, wrap_updated_tool_output, CompressKind,
};
use ctx::config::Config;
use serde_json::{json, Value};

fn test_cfg() -> Config {
    Config {
        compress_enabled: true,
        compress_max_output_chars: 12_000,
        compress_target_chars: 400,
        compress_tools: vec![
            "Bash".into(),
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
        ],
        compress_redact_secrets: true,
        compress_preserve_errors: true,
        ..Default::default()
    }
}

#[test]
fn classify_git_status_command() {
    assert_eq!(
        classify_bash_command("git status -sb"),
        CompressKind::GitStatus
    );
}

#[test]
fn classify_cargo_test_command() {
    assert_eq!(
        classify_bash_command("cargo test -p ctx"),
        CompressKind::TestRunner
    );
}

#[test]
fn golden_git_status_fixture() {
    let raw = include_str!("fixtures/compress/git_status_raw.txt");
    let cfg = test_cfg();
    let r = compress_tool_output(
        "Bash",
        &json!({"command": "git status -sb"}),
        raw,
        &cfg,
        None,
        "/tmp/project",
        false,
    )
    .expect("git status fixture should compress");
    assert!(r.chars_saved() > 500);
    assert_eq!(r.strategy, "git-status");
    assert!(
        r.text.contains("Staged")
            || r.text.contains("Modified")
            || r.text.contains("generated/file")
    );
}

#[test]
fn golden_cargo_test_fixture() {
    let raw = include_str!("fixtures/compress/cargo_test_raw.txt");
    let cfg = test_cfg();
    let r = compress_tool_output(
        "Bash",
        &json!({"command": "cargo test compress"}),
        raw,
        &cfg,
        None,
        "/tmp",
        false,
    )
    .expect("cargo test fixture should compress");
    assert!(r.chars_saved() > 200);
    assert!(r.text.contains("FAILED") || r.text.contains("failure"));
    assert_eq!(r.strategy, "test-runner");
}

#[test]
fn golden_grep_fixture() {
    let raw = include_str!("fixtures/compress/grep_raw.txt");
    let cfg = test_cfg();
    let r = compress_tool_output(
        "Grep",
        &json!({"pattern": "compress_tool_output"}),
        raw,
        &cfg,
        None,
        "/tmp",
        false,
    )
    .expect("grep fixture should compress");
    assert!(r.chars_saved() > 100);
    assert_eq!(r.strategy, "grep");
}

#[test]
fn golden_read_fixture() {
    let raw = include_str!("fixtures/compress/read_raw.txt");
    let cfg = test_cfg();
    let r = compress_tool_output(
        "Read",
        &json!({"file_path": "src/compress/mod.rs"}),
        raw,
        &cfg,
        Some("fix compress_tool_output handler"),
        "/tmp/project",
        false,
    )
    .expect("read fixture should compress");
    assert!(r.chars_saved() > 500);
    assert_eq!(r.strategy, "read");
    assert!(r.text.contains("handler_") || r.text.contains("compress"));
}

#[test]
fn golden_mcp_fixture() {
    let raw = include_str!("fixtures/compress/mcp_raw.json");
    let cfg = test_cfg();
    let r = compress_tool_output(
        "mcp__notion__search",
        &json!({}),
        raw,
        &cfg,
        None,
        "/tmp",
        false,
    )
    .expect("mcp fixture should compress");
    assert!(r.chars_saved() > 100);
    assert!(r.strategy.starts_with("mcp-json"));
}

#[test]
fn redacts_secrets_in_generic_output() {
    let mut body = String::from("export API_KEY=sk-ant-api03-abc123secretvalue\n");
    for i in 0..50 {
        body.push_str(&format!("line {i} padding text for budget\n"));
    }
    let cfg = test_cfg();
    let r = compress_tool_output(
        "Bash",
        &json!({"command": "env"}),
        &body,
        &cfg,
        None,
        "/tmp",
        false,
    )
    .expect("should compress");
    assert!(!r.text.contains("sk-ant"));
    assert!(r.text.contains("redacted"));
}

#[test]
fn hook_payload_contract() {
    let payload: Value =
        serde_json::from_str(include_str!("fixtures/compress/hook_payload.json")).expect("fixture json");
    let response = tool_response_value(&payload).expect("tool_response");
    let raw = extract_tool_output(&payload);
    assert!(raw.contains("On branch"));

    let cfg = test_cfg();
    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let result = compress_tool_output(
        tool_name,
        payload.get("tool_input").unwrap_or(&json!({})),
        &raw,
        &cfg,
        payload.get("session_id").and_then(|v| v.as_str()),
        payload.get("cwd").and_then(|v| v.as_str()).unwrap_or(""),
        false,
    )
    .expect("hook payload should compress");

    let updated = wrap_updated_tool_output(tool_name, &response, &result.text);
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": updated,
            "additionalContext": format!(
                "ctx compressed this tool output ({} to {} chars). The tool still ran successfully.",
                result.chars_in,
                result.chars_out
            )
        }
    });
    let hso = out.get("hookSpecificOutput").expect("hookSpecificOutput");
    assert_eq!(hso.get("hookEventName").and_then(|v| v.as_str()), Some("PostToolUse"));
    let uto = hso.get("updatedToolOutput").expect("updatedToolOutput");
    assert!(uto.is_object(), "Bash updatedToolOutput must stay an object");
    assert!(
        uto.get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .len()
            < raw.len()
    );
    assert!(hso.get("additionalContext").and_then(|v| v.as_str()).unwrap_or("").contains("compressed"));
}

#[test]
fn hook_read_structured_roundtrip() {
    let payload = json!({
        "session_id": "s1",
        "cwd": "/tmp/project",
        "tool_name": "Read",
        "tool_input": {"file_path": "src/compress/mod.rs"},
        "tool_response": {
            "file": {
                "filePath": "src/compress/mod.rs",
                "content": include_str!("fixtures/compress/read_raw.txt")
            }
        }
    });
    let response = tool_response_value(&payload).unwrap();
    let raw = extract_tool_output(&payload);
    assert!(raw.contains("handler_0"));

    let cfg = test_cfg();
    let result = compress_tool_output(
        "Read",
        &payload["tool_input"],
        &raw,
        &cfg,
        Some("fix compress"),
        "/tmp/project",
        false,
    )
    .expect("read should compress");

    let updated = wrap_updated_tool_output("Read", &response, &result.text);
    assert!(updated.get("file").is_some());
    assert!(
        updated
            .pointer("/file/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .len()
            < raw.len()
    );
}

fn test_cfg_sgr() -> Config {
    Config {
        compress_sgr_enabled: true,
        compress_adaptive_budget: true,
        ..test_cfg()
    }
}

#[test]
fn golden_sgr_read_keeps_focus_path() {
    let raw = include_str!("fixtures/compress/read_raw.txt");
    let cfg = test_cfg_sgr();
    let r = compress_tool_output(
        "Read",
        &json!({"file_path": "src/compress/mod.rs"}),
        raw,
        &cfg,
        None,
        "/tmp/project",
        true,
    )
    .expect("sgr read should compress");
    assert!(r.strategy.contains("sgr"));
    assert!(r.text.contains("compress") || r.text.contains("mod.rs"));
}

#[test]
fn hook_extract_output() {
    let payload = json!({
        "tool_name": "Bash",
        "tool_input": {"command": "git status"},
        "tool_response": "On branch main\n\nUntracked files:\n  foo.rs\n"
    });
    assert!(extract_tool_output(&payload).contains("On branch"));
}
