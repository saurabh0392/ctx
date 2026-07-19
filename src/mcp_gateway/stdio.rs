use std::collections::HashMap;
use std::process::Stdio;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::registry::StdioServer;
use crate::agent::ToolResult;
use crate::tool_result::{
    parse_mcp_result, parse_mcp_tools_list, McpApplyRequest, McpPrepareOutcome,
    McpToolContractCache, McpToolContractKey, PreparedMcpTrim,
};

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_REQUESTS: usize = 1_024;

#[derive(Debug)]
enum PendingRequest {
    Initialize,
    ToolsList,
    ToolCall {
        name: String,
        input: Value,
        started_at: Instant,
    },
    Other,
}

pub(super) struct RelayState {
    server_id: String,
    surface: String,
    protocol_version: String,
    cwd: String,
    pending: HashMap<String, PendingRequest>,
    contracts: McpToolContractCache,
}

impl RelayState {
    pub(super) fn new(server_id: &str, surface: &str, cwd: String) -> Self {
        Self {
            server_id: server_id.to_owned(),
            surface: surface.to_owned(),
            protocol_version: "unknown".into(),
            cwd,
            pending: HashMap::new(),
            contracts: McpToolContractCache::new(512),
        }
    }

    pub(super) fn protocol_version(&self) -> &str {
        &self.protocol_version
    }
}

pub async fn serve(server_id: &str, surface: &str, server: &StdioServer) -> Result<()> {
    let mut command = Command::new(&server.command);
    command
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .env_clear();
    copy_baseline_environment(&mut command);
    for name in &server.pass_env {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(cwd) = &server.cwd {
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("spawn approved MCP server {}", server.command.display()))?;
    let mut child_stdin = Some(child.stdin.take().context("capture MCP server stdin")?);
    let child_stdout = child.stdout.take().context("capture MCP server stdout")?;
    let mut server_reader = BufReader::new(child_stdout);
    let mut client_reader = BufReader::new(tokio::io::stdin());
    let mut client_stdout = tokio::io::stdout();
    let cwd = server
        .cwd
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_string_lossy()
        .into_owned();
    let mut state = RelayState::new(server_id, surface, cwd);

    loop {
        tokio::select! {
            client = read_frame(&mut client_reader) => {
                let Some(frame) = client.context("read agent MCP frame")? else {
                    if let Some(mut stdin) = child_stdin.take() {
                        stdin.shutdown().await.ok();
                    }
                    while let Some(frame) = read_frame(&mut server_reader)
                        .await
                        .context("drain MCP server after agent EOF")?
                    {
                        let (outgoing, applied) = transform_server_frame(&mut state, &frame);
                        client_stdout
                            .write_all(&outgoing)
                            .await
                            .context("emit drained MCP response")?;
                        client_stdout.flush().await.context("flush drained MCP response")?;
                        if let Some(prepared) = applied {
                            if let Err(error) = crate::tool_result::mark_mcp_trim_emitted(&prepared) {
                                eprintln!("ctx gateway: result was emitted but applied receipt failed: {error}");
                            }
                        }
                    }
                    break;
                };
                observe_client_frame(&mut state, &frame)?;
                let stdin = child_stdin.as_mut().context("MCP server stdin is closed")?;
                stdin.write_all(&frame).await.context("forward MCP request")?;
                stdin.flush().await.context("flush MCP request")?;
            }
            server_frame = read_frame(&mut server_reader) => {
                let Some(frame) = server_frame.context("read server MCP frame")? else {
                    break;
                };
                let (outgoing, applied) = transform_server_frame(&mut state, &frame);
                client_stdout.write_all(&outgoing).await.context("emit MCP response")?;
                client_stdout.flush().await.context("flush MCP response")?;
                if let Some(prepared) = applied {
                    if let Err(error) = crate::tool_result::mark_mcp_trim_emitted(&prepared) {
                        eprintln!("ctx gateway: result was emitted but applied receipt failed: {error}");
                    }
                }
            }
        }
    }

    drop(child_stdin.take());
    let status = match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
        Ok(status) => status.context("wait for MCP server")?,
        Err(_) => {
            child.kill().await.context("stop unresponsive MCP server")?;
            child.wait().await.context("reap MCP server")?
        }
    };
    if !status.success() {
        crate::db::record_gateway_runtime_event_best_effort(
            surface,
            server_id,
            "failure",
            None,
            Some("stdio-server-exited-unsuccessfully"),
        );
        anyhow::bail!("MCP server exited with {status}");
    }
    Ok(())
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<Option<Vec<u8>>> {
    read_frame_bounded(reader, MAX_FRAME_BYTES).await
}

async fn read_frame_bounded<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
) -> Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Ok(Some(frame))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if frame.len().saturating_add(take) > limit {
            anyhow::bail!("MCP frame exceeds {limit} bytes");
        }
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

