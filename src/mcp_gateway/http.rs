use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use std::time::Instant;
use std::{collections::HashSet, collections::VecDeque};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::registry::HttpServer;
use super::stdio::{observe_client_frame, transform_server_frame, RelayState};

const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_SEEN_EVENT_IDS: usize = 1_024;

struct SseEvent {
    id: Option<String>,
    data: Vec<u8>,
}

pub async fn serve(server_id: &str, surface: &str, server: &HttpServer) -> Result<()> {
    let endpoint = reqwest::Url::parse(&server.url)?;
    let addresses = validate_and_resolve_destination(&endpoint).await?;
    let client = pinned_client(&endpoint, &addresses)?;
    let bearer = match server.bearer_token_env.as_deref() {
        Some(name) => Some(
            std::env::var(name)
                .with_context(|| format!("bearer token environment variable {name} is not set"))?,
        ),
        None => Some(super::oauth::access_token(server_id).await.with_context(|| {
            format!("no OAuth credential for {server_id:?}; run `ctx gateway login {server_id}`")
        })?),
    };
    eprintln!(
        "ctx gateway outbound receipt: server={server_id} destination={} approved_at={} auth={}",
        endpoint,
        server.approved_at,
        server.bearer_token_env.as_deref().unwrap_or("none")
    );

    let mut state = RelayState::new(
        server_id,
        surface,
        std::env::current_dir()?.to_string_lossy().into_owned(),
    );
    let mut session_id: Option<String> = None;
    let mut seen_event_ids = HashSet::new();
    let mut event_id_order = VecDeque::new();
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut frame = Vec::new();
    loop {
        frame.clear();
        let read = stdin.read_until(b'\n', &mut frame).await?;
        if read == 0 {
            break;
        }
        if frame.len() > MAX_MESSAGE_BYTES {
            anyhow::bail!("MCP request exceeds {} bytes", MAX_MESSAGE_BYTES);
        }
        observe_client_frame(&mut state, &frame)?;

        let mut request = client
            .post(endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header("MCP-Protocol-Version", state.protocol_version());
        if let Some(token) = bearer.as_deref() {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(session) = session_id.as_deref() {
            request = request.header("Mcp-Session-Id", session);
        }
        let request_started = Instant::now();
        let response = match request.body(trim_newline(&frame).to_vec()).send().await {
            Ok(response) => response,
            Err(error) => {
                crate::db::record_gateway_runtime_event_best_effort(
                    surface,
                    server_id,
                    "failure",
                    Some(request_started.elapsed().as_millis().min(u64::MAX as u128) as u64),
                    Some("remote-request-failed"),
                );
                return Err(error).context("send approved remote MCP request");
            }
        };
        if let Some(received) = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
        {
            if received.len() <= 1_024 && !received.contains(['\r', '\n']) {
                session_id = Some(received.to_owned());
            }
        }
        let status = response.status();
        if status == reqwest::StatusCode::ACCEPTED || status == reqwest::StatusCode::NO_CONTENT {
            continue;
        }
        if !status.is_success() {
            crate::db::record_gateway_runtime_event_best_effort(
                surface,
                server_id,
                "failure",
                Some(request_started.elapsed().as_millis().min(u64::MAX as u128) as u64),
                Some(&format!("remote-http-status-{status}")),
            );
            anyhow::bail!("remote MCP destination returned HTTP {status}");
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let events = if content_type.starts_with("text/event-stream") {
            read_sse_messages(response).await?
        } else {
            vec![SseEvent {
                id: None,
                data: read_json_message(response).await?,
            }]
        };
        for event in events {
            if let Some(id) = event.id.as_ref() {
                if !seen_event_ids.insert(id.clone()) {
                    continue;
                }
                event_id_order.push_back(id.clone());
                if event_id_order.len() > MAX_SEEN_EVENT_IDS {
                    if let Some(expired) = event_id_order.pop_front() {
                        seen_event_ids.remove(&expired);
                    }
                }
            }
            let mut server_frame = event.data;
            if !server_frame.ends_with(b"\n") {
                server_frame.push(b'\n');
            }
            let (outgoing, applied) = transform_server_frame(&mut state, &server_frame);
            stdout.write_all(&outgoing).await?;
            stdout.flush().await?;
            if let Some(prepared) = applied {
                if let Err(error) = crate::tool_result::mark_mcp_trim_emitted(&prepared) {
                    eprintln!(
                        "ctx gateway: result was emitted but applied receipt failed: {error}"
                    );
                }
            }
        }
    }
    Ok(())
}

fn pinned_client(endpoint: &reqwest::Url, addresses: &[SocketAddr]) -> Result<reqwest::Client> {
    let host = endpoint
        .host_str()
        .context("remote MCP endpoint has no host")?;
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .resolve_to_addrs(host, addresses)
        .build()?)
}

pub(super) async fn secure_client_for_url(url: &reqwest::Url) -> Result<reqwest::Client> {
    let addresses = validate_and_resolve_destination(url).await?;
    pinned_client(url, &addresses)
}

async fn validate_and_resolve_destination(endpoint: &reqwest::Url) -> Result<Vec<SocketAddr>> {
    let host = endpoint
        .host_str()
        .context("remote MCP endpoint has no host")?;
    let port = endpoint
        .port_or_known_default()
        .context("remote MCP endpoint has no port")?;
    let addresses: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .context("resolve approved MCP destination")?
        .collect();
    if addresses.is_empty() {
        anyhow::bail!("approved MCP destination resolved to no addresses");
    }
    let loopback_http = endpoint.scheme() == "http";
    for address in &addresses {
        if loopback_http {
            if !address.ip().is_loopback() {
                anyhow::bail!("plain HTTP destination resolved outside loopback");
            }
        } else if !is_public_ip(address.ip()) {
            anyhow::bail!(
                "remote MCP destination resolved to blocked non-public address {}",
                address.ip()
            );
        }
    }
    Ok(addresses)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || octets[0] == 0
                || octets[0] >= 240
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && matches!(octets[1], 18 | 19)))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

async fn read_json_message(response: reqwest::Response) -> Result<Vec<u8>> {
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        anyhow::bail!("remote MCP response exceeds {} bytes", MAX_MESSAGE_BYTES);
    }
    serde_json::from_slice::<serde_json::Value>(&bytes).context("invalid MCP JSON response")?;
    Ok(bytes.to_vec())
}

