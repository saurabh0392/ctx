#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn ctx() -> &'static str {
    env!("CARGO_BIN_EXE_ctx")
}

fn echo_server(temp: &TempDir) -> std::path::PathBuf {
    let path = temp.path().join("echo-mcp.sh");
    std::fs::write(
        &path,
        "#!/bin/sh\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

fn trimming_server(temp: &TempDir, long_text: &str) -> std::path::PathBuf {
    let path = temp.path().join("trimming-mcp.sh");
    let responses = [
        serde_json::json!({
            "jsonrpc":"2.0","id":1,
            "result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}
        }),
        serde_json::json!({
            "jsonrpc":"2.0","id":2,
            "result":{"tools":[{"name":"read","inputSchema":{"type":"object"}}]}
        }),
        serde_json::json!({
            "jsonrpc":"2.0","id":3,
            "result":{
                "content":[
                    {"type":"text","text":long_text,"vendor":"kept"},
                    {"type":"image","data":"aGVsbG8=","mimeType":"image/png","annotations":{"audience":["assistant"]}}
                ],
                "isError":false,
                "_meta":{"trace":"exact"}
            }
        }),
    ];
    let response_lines = responses
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect::<Vec<_>>();
    let script = format!(
        "#!/bin/sh\ni=0\nwhile IFS= read -r line; do\n  i=$((i+1))\n  case $i in\n    1) printf '%s\\n' '{}' ;;\n    2) printf '%s\\n' '{}' ;;\n    3) printf '%s\\n' '{}' ;;\n  esac\ndone\n",
        response_lines[0], response_lines[1], response_lines[2]
    );
    std::fs::write(&path, script).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

fn command(temp: &TempDir) -> Command {
    let mut command = Command::new(ctx());
    command
        .env("CTX_HOME", temp.path().join("ctx-home"))
        .env("CTX_TEST_HOME", temp.path());
    command
}

#[test]
fn stdio_gateway_is_byte_identical_when_trimming_is_off() {
    let temp = TempDir::new().unwrap();
    let server = echo_server(&temp);
    let status = command(&temp)
        .args([
            "gateway",
            "add-stdio",
            "echo",
            "--command",
            server.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let mut gateway = command(&temp)
        .args(["gateway", "serve", "echo", "--surface", "codex"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let frames = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"a\",\"method\":\"unknown/vendor\",\"params\":{\"x\":1}}\n"
    );
    gateway
        .stdin
        .as_mut()
        .unwrap()
        .write_all(frames.as_bytes())
        .unwrap();
    drop(gateway.stdin.take());
    let output = gateway.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, frames.as_bytes());
}

#[test]
fn codex_rewire_preserves_policy_and_restores_original_definition() {
    let temp = TempDir::new().unwrap();
    let server = echo_server(&temp);
    let codex_dir = temp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let config_path = codex_dir.join("config.toml");
    let original = format!(
        "model = \"gpt-test\"\n\n[mcp_servers.echo]\ncommand = {:?}\nargs = [\"--fixture\"]\nenabled = true\nenabled_tools = [\"read\"]\n",
        server.to_string_lossy()
    );
    std::fs::write(&config_path, &original).unwrap();
    let original_value: toml::Value = toml::from_str(&original).unwrap();

    let enabled = command(&temp)
        .args(["gateway", "codex-enable", "echo"])
        .output()
        .unwrap();
    assert!(
        enabled.status.success(),
        "{}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let routed: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    let table = routed["mcp_servers"]["echo"].as_table().unwrap();
    assert_eq!(table["enabled"].as_bool(), Some(true));
    assert_eq!(table["enabled_tools"][0].as_str(), Some("read"));
    assert!(table["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str() == Some("gateway")));

    let disabled = command(&temp)
        .args(["gateway", "codex-disable", "echo"])
        .output()
        .unwrap();
    assert!(
        disabled.status.success(),
        "{}",
        String::from_utf8_lossy(&disabled.stderr)
    );
    let restored: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(restored, original_value);
}

#[test]
fn applied_gateway_trim_preserves_non_text_and_is_exactly_recoverable() {
    let temp = TempDir::new().unwrap();
    let ctx_home = temp.path().join("ctx-home");
    std::fs::create_dir_all(&ctx_home).unwrap();
    std::fs::write(
        ctx_home.join("config.toml"),
        "compress_enabled = true\ncompress_shadow_enabled = true\ncompress_trial_tools = [\"mcp__fixture__read\"]\ncompress_target_chars = 300\ncompress_max_output_chars = 100000\n",
    )
    .unwrap();
    let long_text = (0..240)
        .map(|index| format!("result line {index}: deterministic fixture content"))
        .collect::<Vec<_>>()
        .join("\n");
    let server = trimming_server(&temp, &long_text);
    assert!(command(&temp)
        .args([
            "gateway",
            "add-stdio",
            "fixture",
            "--command",
            server.to_str().unwrap(),
        ])
        .status()
        .unwrap()
        .success());

    let mut gateway = command(&temp)
        .args(["gateway", "serve", "fixture", "--surface", "codex"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{}}}\n"
    );
    gateway
        .stdin
        .as_mut()
        .unwrap()
        .write_all(requests.as_bytes())
        .unwrap();
    drop(gateway.stdin.take());
    let output = gateway.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    let result = &responses[2]["result"];
    let trimmed = result["content"][0]["text"].as_str().unwrap();
    assert!(trimmed.len() < long_text.len());
    assert_eq!(result["content"][0]["vendor"], "kept");
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["data"], "aGVsbG8=");
    assert_eq!(result["_meta"]["trace"], "exact");

    let connection = rusqlite::Connection::open(ctx_home.join("ctx.db")).unwrap();
    let (original, rewind_id): (String, String) = connection
        .query_row(
            "SELECT r.original, d.rewind_id FROM compress_decisions d JOIN rewind_store r ON r.id=d.rewind_id WHERE d.applied=1 AND d.tool_name='mcp__fixture__read'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(rewind_id.starts_with("mcp-"));
    let original: serde_json::Value = serde_json::from_str(&original).unwrap();
    assert_eq!(original["content"][0]["text"], long_text);
    assert_eq!(original["content"][1], result["content"][1]);
}
