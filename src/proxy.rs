use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use colored::Colorize;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Request as HyperRequest, Response as HyperResponse, StatusCode as HyperStatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

use crate::ca::{self, CertAuthority};
use crate::{
    analytics,
    config::{Config, ProxyMode},
};

pub struct ProxyState {
    pub client: reqwest::Client,
    pub upstream: String,
    /// TLS terminator for CONNECT MITM to Anthropic (None in unit tests that only use reverse HTTP).
    pub tls_acceptor: Option<TlsAcceptor>,
}

/// Axum router for the proxy (legacy reverse HTTP; used by integration tests).
pub fn router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/", any(proxy_handler))
        .route("/*path", any(proxy_handler))
        .with_state(state)
}

pub async fn start(port: u16, upstream: &str) -> Result<()> {
    crate::ensure_tls_crypto_provider();
    ca::ensure_ca()?;
    let authority = CertAuthority::load_or_generate()?;
    let tls_acceptor = TlsAcceptor::from(authority.server_config_for_anthropic()?);

    let config = Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");

    println!("{} ctx proxy on :{port}", "->".cyan().bold());
    println!("  Mode:     {}", config.proxy_mode.as_str());
    println!("  Upstream: {upstream}");
    println!(
        "  MITM:     CONNECT {}:443 (HTTPS_PROXY mode)",
        ca::ANTHROPIC_API_HOST
    );
    println!("  Profile:  {active}");
    println!(
        "  Auto-profile: {}",
        if config.auto_profile_enabled {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  Inject:   {}",
        if config.inject_enabled { "on" } else { "off" }
    );
    println!("{} Ctrl+C to stop\n", "i".dimmed());

    let state = Arc::new(ProxyState {
        client: reqwest::Client::builder()
            .use_rustls_tls()
            .no_gzip()
            .no_proxy()
            .default_headers(reqwest::header::HeaderMap::new())
            .build()?,
        upstream: upstream.to_string(),
        tls_acceptor: Some(tls_acceptor),
    });

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}. Is the port already in use?"))?;

    println!("Listening on http://{addr} (CONNECT + legacy HTTP)");
    loop {
        let (stream, _) = listener.accept().await?;
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = service_fn(move |req: HyperRequest<Incoming>| {
                let st = Arc::clone(&st);
                async move { outer_serve(req, st).await }
            });
            let err = hyper::server::conn::http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(io, svc)
                .with_upgrades()
                .await;
            if let Err(e) = err {
                eprintln!("[ctx] connection error: {e}");
            }
        });
    }
}

async fn outer_serve(
    req: HyperRequest<Incoming>,
    state: Arc<ProxyState>,
) -> Result<HyperResponse<Body>, Infallible> {
    if req.method() == hyper::Method::CONNECT {
        let Some(auth) = req.uri().authority().map(|a| a.as_str().to_string()) else {
            return Ok(bad_request("CONNECT missing authority"));
        };
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_connect_upgraded(upgraded, auth, state).await {
                        eprintln!("[ctx] CONNECT pipeline error: {e}");
                    }
                }
                Err(e) => eprintln!("[ctx] CONNECT upgrade error: {e}"),
            }
        });
        return Ok(HyperResponse::new(Body::empty()));
    }

    match legacy_hyper_forward(req, state).await {
        Ok(r) => Ok(r),
        Err(e) => {
            eprintln!("[ctx] legacy forward error: {e}");
            Ok(HyperResponse::builder()
                .status(HyperStatusCode::BAD_GATEWAY)
                .body(Body::from("upstream error"))
                .unwrap())
        }
    }
}

fn bad_request(msg: &'static str) -> HyperResponse<Body> {
    HyperResponse::builder()
        .status(HyperStatusCode::BAD_REQUEST)
        .body(Body::from(msg))
        .unwrap()
}

fn connect_target_is_anthropic_api(auth: &str) -> bool {
    match parse_connect_host(auth) {
        Some((host, port)) => host == ca::ANTHROPIC_API_HOST && port == 443,
        None => false,
    }
}