async fn read_sse_messages(response: reqwest::Response) -> Result<Vec<SseEvent>> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > MAX_MESSAGE_BYTES {
            anyhow::bail!(
                "remote MCP event stream exceeds {} bytes",
                MAX_MESSAGE_BYTES
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_sse_text(std::str::from_utf8(&bytes).context("MCP SSE response is not UTF-8")?)
}

fn parse_sse_text(text: &str) -> Result<Vec<SseEvent>> {
    let mut messages = Vec::new();
    let mut data = Vec::new();
    let mut event_id = None;
    for line in text.lines() {
        if line.is_empty() {
            push_sse_data(&mut messages, &mut data, &mut event_id)?;
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        } else if let Some(value) = line.strip_prefix("id:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if value.len() <= 1_024 && !value.contains('\0') {
                event_id = Some(value.to_owned());
            }
        }
    }
    push_sse_data(&mut messages, &mut data, &mut event_id)?;
    Ok(messages)
}

fn push_sse_data(
    messages: &mut Vec<SseEvent>,
    data: &mut Vec<&str>,
    event_id: &mut Option<String>,
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let joined = data.join("\n");
    serde_json::from_str::<serde_json::Value>(&joined).context("invalid MCP SSE data event")?;
    messages.push(SseEvent {
        id: event_id.take(),
        data: joined.into_bytes(),
    });
    data.clear();
    Ok(())
}

fn trim_newline(frame: &[u8]) -> &[u8] {
    frame
        .strip_suffix(b"\n")
        .and_then(|line| line.strip_suffix(b"\r").or(Some(line)))
        .unwrap_or(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_and_special_addresses() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("169.254.169.254".parse().unwrap()));
        assert!(!is_public_ip("10.1.2.3".parse().unwrap()));
        assert!(!is_public_ip("100.100.100.100".parse().unwrap()));
        assert!(!is_public_ip("2001:db8::1".parse().unwrap()));
        assert!(is_public_ip("1.1.1.1".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn parses_multiline_sse_and_retains_bounded_redelivery_ids() {
        let events = parse_sse_text(
            "id: event-1\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1}\n\nid: event-2\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n\n",
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id.as_deref(), Some("event-1"));
        assert_eq!(events[1].id.as_deref(), Some("event-2"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&events[0].data).unwrap()["id"],
            1
        );
    }
}
