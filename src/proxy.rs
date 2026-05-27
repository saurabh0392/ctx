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
use crate::{analytics, config::Config};

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
    println!("  Upstream: {upstream}");
    println!("  MITM:     CONNECT {}:443 (HTTPS_PROXY mode)", ca::ANTHROPIC_API_HOST);
    println!("  Profile:  {active}");
    println!("  Auto-profile: {}", if config.auto_profile_enabled { "on" } else { "off" });
    println!("  Inject:   {}", if config.inject_enabled { "on" } else { "off" });
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
        run_gates(&body_bytes)
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
        .body(filtered_bytes);

    let upstream_resp = req_builder.send().await?;
    let status = HyperStatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(HyperStatusCode::INTERNAL_SERVER_ERROR);

    let mut response = HyperResponse::builder().status(status);
    for (k, v) in upstream_resp.headers() {
        let n = k.as_str().to_lowercase();
        if n == "transfer-encoding" || n == "connection" || n == "keep-alive" {
            continue;
        }
        response = response.header(k, v);
    }
    let full_body = upstream_resp.bytes().await?;
    Ok(response.body(Body::from(full_body)).unwrap())
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
        run_gates(&body_bytes)
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
        .body(filtered_bytes);

    let upstream_resp = req_builder.send().await?;
    let status = HyperStatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(HyperStatusCode::INTERNAL_SERVER_ERROR);

    let mut response = HyperResponse::builder().status(status);
    for (k, v) in upstream_resp.headers() {
        let n = k.as_str().to_lowercase();
        if n == "transfer-encoding" || n == "connection" || n == "keep-alive" {
            continue;
        }
        response = response.header(k, v);
    }
    let full_body = upstream_resp.bytes().await?;
    Ok(response.body(Body::from(full_body)).unwrap())
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
        run_gates(&body_bytes)
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
        .body(filtered_bytes);

    let upstream_resp = req_builder.send().await.map_err(|e| {
        eprintln!("[ctx] upstream error: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response = Response::builder().status(status);
    for (k, v) in upstream_resp.headers() {
        let n = k.as_str().to_lowercase();
        if n == "transfer-encoding" || n == "connection" || n == "keep-alive" {
            continue;
        }
        response = response.header(k, v);
    }

    let body = Body::from_stream(upstream_resp.bytes_stream());
    Ok(response.body(body).unwrap())
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
    let active_slug = config.active_profile.as_deref().unwrap_or("all").to_string();

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
    let mut bytes = filter_result.body;

    let mut inject_fired = false;
    let mut coach_kind: Option<String> = None;
    let mut budget_fired = false;
    let mut behavior_kind: Option<String> = None;

    if removed > 0 {
        let tokens_saved = bytes_before.saturating_sub(bytes.len()) / 4;
        eprintln!("[ctx] -{removed} tools (~{tokens_saved} tokens)  profile={effective_slug}");
        analytics::record(removed, tokens_saved, &effective_slug, analytics::TraceInfo {
            removed_servers: filter_result.removed_servers,
            kept_servers: filter_result.kept_servers,
            auto_selected,
            auto_trigger: auto_trigger.clone(),
            inject_fired: false,
            coach_kind: None,
            budget_fired: false,
            behavior_kind: None,
            working_directory: working_directory.clone(),
        });
    }

    if config.inject_enabled {
        if let Some(prefix) = crate::inject::load_prefix() {
            bytes = crate::inject::inject_system(&bytes, &prefix);
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

    if removed == 0 && (inject_fired || coach_kind.is_some() || budget_fired || behavior_kind.is_some() || auto_selected) {
        analytics::record(0, 0, &effective_slug, analytics::TraceInfo {
            removed_servers: vec![],
            kept_servers: vec![],
            auto_selected,
            auto_trigger,
            inject_fired,
            coach_kind,
            budget_fired,
            behavior_kind,
            working_directory: working_directory.clone(),
        });
    } else if removed > 0 && (inject_fired || coach_kind.is_some() || budget_fired || behavior_kind.is_some()) {
        analytics::record_gates(&effective_slug, analytics::TraceInfo {
            removed_servers: vec![],
            kept_servers: vec![],
            auto_selected: false,
            auto_trigger: None,
            inject_fired,
            coach_kind,
            budget_fired,
            behavior_kind,
            working_directory: working_directory.clone(),
        });
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

fn auto_profile_info(body: &[u8], active_slug: &str) -> Option<(String, String)> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;

    let system_text = match value.get("system") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };

    if let Some((slug, trigger)) = crate::profiles::auto_select(&system_text, active_slug) {
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

fn set_env_object(settings: &mut serde_json::Value, env: serde_json::Map<String, serde_json::Value>) {
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

pub fn install(port: u16, upstream: &str) -> Result<()> {
    if !is_proxy_listening(port) {
        anyhow::bail!(
            "ctx proxy is not listening on port {port}.\n\
             Start it first, then re-run install:\n\
             \n  ctx proxy start --port {port}\n\
             \nOr use `ctx setup` which handles the ordering automatically."
        );
    }

    crate::filter_hook::write_filter_js()?;
    crate::filter_hook::sync_filter_config_from_active_config()?;
    let filter_abs = crate::filter_hook::filter_js_abs_path_string()?;

    let settings_path = crate::config::claude_settings_path();
    let content = std::fs::read_to_string(&settings_path)
        .context("Failed to read ~/.claude/settings.json")?;
    let mut settings: serde_json::Value = serde_json::from_str(&content)?;

    let mut env = env_object(&settings);
    let proxy_http = proxy_url_for_port(port);

    let prev_anthropic = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let ca_path_opt = ca::canonical_ca_cert_path_string().ok();

    let curr_node = env.get("NODE_OPTIONS").and_then(|v| v.as_str()).unwrap_or("");
    let merged = crate::filter_hook::merge_node_options_require(Some(curr_node), &filter_abs);
    let has_forward = is_ctx_forward_proxy_url(
        env.get("HTTPS_PROXY").and_then(|v| v.as_str()).unwrap_or(""),
        port,
    ) || is_ctx_forward_proxy_url(
        env.get("CLAUDE_CODE_HTTPS_PROXY").and_then(|v| v.as_str()).unwrap_or(""),
        port,
    );
    let extra_ours = ca_path_opt
        .as_deref()
        .zip(env.get("NODE_EXTRA_CA_CERTS").and_then(|v| v.as_str()))
        .map(|(a, b)| a == b)
        .unwrap_or(false);

    if merged == curr_node && !has_forward && !extra_ours {
        println!("Already installed (NODE_OPTIONS in-process filter)");
        return Ok(());
    }

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

    if let Some(ref u) = prev_anthropic {
        if is_ctx_reverse_base_url(u, port) {
            env.remove("ANTHROPIC_BASE_URL");
        }
    }

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

    if let Some(ref ca_path) = ca_path_opt {
        if env.get("NODE_EXTRA_CA_CERTS").and_then(|v| v.as_str()) == Some(ca_path.as_str()) {
            env.remove("NODE_EXTRA_CA_CERTS");
        }
    }

    env.insert(
        "NODE_OPTIONS".to_string(),
        serde_json::Value::String(merged),
    );

    set_env_object(&mut settings, env);
    crate::settings_hooks::merge_compress_hook(&mut settings)?;
    crate::config::write_json_atomic(&settings_path, &settings)?;

    let mut config = Config::load();
    config.proxy_port = Some(port);
    config.proxy_upstream = Some(upstream.to_string());
    config.original_base_url = Some(original_base);
    config.proxy_install_mode = Some("node_inject".to_string());
    config.save()?;

    let host = crate::host::detect_primary_host();
    println!(
        "{} Claude Code wired for in-process tool filtering",
        "✓".green().bold()
    );
    println!("  NODE_OPTIONS includes --require {}", filter_abs);
    println!("  API traffic uses first-party TLS to Anthropic (no HTTPS_PROXY)");
    println!(
        "  ctx proxy on {} still serves dashboard, analytics, and inject when used",
        proxy_http
    );
    println!("\nNext steps:");
    println!("  1. {}", host.reload_instruction());
    println!("     (re-reads NODE_OPTIONS and MCP config without quitting)");
    println!("  2. Run {} if you change focus profile", "`ctx use carrier`".bold());

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
    if !settings_path.exists() {
        let mut cfg = Config::load();
        cfg.proxy_install_mode = None;
        let _ = cfg.save();
        return Ok(());
    }
    let content = std::fs::read_to_string(&settings_path)?;
    let mut settings: serde_json::Value = serde_json::from_str(&content)?;
    let mut env = env_object(&settings);

    let still_points_at_ctx_reverse = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|u| is_ctx_reverse_base_url(u, port))
        .unwrap_or(false);

    let our_ca = ca::canonical_ca_cert_path_string().ok();

    for key in ["HTTPS_PROXY", "HTTP_PROXY", "CLAUDE_CODE_HTTPS_PROXY", "CLAUDE_CODE_HTTP_PROXY"] {
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

    if let Some(curr) = env.get("NODE_OPTIONS").and_then(|v| v.as_str()) {
        match crate::filter_hook::strip_ctx_require_from_node_options(Some(curr)) {
            Some(stripped) if !stripped.trim().is_empty() => {
                env.insert("NODE_OPTIONS".to_string(), serde_json::Value::String(stripped));
            }
            _ => {
                env.remove("NODE_OPTIONS");
            }
        }
    }

    let mode = config.proxy_install_mode.as_deref();
    if mode == Some("reverse") || still_points_at_ctx_reverse {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            serde_json::Value::String(original.to_string()),
        );
    }

    set_env_object(&mut settings, env);
    crate::settings_hooks::strip_compress_hook(&mut settings)?;
    crate::settings_hooks::strip_gain_stop_hook(&mut settings)?;
    crate::config::write_json_atomic(&settings_path, &settings)?;

    let mut cfg = Config::load();
    cfg.proxy_install_mode = None;
    cfg.save()?;

    println!("{} Uninstalled ctx proxy env from settings.json", "✓".green());
    println!("{}", crate::host::detect_primary_host().reload_instruction());
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

    println!("Proxy:    http://127.0.0.1:{port} -> {upstream}");
    println!("Profile:  {profile}");
    println!("Auto-profile: {}", if config.auto_profile_enabled { "on" } else { "off" });
    println!("Inject:   {}", if config.inject_enabled { "on" } else { "off" });

    let settings_path = crate::config::claude_settings_path();
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
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
            let node_opts = settings
                .pointer("/env/NODE_OPTIONS")
                .and_then(|v| v.as_str())
                .unwrap_or("not set");
            let mitm_on = is_ctx_forward_proxy_url(https_proxy, port);
            println!(
                "MITM:     {} (HTTPS_PROXY={})",
                if mitm_on { "yes".green().to_string() } else { "no".red().to_string() },
                https_proxy
            );
            println!("  ANTHROPIC_BASE_URL={anthropic}");
            println!("  NODE_EXTRA_CA_CERTS={node_extra}");
            println!("  NODE_OPTIONS={node_opts}");
        }
    }

    Ok(())
}
