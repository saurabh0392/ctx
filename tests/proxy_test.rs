//! Integration test: ctx proxy + mock Anthropic upstream (no real API calls).

use axum::{
    body::Body,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode as AxumStatus},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use ctx::ca;
use ctx::proxy::{router, run_gates_for_mode, ProxyState};
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Captured(Arc<Mutex<Option<Vec<u8>>>>);

async fn mock_v1_messages(
    State(Captured(buf)): State<Captured>,
    body: Bytes,
) -> Json<Value> {
    *buf.lock().unwrap() = Some(body.to_vec());
    Json(json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "ok"}],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 2}
    }))
}

async fn spawn_mock_upstream() -> (u16, Captured) {
    let buf = Arc::new(Mutex::new(None));
    let cap = Captured(buf.clone());
    let app = Router::new()
        .route("/v1/messages", post(mock_v1_messages))
        .with_state(cap.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (port, cap)
}

fn write_test_config(dir: &std::path::Path, proxy_mode: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        format!(
            r#"active_profile = "carrier"
proxy_mode = "{proxy_mode}"
auto_profile_enabled = false
inject_enabled = false
"#
        ),
    )
    .unwrap();
}

#[tokio::test]
#[serial]
async fn proxy_filters_tools_and_forwards_response() {
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    write_test_config(tmp.path(), "standalone");

    let (mock_port, captured) = spawn_mock_upstream().await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_gzip()
        .build()
        .unwrap();
    let upstream = format!("http://127.0.0.1:{mock_port}");
    let state = Arc::new(ProxyState {
        client: client.clone(),
        upstream,
        tls_acceptor: None,
    });
    let app = router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let request_body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "hi"}],
        "system": "You are a test assistant.",
        "tools": [
            {"name": "mcp__claude_ai_Slack__search", "input_schema": {"type": "object"}},
            {"name": "mcp__claude_ai_Figma__get_file", "input_schema": {"type": "object"}}
        ]
    });

    let url = format!("http://127.0.0.1:{proxy_port}/v1/messages");
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .send()
        .await
        .expect("request to proxy");

    assert!(resp.status().is_success(), "status {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "msg_test");
    assert_eq!(body["usage"]["output_tokens"], 2);

    let upstream_bytes = captured.0.lock().unwrap().clone().expect("upstream saw body");
    let upstream_json: Value = serde_json::from_slice(&upstream_bytes).unwrap();
    let tools = upstream_json["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 1, "Figma tool should be stripped for carrier profile");
    assert_eq!(
        tools[0]["name"].as_str().unwrap(),
        "mcp__claude_ai_Slack__search"
    );

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

async fn spawn_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                loop {
                    let n = match sock.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    if sock.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    port
}

#[tokio::test]
#[serial]
async fn mitm_connect_filters_tools_and_forwards_response() {
    ctx::ensure_tls_crypto_provider();
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    write_test_config(tmp.path(), "standalone");

    let (mock_port, captured) = spawn_mock_upstream().await;
    let proxy_port = pick_free_port();
    let upstream = format!("http://127.0.0.1:{mock_port}");
    let _proxy_task = tokio::spawn(async move {
        let _ = ctx::proxy::start(proxy_port, &upstream).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let pem = std::fs::read(ca::ca_cert_path()).unwrap();
    let cert = reqwest::Certificate::from_pem(&pem).unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_gzip()
        .add_root_certificate(cert)
        .proxy(
            reqwest::Proxy::https(format!("http://127.0.0.1:{proxy_port}"))
                .unwrap(),
        )
        .build()
        .unwrap();

    let request_body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "hi"}],
        "system": "You are a test assistant.",
        "tools": [
            {"name": "mcp__claude_ai_Slack__search", "input_schema": {"type": "object"}},
            {"name": "mcp__claude_ai_Figma__get_file", "input_schema": {"type": "object"}}
        ]
    });

    let url = "https://api.anthropic.com/v1/messages";
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .send()
        .await
        .expect("MITM request through CONNECT");

    assert!(resp.status().is_success(), "status {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "msg_test");

    let upstream_bytes = captured.0.lock().unwrap().clone().expect("upstream saw body");
    let upstream_json: Value = serde_json::from_slice(&upstream_bytes).unwrap();
    let tools = upstream_json["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 1);

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

#[tokio::test]
#[serial]
async fn connect_tunnel_passes_through_non_anthropic() {
    ctx::ensure_tls_crypto_provider();
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    write_test_config(tmp.path(), "standalone");

    let echo_port = spawn_echo_server().await;
    let proxy_port = pick_free_port();
    let upstream = "http://127.0.0.1:9";
    let _proxy_task = tokio::spawn(async move {
        let _ = ctx::proxy::start(proxy_port, upstream).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let mut c = tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .unwrap();
    let req = format!(
        "CONNECT 127.0.0.1:{echo_port} HTTP/1.1\r\nHost: 127.0.0.1:{echo_port}\r\n\r\n"
    );
    c.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = c.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(
        head.contains("200"),
        "expected 200 Connection Established, got {head:?}"
    );
    c.write_all(b"ping").await.unwrap();
    let n2 = c.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n2], b"ping");

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

async fn spawn_sse_mock_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let app = Router::new().route(
            "/v1/messages",
            post(|| async {
                let body_stream = stream::unfold(0u8, |step| async move {
                    match step {
                        0 => {
                            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                            Some((
                                Ok::<_, std::convert::Infallible>(Bytes::from(
                                    "data: {\"chunk\":1}\n\n",
                                )),
                                1,
                            ))
                        }
                        1 => {
                            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                            Some((
                                Ok(Bytes::from("data: {\"chunk\":2}\n\n")),
                                2,
                            ))
                        }
                        _ => None,
                    }
                });
                Response::builder()
                    .status(AxumStatus::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(body_stream))
                    .unwrap()
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
}

#[tokio::test]
#[serial]
async fn mitm_streams_sse_response() {
    ctx::ensure_tls_crypto_provider();
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    write_test_config(tmp.path(), "standalone");

    let mock_port = spawn_sse_mock_upstream().await;
    let proxy_port = pick_free_port();
    let upstream = format!("http://127.0.0.1:{mock_port}");
    let _proxy_task = tokio::spawn(async move {
        let _ = ctx::proxy::start(proxy_port, &upstream).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let pem = std::fs::read(ca::ca_cert_path()).unwrap();
    let cert = reqwest::Certificate::from_pem(&pem).unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_gzip()
        .add_root_certificate(cert)
        .proxy(
            reqwest::Proxy::https(format!("http://127.0.0.1:{proxy_port}"))
                .unwrap(),
        )
        .build()
        .unwrap();

    let request_body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}],
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .send()
        .await
        .expect("MITM streaming request");

    assert!(resp.status().is_success(), "status {}", resp.status());
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("text/event-stream"), "content-type={ct}");

    let mut byte_stream = resp.bytes_stream();
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        byte_stream.next(),
    )
    .await
    .expect("timeout waiting for first SSE chunk")
    .expect("stream ended early")
    .expect("chunk error");
    assert!(
        first.starts_with(b"data:"),
        "first chunk should be SSE: {:?}",
        String::from_utf8_lossy(&first)
    );

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

async fn spawn_rate_limit_mock() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let app = Router::new().route(
            "/v1/messages",
            post(|| async {
                let mut headers = HeaderMap::new();
                headers.insert("retry-after", "30".parse().unwrap());
                (AxumStatus::TOO_MANY_REQUESTS, headers, "rate limited").into_response()
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
}

#[tokio::test]
#[serial]
async fn mitm_passthrough_429() {
    ctx::ensure_tls_crypto_provider();
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    write_test_config(tmp.path(), "standalone");

    let mock_port = spawn_rate_limit_mock().await;
    let proxy_port = pick_free_port();
    let upstream = format!("http://127.0.0.1:{mock_port}");
    let _proxy_task = tokio::spawn(async move {
        let _ = ctx::proxy::start(proxy_port, &upstream).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let pem = std::fs::read(ca::ca_cert_path()).unwrap();
    let cert = reqwest::Certificate::from_pem(&pem).unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_gzip()
        .add_root_certificate(cert)
        .proxy(
            reqwest::Proxy::https(format!("http://127.0.0.1:{proxy_port}"))
                .unwrap(),
        )
        .build()
        .unwrap();

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .send()
        .await
        .expect("MITM 429 request");

    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        resp.headers().get("retry-after").and_then(|v| v.to_str().ok()),
        Some("30")
    );
    assert_eq!(resp.text().await.unwrap(), "rate limited");

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

#[test]
#[serial]
fn complement_mode_filters_without_inject() {
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var("CTX_HOME").ok();
    std::env::set_var("CTX_HOME", tmp.path());
    std::fs::write(
        tmp.path().join("config.toml"),
        r#"active_profile = "carrier"
proxy_mode = "complement"
auto_profile_enabled = false
inject_enabled = true
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("system_prefix.md"),
        "CTX_TEST_INJECT_PREFIX",
    )
    .unwrap();

    let body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "hi"}],
        "system": "You are a test assistant.",
        "tools": [
            {"name": "mcp__claude_ai_Slack__search", "input_schema": {"type": "object"}},
            {"name": "mcp__claude_ai_Figma__get_file", "input_schema": {"type": "object"}}
        ]
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    let out = run_gates_for_mode(&bytes);
    let parsed: Value = serde_json::from_slice(&out).unwrap();
    let system = parsed["system"].as_str().unwrap_or("");
    assert!(
        !system.contains("CTX_TEST_INJECT_PREFIX"),
        "complement mode must not inject in proxy"
    );
    let tools = parsed["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "complement mode still filters tools");

    if let Some(p) = prev {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}

#[test]
#[serial]
fn install_sets_https_proxy_and_ca() {
    let tmp = tempfile::tempdir().unwrap();
    let prev_home = std::env::var("HOME").ok();
    let prev_ctx = std::env::var("CTX_HOME").ok();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("CTX_HOME", tmp.path().join(".ctx"));
    std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

    ctx::ensure_tls_crypto_provider();
    ctx::proxy::install(8799, "https://api.anthropic.com", ctx::config::ProxyMode::Complement)
        .expect("proxy install");

    let settings_path = tmp.path().join(".claude").join("settings.json");
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(
        settings["env"]["CLAUDE_CODE_HTTPS_PROXY"].as_str().unwrap(),
        "http://127.0.0.1:8799"
    );
    assert_eq!(
        settings["env"]["HTTPS_PROXY"].as_str().unwrap(),
        "http://127.0.0.1:8799"
    );
    let ca = settings["env"]["NODE_EXTRA_CA_CERTS"].as_str().unwrap();
    assert!(ca.ends_with("ca-cert.pem"), "ca path={ca}");

    let cfg = ctx::config::Config::load();
    assert_eq!(cfg.proxy_mode, ctx::config::ProxyMode::Complement);

    if let Some(p) = prev_home {
        std::env::set_var("HOME", p);
    } else {
        std::env::remove_var("HOME");
    }
    if let Some(p) = prev_ctx {
        std::env::set_var("CTX_HOME", p);
    } else {
        std::env::remove_var("CTX_HOME");
    }
}
