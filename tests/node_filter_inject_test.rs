//! Verifies NODE_OPTIONS `--require` filter strips MCP tools (no real Anthropic calls).

use ctx::filter_hook;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn node_require_filter_strips_mcp_tools() {
    let _g = ctx::test_lock::CTX_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let probe = Command::new("node")
        .arg("-e")
        .arg("process.exit(0)")
        .output();
    let Ok(out) = probe else {
        eprintln!("skip node_filter_inject: no node binary on PATH");
        return;
    };
    if !out.status.success() {
        eprintln!(
            "skip node_filter_inject: node not runnable: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let prev_home = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());

    std::fs::write(
        tmp.path().join("filter-config.json"),
        br#"{"profile":"t","keep":["mcp__claude_ai_Slack__"]}"#,
    )
    .unwrap();
    filter_hook::write_filter_js().unwrap();
    let abs = filter_hook::filter_js_path().canonicalize().unwrap();

    let (tx_port, rx_port) = mpsc::channel::<u16>();
    let (tx_body, rx_body) = mpsc::channel::<Vec<u8>>();

    let server = thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        tx_port.send(listener.local_addr().unwrap().port()).unwrap();
        let mut stream = listener.accept().unwrap().0;
        let mut buf = Vec::new();
        let mut scratch = [0u8; 2048];
        loop {
            let n = stream.read(&mut scratch).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&scratch[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&buf[..pos]).unwrap();
                let mut cl: Option<usize> = None;
                for line in headers.lines() {
                    let lower = line.to_ascii_lowercase();
                    if let Some(rest) = lower.strip_prefix("content-length:") {
                        cl = rest.trim().parse().ok();
                    }
                }
                let start = pos + 4;
                if let Some(len) = cl {
                    if buf.len() >= start + len {
                        tx_body.send(buf[start..start + len].to_vec()).unwrap();
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        );
                        break;
                    }
                }
            }
        }
    });

    let port = rx_port.recv_timeout(Duration::from_secs(2)).unwrap();

    let script = format!(
        r#"
const http = require('http');
const body = JSON.stringify({{
  model: 'x',
  messages: [],
  tools: [
    {{ name: 'mcp__claude_ai_Slack__search', input_schema: {{}} }},
    {{ name: 'mcp__claude_ai_Figma__x', input_schema: {{}} }},
  ],
}});
const req = http.request(
  {{
    hostname: '127.0.0.1',
    port: {port},
    path: '/v1/messages',
    method: 'POST',
    headers: {{
      'Content-Type': 'application/json',
      'Content-Length': Buffer.byteLength(body),
    }},
  }},
  (res) => {{
    res.resume();
    res.on('end', () => process.exit(0));
  }},
);
req.on('error', (e) => {{ console.error(e); process.exit(1); }});
req.end(body);
"#
    );

    let out = Command::new("node")
        .env("CTX_HOME", tmp.path())
        .env("CTX_FILTER_HOST", "127.0.0.1")
        .env("CTX_FILTER_PORT", port.to_string())
        .env(
            "NODE_OPTIONS",
            format!("--require {}", abs.display()),
        )
        .arg("-e")
        .arg(&script)
        .output()
        .expect("spawn node");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let body = rx_body.recv_timeout(Duration::from_secs(3)).unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let tools = v["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0]["name"].as_str().unwrap(),
        "mcp__claude_ai_Slack__search"
    );

    server.join().ok();

    match prev_home {
        Some(h) => std::env::set_var("CTX_HOME", h),
        None => std::env::remove_var("CTX_HOME"),
    }
}