pub(super) fn observe_client_frame(state: &mut RelayState, frame: &[u8]) -> Result<()> {
    let Ok(message) = serde_json::from_slice::<Value>(trim_newline(frame)) else {
        return Ok(());
    };
    let Some(id) = message.get("id").map(request_key) else {
        return Ok(());
    };
    if state.pending.len() >= MAX_PENDING_REQUESTS {
        anyhow::bail!("too many in-flight MCP requests");
    }
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let pending = match method {
        "initialize" => {
            if let Some(version) = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
            {
                state.protocol_version = version.to_owned();
            }
            PendingRequest::Initialize
        }
        "tools/list" => PendingRequest::ToolsList,
        "tools/call" => {
            let Some(name) = message.pointer("/params/name").and_then(Value::as_str) else {
                return Ok(());
            };
            PendingRequest::ToolCall {
                name: name.to_owned(),
                input: message
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Default::default())),
                started_at: Instant::now(),
            }
        }
        _ => PendingRequest::Other,
    };
    state.pending.insert(id, pending);
    Ok(())
}

pub(super) fn transform_server_frame(
    state: &mut RelayState,
    frame: &[u8],
) -> (Vec<u8>, Option<PreparedMcpTrim>) {
    let Ok(mut message) = serde_json::from_slice::<Value>(trim_newline(frame)) else {
        return (frame.to_vec(), None);
    };
    let Some(id) = message.get("id").map(request_key) else {
        return (frame.to_vec(), None);
    };
    let Some(pending) = state.pending.remove(&id) else {
        return (frame.to_vec(), None);
    };
    match pending {
        PendingRequest::Initialize => {
            if let Some(version) = message
                .pointer("/result/protocolVersion")
                .and_then(Value::as_str)
            {
                state.protocol_version = version.to_owned();
            }
        }
        PendingRequest::ToolsList => {
            if let Some(result) = message.get("result") {
                if let Ok(list) = parse_mcp_tools_list(result) {
                    let _ = state.contracts.capture_tools_list(
                        &state.server_id,
                        &state.protocol_version,
                        &list,
                    );
                }
            }
        }
        PendingRequest::ToolCall {
            name,
            input,
            started_at,
        } => {
            let latency_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let Some(result_value) = message.get("result").cloned() else {
                crate::db::record_gateway_runtime_event_best_effort(
                    &state.surface,
                    &state.server_id,
                    "pass_through",
                    Some(latency_ms),
                    Some("unsupported-shape: missing result"),
                );
                return (frame.to_vec(), None);
            };
            let Ok(canonical) = parse_mcp_result(&result_value) else {
                crate::db::record_gateway_runtime_event_best_effort(
                    &state.surface,
                    &state.server_id,
                    "pass_through",
                    Some(latency_ms),
                    Some("schema-failure: invalid MCP result envelope"),
                );
                return (frame.to_vec(), None);
            };
            let contract = state
                .contracts
                .get(&McpToolContractKey::new(
                    &state.server_id,
                    &state.protocol_version,
                    &name,
                ))
                .cloned();
            let cfg = crate::config::Config::load();
            let candidate = crate::compress::propose_mcp_apply_candidate(
                &canonical,
                contract.as_ref(),
                &input,
                &cfg,
                &state.cwd,
            );
            let candidate = match candidate {
                Ok(candidate) => candidate,
                Err(reason) => {
                    crate::db::record_gateway_runtime_event_best_effort(
                        &state.surface,
                        &state.server_id,
                        "pass_through",
                        Some(latency_ms),
                        Some(reason),
                    );
                    return (frame.to_vec(), None);
                }
            };
            let tool_name = format!("mcp__{}__{}", state.server_id, name);
            let decision = crate::agent::decide_for_surface(
                &cfg,
                &ToolResult {
                    tool_name: tool_name.clone(),
                    tool_input: input.clone(),
                    raw_output: canonical.compressible_text().unwrap_or_default(),
                    canonical_mcp: Some(canonical.clone()),
                    session_id: None,
                    cwd: state.cwd.clone(),
                    recent_intent_text: None,
                },
                &state.surface,
            );
            let command_or_path = format!("{}/{}", state.server_id, name);
            let request = McpApplyRequest {
                surface: &state.surface,
                server_id: &state.server_id,
                protocol_version: &state.protocol_version,
                tool_name: &tool_name,
                tool_input: &input,
                session_id: None,
                command_or_path: &command_or_path,
                contract: contract.as_ref(),
                manifest: candidate.manifest,
                proposal: &candidate.proposal,
                authorized: decision.apply,
                transport_latency_ms: Some(latency_ms),
            };
            if let McpPrepareOutcome::Ready(prepared) =
                crate::tool_result::prepare_mcp_trim(&canonical, &request)
            {
                message["result"] = prepared.result.clone();
                let mut outgoing = serde_json::to_vec(&message).unwrap_or_else(|_| frame.to_vec());
                outgoing.push(b'\n');
                return (outgoing, Some(*prepared));
            }
        }
        PendingRequest::Other => {}
    }
    (frame.to_vec(), None)
}