fn parse_connect_host(auth: &str) -> Option<(String, u16)> {
    if let Some(rest) = auth.strip_prefix('[') {
        let (ip, tail) = rest.split_once(']')?;
        let port = tail.strip_prefix(':')?.parse().ok()?;
        return Some((format!("[{ip}]"), port));
    }
    let (h, p) = auth.rsplit_once(':')?;
    let port: u16 = p.parse().ok()?;
    Some((h.to_string(), port))
}

async fn handle_connect_upgraded(
    upgraded: Upgraded,
    authority: String,
    state: Arc<ProxyState>,
) -> anyhow::Result<()> {
    let mut client = TokioIo::new(upgraded);
    if connect_target_is_anthropic_api(&authority) {
        let Some(ref acceptor) = state.tls_acceptor else {
            anyhow::bail!("MITM requested but tls_acceptor is not configured");
        };
        let tls = acceptor.accept(client).await?;
        let st = Arc::clone(&state);
        let svc = service_fn(move |req: HyperRequest<Incoming>| {
            let st = Arc::clone(&st);
            async move { mitm_http_forward(req, st).await }
        });
        if let Err(e) = hyper::server::conn::http1::Builder::new()
            .preserve_header_case(true)
            .serve_connection(TokioIo::new(tls), svc)
            .await
        {
            eprintln!("[ctx] mitm inner HTTP error: {e}");
        }
        return Ok(());
    }

    let mut remote = TcpStream::connect(authority.as_str()).await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut remote).await?;
    Ok(())
}

async fn mitm_http_forward(
    req: HyperRequest<Incoming>,
    state: Arc<ProxyState>,
) -> Result<HyperResponse<Body>, Infallible> {
    match mitm_http_forward_inner(req, state).await {
        Ok(r) => Ok(r),
        Err(e) => {
            eprintln!("[ctx] mitm forward error: {e}");
            Ok(HyperResponse::builder()
                .status(HyperStatusCode::BAD_GATEWAY)
                .body(Body::from("bad gateway"))
                .unwrap())
        }
    }
}

async fn mitm_http_forward_inner(
    req: HyperRequest<Incoming>,
    state: Arc<ProxyState>,
) -> Result<HyperResponse<Body>, anyhow::Error> {
    let (parts, body) = req.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    let is_messages = parts.method == Method::POST && parts.uri.path() == "/v1/messages";
    let has_encoding = parts.headers.contains_key(http::header::CONTENT_ENCODING);

    let filtered_bytes = if is_messages && !has_encoding {
        run_gates_for_mode(&body_bytes)
    } else {
        body_bytes.to_vec()
    };

    let path = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let upstream_url = format!("{}{}", state.upstream.trim_end_matches('/'), path);
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut req_builder = state.client.request(method, &upstream_url);
    for (name, value) in &parts.headers {
        let n = name.as_str().to_lowercase();
        if n == "host" || n == "content-length" || n == "transfer-encoding" || n == "connection" {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }
    req_builder = req_builder
        .header("content-length", filtered_bytes.len().to_string())
        .body(filtered_bytes.clone());

    let upstream_resp = req_builder.send().await?;
    build_hyper_response(upstream_resp, &filtered_bytes, path).await
}

async fn legacy_hyper_forward(
    req: HyperRequest<Incoming>,
    state: Arc<ProxyState>,
) -> Result<HyperResponse<Body>, anyhow::Error> {
    let (parts, body) = req.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    let is_messages = parts.method == Method::POST && parts.uri.path() == "/v1/messages";
    let has_encoding = parts.headers.contains_key(http::header::CONTENT_ENCODING);

    let filtered_bytes = if is_messages && !has_encoding {
        run_gates_for_mode(&body_bytes)
    } else {
        body_bytes.to_vec()
    };

    let uri = &parts.uri;
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let upstream_url = if uri.scheme().is_some() {
        uri.to_string()
    } else {
        format!("{}{}", state.upstream.trim_end_matches('/'), path)
    };

    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut req_builder = state.client.request(method, &upstream_url);
    for (name, value) in &parts.headers {
        let n = name.as_str().to_lowercase();
        if n == "host" || n == "content-length" || n == "transfer-encoding" || n == "connection" {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }
    req_builder = req_builder
        .header("content-length", filtered_bytes.len().to_string())
        .body(filtered_bytes.clone());

    let upstream_resp = req_builder.send().await?;
    build_hyper_response(upstream_resp, &filtered_bytes, path).await
}

pub async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    req: Request,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();

    let body_bytes = axum::body::to_bytes(body, 64 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let is_messages = parts.method == Method::POST && parts.uri.path() == "/v1/messages";
    let has_encoding = parts.headers.contains_key("content-encoding");

    let filtered_bytes = if is_messages && !has_encoding {
        run_gates_for_mode(&body_bytes)
    } else {
        body_bytes.to_vec()
    };

    let upstream_url = format!("{}{}", state.upstream, parts.uri);
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut req_builder = state.client.request(method, &upstream_url);

    for (name, value) in &parts.headers {
        let n = name.as_str().to_lowercase();
        if n == "host" || n == "content-length" || n == "transfer-encoding" || n == "connection" {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }

    req_builder = req_builder
        .header("content-length", filtered_bytes.len().to_string())
        .body(filtered_bytes.clone());

    let upstream_resp = req_builder.send().await.map_err(|e| {
        eprintln!("[ctx] upstream error: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    let status_code = upstream_resp.status().as_u16();
    let streaming =
        is_streaming_request(&filtered_bytes) || response_is_event_stream(upstream_resp.headers());
    log_proxy_response(
        parts.uri.path(),
        streaming,
        status_code,
        upstream_resp.headers().get("request-id"),
    );

    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response = Response::builder().status(status);
    for (k, v) in upstream_resp.headers() {
        let n = k.as_str().to_lowercase();
        if n == "transfer-encoding" || n == "connection" || n == "keep-alive" {
            continue;
        }
        response = response.header(k, v);
    }

    let body = if streaming {
        Body::from_stream(upstream_resp.bytes_stream())
    } else {
        Body::from(
            upstream_resp
                .bytes()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?,
        )
    };
    Ok(response.body(body).unwrap())
}

fn is_streaming_request(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    value.get("stream").and_then(|v| v.as_bool()) == Some(true)
}

fn response_is_event_stream(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("text/event-stream"))
        .unwrap_or(false)
}

fn log_proxy_response(
    path: &str,
    stream: bool,
    status: u16,
    request_id: Option<&reqwest::header::HeaderValue>,
) {
    let rid = request_id.and_then(|v| v.to_str().ok()).unwrap_or("-");
    let line = format!("proxy path={path} stream={stream} status={status} request-id={rid}\n");
    eprintln!("[ctx] {line}");
    let log_path = crate::config::ctx_dir().join("proxy.stderr.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

async fn build_hyper_response(
    upstream_resp: reqwest::Response,
    request_body: &[u8],
    path: &str,
) -> Result<HyperResponse<Body>, anyhow::Error> {
    let status_code = upstream_resp.status().as_u16();
    let status =
        HyperStatusCode::from_u16(status_code).unwrap_or(HyperStatusCode::INTERNAL_SERVER_ERROR);
    let stream_req = is_streaming_request(request_body);
    let stream_resp = response_is_event_stream(upstream_resp.headers());
    let streaming = stream_req || stream_resp;

    log_proxy_response(
        path,
        streaming,
        status_code,
        upstream_resp.headers().get("request-id"),
    );

    let mut response = HyperResponse::builder().status(status);
    for (k, v) in upstream_resp.headers() {
        let n = k.as_str().to_lowercase();
        if n == "transfer-encoding" || n == "connection" || n == "keep-alive" {
            continue;
        }
        response = response.header(k, v);
    }

    if streaming {
        Ok(response
            .body(Body::from_stream(upstream_resp.bytes_stream()))
            .unwrap())
    } else {
        let full_body = upstream_resp.bytes().await?;
        Ok(response.body(Body::from(full_body)).unwrap())
    }
}

/// Mode-aware gate pipeline for MITM / reverse proxy paths.
pub fn run_gates_for_mode(body: &[u8]) -> Vec<u8> {
    let original = body.to_vec();
    let mode = Config::load().proxy_mode;
    let result = std::panic::catch_unwind(|| match mode {
        ProxyMode::Off => Ok(original.clone()),
        ProxyMode::Complement | ProxyMode::FilterOnly => run_gates_filter_only(body),
        ProxyMode::Standalone => run_gates_inner(body),
    });
    match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            eprintln!("[ctx] gate pipeline error (fail-open): {e}");
            original
        }
        Err(_) => {
            eprintln!("[ctx] gate pipeline panic (fail-open): forwarding original body");
            original
        }
    }
}

fn run_gates_filter_only(body: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let config = Config::load();
    let active_slug = config
        .active_profile
        .as_deref()
        .unwrap_or("all")
        .to_string();
    let working_directory = working_dir_from_body(body);

    let bytes_before = body.len();
    let filter_result = crate::filter::filter_request(body);
    let removed = filter_result.tools_removed;
    let removed_servers = filter_result.removed_servers;
    let kept_servers = filter_result.kept_servers;
    let bytes = filter_result.body;

    let tokens_saved = if removed > 0 {
        let saved = bytes_before.saturating_sub(bytes.len()) / 4;
        eprintln!(
            "[ctx] -{removed} tools (~{saved} tokens)  profile={active_slug} (proxy filter-only)"
        );
        saved
    } else {
        0
    };

    if removed > 0 {
        analytics::record(
            removed,
            tokens_saved,
            &active_slug,
            analytics::TraceInfo {
                removed_servers,
                kept_servers,
                auto_selected: false,
                auto_trigger: None,
                inject_fired: false,
                inject_chars: 0,
                adaptive_chars: 0,
                budget_blocked: false,
                coach_kind: None,
                budget_fired: false,
                behavior_kind: None,
                working_directory,
            },
        );
    }

    Ok(bytes)
}

/// Run all gate pipeline stages against `body`. Returns the (possibly modified) body bytes.
pub fn run_gates(body: &[u8]) -> Vec<u8> {
    let original = body.to_vec();
    match std::panic::catch_unwind(|| run_gates_inner(body)) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            eprintln!("[ctx] gate pipeline error (fail-open): {e}");
            original
        }
        Err(_) => {
            eprintln!("[ctx] gate pipeline panic (fail-open): forwarding original body");
            original
        }
    }
}

fn run_gates_inner(body: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let config = Config::load();
    let active_slug = config
        .active_profile
        .as_deref()
        .unwrap_or("all")
        .to_string();

    let working_directory = working_dir_from_body(body);

    let (effective_slug, auto_selected, auto_trigger) = if config.auto_profile_enabled {
        match auto_profile_info(body, &active_slug) {
            Some((slug, trigger)) => (slug, true, Some(trigger)),
            None => (active_slug.clone(), false, None),
        }
    } else {
        (active_slug.clone(), false, None)
    };

    let bytes_before = body.len();
    let filter_result = if effective_slug != active_slug {
        match crate::profiles::get(&effective_slug) {
            Ok(p) => crate::filter::filter_with_trace(body, &p),
            Err(_) => crate::filter::filter_request(body),
        }
    } else {
        crate::filter::filter_request(body)
    };

    let removed = filter_result.tools_removed;
    // Capture server lists before moving filter_result.body into bytes.
    let removed_servers = filter_result.removed_servers;
    let kept_servers = filter_result.kept_servers;
    let mut bytes = filter_result.body;

    // tokens_saved is measured before gates add content to the body.
    let tokens_saved = if removed > 0 {
        let saved = bytes_before.saturating_sub(bytes.len()) / 4;
        eprintln!("[ctx] -{removed} tools (~{saved} tokens)  profile={effective_slug}");
        saved
    } else {
        0
    };

    let mut inject_fired = false;
    let mut inject_chars = 0usize;
    let mut coach_kind: Option<String> = None;
    let mut budget_fired = false;
    let mut behavior_kind: Option<String> = None;

    if config.inject_enabled {
        if let Some(prefix) = crate::inject::load_prefix() {
            let trimmed = prefix.trim();
            inject_chars = trimmed.chars().count();
            bytes = crate::inject::inject_system(&bytes, trimmed);
            inject_fired = true;
        }
    }

    if let Some(signal) = crate::coach::detect_from_body(&bytes) {
        let kind = match signal.kind {
            crate::coach::SignalKind::CorrectionCascade => "correction-cascade",
            crate::coach::SignalKind::ReAsk => "re-ask",
        };
        coach_kind = Some(kind.to_string());
        bytes = crate::inject::inject_system(&bytes, &signal.suggestion);
    }

    if let Some(hint) = crate::behavior_guard::check(&bytes) {
        behavior_kind = Some("historical-pattern".to_string());
        bytes = crate::inject::inject_system(&bytes, &hint);
    }

    if let Some(warning) = crate::budget_guard::check(&bytes) {
        budget_fired = true;
        bytes = crate::inject::inject_system(&bytes, &warning);
    }

    // Write exactly one analytics record per request, combining filter and gate info.
    // Previously this was split into two partial records (filter info written before gates
    // ran, gate info written after), which caused double-counting in session aggregation.
    let anything_happened = removed > 0
        || inject_fired
        || coach_kind.is_some()
        || budget_fired
        || behavior_kind.is_some()
        || auto_selected;

    if anything_happened {
        analytics::record(
            removed,
            tokens_saved,
            &effective_slug,
            analytics::TraceInfo {
                removed_servers,
                kept_servers,
                auto_selected,
                auto_trigger,
                inject_fired,
                inject_chars,
                adaptive_chars: 0,
                budget_blocked: false,
                coach_kind,
                budget_fired,
                behavior_kind,
                working_directory,
            },
        );
    }

    Ok(bytes)
}

fn working_dir_from_body(body: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return String::new();
    };
    let system_text = match value.get("system") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return String::new(),
    };
    crate::profiles::extract_working_directory_from_system(&system_text).unwrap_or_default()
}

fn first_user_text_from_body(value: &serde_json::Value) -> String {
    let Some(messages) = value.get("messages").and_then(|m| m.as_array()) else {
        return String::new();
    };
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
            return text.to_string();
        }
        if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
            let text: String = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn auto_profile_info(body: &[u8], active_slug: &str) -> Option<(String, String)> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;

    let system_text = match value.get("system") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };

    let cwd =
        crate::profiles::extract_working_directory_from_system(&system_text).unwrap_or_default();
    let prompt = first_user_text_from_body(&value);

    if let Some((slug, trigger)) = crate::profiles::auto_select(&cwd, &prompt, active_slug) {
        eprintln!("[ctx] auto-profile: {slug} (matched \"{trigger}\")");
        return Some((slug, trigger));
    }

    None
}