fn request_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_else(|_| "null".into())
}

fn trim_newline(frame: &[u8]) -> &[u8] {
    frame
        .strip_suffix(b"\n")
        .and_then(|line| line.strip_suffix(b"\r").or(Some(line)))
        .unwrap_or(frame)
}

fn copy_baseline_environment(command: &mut Command) {
    const NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "WINDIR",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
    ];
    for name in NAMES {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn correlates_out_of_order_response_ids_without_touching_unknown_messages() {
        let mut state = RelayState {
            server_id: "fixture".into(),
            surface: "codex".into(),
            protocol_version: "v".into(),
            cwd: ".".into(),
            pending: HashMap::new(),
            contracts: McpToolContractCache::new(8),
        };
        observe_client_frame(&mut state, br#"{"jsonrpc":"2.0","id":7,"method":"ping"}\n"#).unwrap();
        observe_client_frame(
            &mut state,
            br#"{"jsonrpc":"2.0","id":"a","method":"ping"}\n"#,
        )
        .unwrap();
        let second = br#"{"jsonrpc":"2.0","id":"a","result":{"ok":true}}\n"#;
        let first = br#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}\n"#;
        assert_eq!(transform_server_frame(&mut state, second).0, second);
        assert_eq!(transform_server_frame(&mut state, first).0, first);
        assert!(state.pending.is_empty());
        let notification = json!({"jsonrpc":"2.0","method":"notifications/progress"});
        let mut frame = serde_json::to_vec(&notification).unwrap();
        frame.push(b'\n');
        assert_eq!(transform_server_frame(&mut state, &frame).0, frame);
    }

    #[tokio::test]
    async fn frame_reader_stops_at_the_bound_without_waiting_for_a_newline() {
        let input = std::io::Cursor::new(vec![b'x'; 17]);
        let mut reader = BufReader::with_capacity(4, input);
        let error = read_frame_bounded(&mut reader, 16).await.unwrap_err();
        assert!(error.to_string().contains("exceeds 16 bytes"));
    }
}