fn is_proxy_listening(port: u16) -> bool {
    std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok()
}

fn proxy_url_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn env_object(settings: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    settings
        .pointer("/env")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}

fn set_env_object(
    settings: &mut serde_json::Value,
    env: serde_json::Map<String, serde_json::Value>,
) {
    settings["env"] = serde_json::Value::Object(env);
}

/// True if the value is exactly our reverse-proxy base URL for `port`.
fn is_ctx_reverse_base_url(url: &str, port: u16) -> bool {
    url == proxy_url_for_port(port)
}

/// True if HTTPS_PROXY / HTTP_PROXY points at our listener.
fn is_ctx_forward_proxy_url(url: &str, port: u16) -> bool {
    url == proxy_url_for_port(port)
}

pub fn install(port: u16, upstream: &str, mode: ProxyMode) -> Result<()> {
    if mode == ProxyMode::Off {
        anyhow::bail!("Choose a MITM mode: --mode complement, standalone, or filter-only");
    }

    ca::ensure_ca()?;

    crate::filter_hook::write_filter_js()?;
    crate::filter_hook::sync_filter_config_from_active_config()?;

    let settings_path = crate::config::claude_settings_path();
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .context("Failed to read ~/.claude/settings.json")?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        if let Some(parent) = settings_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        serde_json::json!({})
    };

    let mut env = env_object(&settings);
    let proxy_http = proxy_url_for_port(port);
    let ca_path = ca::canonical_ca_cert_path_string()?;

    let prev_anthropic = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut original_base = prev_anthropic
        .clone()
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    if let Some(ref u) = prev_anthropic {
        if is_ctx_reverse_base_url(u, port) {
            let cfg = Config::load();
            if let Some(saved) = cfg.original_base_url.clone() {
                original_base = saved;
            }
        }
    }

    // Deprecate reverse-proxy mode (breaks MCP Tool Search).
    if let Some(ref u) = prev_anthropic {
        if is_ctx_reverse_base_url(u, port) {
            env.remove("ANTHROPIC_BASE_URL");
        }
    }

    env.insert(
        "CLAUDE_CODE_HTTPS_PROXY".to_string(),
        serde_json::Value::String(proxy_http.clone()),
    );
    env.insert(
        "HTTPS_PROXY".to_string(),
        serde_json::Value::String(proxy_http.clone()),
    );
    env.insert(
        "NODE_EXTRA_CA_CERTS".to_string(),
        serde_json::Value::String(ca_path.clone()),
    );

    if let Some(curr) = env.get("NODE_OPTIONS").and_then(|v| v.as_str()) {
        match crate::filter_hook::strip_ctx_require_from_node_options(Some(curr)) {
            Some(st) if !st.trim().is_empty() => {
                env.insert("NODE_OPTIONS".to_string(), serde_json::Value::String(st));
            }
            _ => {
                env.remove("NODE_OPTIONS");
            }
        }
    }

    set_env_object(&mut settings, env);

    let cfg_snapshot = Config::load();
    let active = cfg_snapshot.active_profile.as_deref().unwrap_or("all");
    let dash = cfg_snapshot.dashboard_port.unwrap_or(8789);

    match mode {
        ProxyMode::Complement | ProxyMode::FilterOnly => {
            crate::claude_settings::apply_native_ctx_to_settings_doc(&mut settings, active, dash)?;
        }
        ProxyMode::Standalone => {
            crate::claude_settings::strip_ctx_native_hooks_from_settings(&mut settings);
            crate::claude_settings::strip_ctx_deny_rules(&mut settings);
            crate::claude_settings::strip_allowed_mcp_servers(&mut settings);
            eprintln!(
                "{} standalone mode: removed ctx hooks and deny rules from settings.json",
                "i".yellow()
            );
        }
        ProxyMode::Off => unreachable!(),
    }

    crate::config::write_json_atomic(&settings_path, &settings)?;

    let mut config = Config::load();
    config.proxy_port = Some(port);
    config.proxy_upstream = Some(upstream.to_string());
    config.original_base_url = Some(original_base);
    config.proxy_mode = mode;
    config.proxy_install_mode = None;
    config.save()?;

    if let Err(e) = crate::daemon::install_proxy(port, upstream) {
        eprintln!("{} launchd/systemd proxy install: {e}", "!".yellow());
        eprintln!("  Start manually: ctx proxy start --port {port}");
    } else if let Err(e) = crate::daemon::bootstrap_proxy(port, upstream) {
        eprintln!("{} proxy service bootstrap: {e}", "!".yellow());
        eprintln!("  Start manually: ctx proxy start --port {port}");
    }

    if !is_proxy_listening(port) {
        eprintln!(
            "{} ctx proxy is not listening on :{port} yet — start it or wait for launchd",
            "!".yellow()
        );
    }

    let host = crate::host::detect_primary_host();
    println!(
        "{} MITM proxy installed ({})",
        "✓".green().bold(),
        mode.as_str()
    );
    println!("  CLAUDE_CODE_HTTPS_PROXY={proxy_http}");
    println!("  HTTPS_PROXY={proxy_http}");
    println!("  NODE_EXTRA_CA_CERTS={ca_path}");
    println!("  Upstream: {upstream}");
    if matches!(mode, ProxyMode::Complement | ProxyMode::FilterOnly) {
        println!(
            "  Hooks + {} filter remain active (proxy strips tools in HTTP body only)",
            cfg_snapshot.filter_mode.as_str()
        );
    }
    println!("\nNext steps:");
    println!("  1. {}", host.reload_instruction());
    println!("  2. Run {} to verify wiring", "`ctx proxy status`".bold());
    println!("  3. Test streaming: curl -x {proxy_http} https://api.anthropic.com/v1/messages ...");

    Ok(())
}

pub fn uninstall() -> Result<()> {
    let config = Config::load();
    let port = config.proxy_port.unwrap_or(8788);
    let original = config
        .original_base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com");

    let settings_path = crate::config::claude_settings_path();
    if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        let mut settings: serde_json::Value = serde_json::from_str(&content)?;
        let mut env = env_object(&settings);

        let still_points_at_ctx_reverse = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(|u| is_ctx_reverse_base_url(u, port))
            .unwrap_or(false);

        let our_ca = ca::canonical_ca_cert_path_string().ok();

        for key in [
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "CLAUDE_CODE_HTTPS_PROXY",
            "CLAUDE_CODE_HTTP_PROXY",
        ] {
            if let Some(v) = env.get(key).and_then(|x| x.as_str()) {
                if is_ctx_forward_proxy_url(v, port) {
                    env.remove(key);
                }
            }
        }

        if let Some(ref ca_path) = our_ca {
            if env.get("NODE_EXTRA_CA_CERTS").and_then(|v| v.as_str()) == Some(ca_path.as_str()) {
                env.remove("NODE_EXTRA_CA_CERTS");
            }
        }

        if config.proxy_mode == ProxyMode::Standalone {
            let leg = crate::config::strip_ctx_managed_hooks_from_settings(&mut settings);
            let nat = crate::claude_settings::strip_ctx_native_hooks_from_settings(&mut settings);
            let mcp = crate::claude_settings::strip_allowed_mcp_servers(&mut settings);
            if leg || nat || mcp {
                println!("{} Removed ctx hooks from settings.json", "✓".green());
            }
        }

        if still_points_at_ctx_reverse {
            env.insert(
                "ANTHROPIC_BASE_URL".to_string(),
                serde_json::Value::String(original.to_string()),
            );
        }

        set_env_object(&mut settings, env);
        crate::config::write_json_atomic(&settings_path, &settings)?;
    }

    let _ = crate::daemon::stop_proxy_service();

    let mut cfg = Config::load();
    cfg.proxy_mode = ProxyMode::Off;
    cfg.proxy_install_mode = None;
    cfg.save()?;

    println!(
        "{} Uninstalled ctx MITM proxy env from settings.json",
        "✓".green()
    );
    println!(
        "{}",
        crate::host::detect_primary_host().reload_instruction()
    );
    Ok(())
}

pub fn status() -> Result<()> {
    let config = Config::load();
    let port = config.proxy_port.unwrap_or(8788);
    let upstream = config
        .proxy_upstream
        .as_deref()
        .unwrap_or("https://api.anthropic.com");
    let profile = config.active_profile.as_deref().unwrap_or("all");
    let listening = is_proxy_listening(port);

    println!("Mode:     {}", config.proxy_mode.as_str());
    println!(
        "Proxy:    http://127.0.0.1:{port} -> {upstream} ({})",
        if listening {
            "listening".green().to_string()
        } else {
            "not listening".red().to_string()
        }
    );
    println!("Profile:  {profile}");
    println!(
        "Auto-profile: {}",
        if config.auto_profile_enabled {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "Inject:   {}",
        if config.inject_enabled { "on" } else { "off" }
    );

    let ca_path =
        ca::canonical_ca_cert_path_string().unwrap_or_else(|_| "(CA not generated)".to_string());
    println!("CA cert:  {ca_path}");

    let log_path = crate::config::ctx_dir().join("proxy.stderr.log");
    if log_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&log_path) {
            let tail: String = content
                .lines()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            if !tail.is_empty() {
                println!("Recent log (~/.ctx/proxy.stderr.log):");
                for line in tail.lines() {
                    println!("  {line}");
                }
            }
        }
    }

    let settings_path = crate::config::claude_settings_path();
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
            let claude_proxy = settings
                .pointer("/env/CLAUDE_CODE_HTTPS_PROXY")
                .and_then(|v| v.as_str())
                .unwrap_or("not set");
            let https_proxy = settings
                .pointer("/env/HTTPS_PROXY")
                .and_then(|v| v.as_str())
                .unwrap_or("not set");
            let anthropic = settings
                .pointer("/env/ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str())
                .unwrap_or("not set");
            let node_extra = settings
                .pointer("/env/NODE_EXTRA_CA_CERTS")
                .and_then(|v| v.as_str())
                .unwrap_or("not set");
            let mitm_on = is_ctx_forward_proxy_url(claude_proxy, port)
                || is_ctx_forward_proxy_url(https_proxy, port);
            println!(
                "MITM env: {} (CLAUDE_CODE_HTTPS_PROXY={})",
                if mitm_on {
                    "wired".green().to_string()
                } else {
                    "not wired".red().to_string()
                },
                claude_proxy
            );
            println!("  HTTPS_PROXY={https_proxy}");
            println!("  ANTHROPIC_BASE_URL={anthropic}");
            println!("  NODE_EXTRA_CA_CERTS={node_extra}");
            let mcp_allow = match settings.get("allowedMcpServers") {
                Some(serde_json::Value::Array(a)) if a.is_empty() => {
                    "allowedMcpServers=[] (explicit empty)".to_string()
                }
                Some(serde_json::Value::Array(a)) => {
                    format!("allowedMcpServers={} entries", a.len())
                }
                None => "allowedMcpServers unset (all MCP servers)".to_string(),
                _ => "allowedMcpServers present (unexpected shape)".to_string(),
            };
            println!("  {mcp_allow}");
            let ups_ctx = settings
                .pointer("/hooks/UserPromptSubmit")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|e| crate::claude_settings::entry_is_ctx_user_prompt_hook(e))
                        .count()
                })
                .unwrap_or(0);
            let async_ctx = ["PostToolUse", "SessionStart", "SessionEnd", "Stop"]
                .iter()
                .filter(|k| {
                    settings
                        .pointer(&format!("/hooks/{k}"))
                        .and_then(|v| v.as_array())
                        .is_some_and(|arr| {
                            arr.iter()
                                .any(|e| crate::claude_settings::entry_is_ctx_hook_http_endpoint(e))
                        })
                })
                .count();
            println!(
                "  ctx v2 hooks: UserPromptSubmit (ctx)={ups_ctx}, async HTTP to dashboard keys={async_ctx}"
            );
        }
    }

    if config.proxy_mode.mitm_active() && !listening {
        println!(
            "\n{} Proxy mode is {} but nothing is listening on :{port}. Run `ctx proxy start` or check launchd.",
            "!".yellow(),
            config.proxy_mode.as_str()
        );
    }

    Ok(())
}
