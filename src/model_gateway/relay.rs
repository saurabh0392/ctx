//! Byte-faithful, transformation-off HTTP/SSE relay for M1.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::header::{
    HeaderName, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, TRANSFER_ENCODING, UPGRADE,
};
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Serialize;

use super::registry::ModelRoute;
use super::shadow::{ShadowEngine, ShadowHealthReceipt};

#[cfg(not(test))]
const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
#[cfg(test)]
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub(super) struct RelayState {
    route: ModelRoute,
    upstream: reqwest::Url,
    client: reqwest::Client,
    shadow: ShadowEngine,
    health_nonce: Option<String>,
}

impl RelayState {
    #[cfg(test)]
    pub(super) fn new(
        route: ModelRoute,
        upstream: reqwest::Url,
        client: reqwest::Client,
    ) -> anyhow::Result<Self> {
        Self::new_with_health_nonce(route, upstream, client, None)
    }

    pub(super) fn new_with_health_nonce(
        route: ModelRoute,
        upstream: reqwest::Url,
        client: reqwest::Client,
        health_nonce: Option<String>,
    ) -> anyhow::Result<Self> {
        route.validate()?;
        if upstream.username() != ""
            || upstream.password().is_some()
            || upstream.fragment().is_some()
        {
            anyhow::bail!("model upstream cannot contain credentials or a fragment");
        }
        Ok(Self {
            route,
            upstream,
            client,
            shadow: ShadowEngine::new(crate::config::Config::load()),
            health_nonce,
        })
    }
}

pub(super) fn router(state: Arc<RelayState>) -> Router {
    Router::new()
        .route("/__ctx/health", get(health))
        .fallback(any(relay))
        .with_state(state)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthReceipt {
    schema_version: u32,
    status: &'static str,
    route_id: String,
    surface: &'static str,
    protocol: &'static str,
    authentication: &'static str,
    fixed_upstream: &'static str,
    upstream_verified: bool,
    transformations: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_nonce: Option<String>,
    shadow: ShadowHealthReceipt,
}

async fn health(State(state): State<Arc<RelayState>>) -> impl IntoResponse {
    Json(HealthReceipt {
        schema_version: 1,
        status: "listener-ready",
        route_id: state.route.id.clone(),
        surface: state.route.surface.as_str(),
        protocol: state.route.protocol.as_str(),
        authentication: state.route.authentication.as_str(),
        fixed_upstream: state.route.upstream.origin(),
        upstream_verified: false,
        transformations: match state.route.mode {
            super::registry::ModelRouteMode::Shadow => "off",
            super::registry::ModelRouteMode::Testing => "testing",
        },
        instance_nonce: state.health_nonce.clone(),
        shadow: state.shadow.health(),
    })
}

async fn relay(State(state): State<Arc<RelayState>>, request: Request<Body>) -> Response<Body> {
    if request.method() != Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "method-not-allowed");
    }
    if request.uri().path() != state.route.endpoint_path() {
        return error_response(StatusCode::NOT_FOUND, "route-path-not-allowed");
    }
    if request.headers().contains_key("origin") {
        return error_response(StatusCode::FORBIDDEN, "browser-origin-not-allowed");
    }

    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(_) => return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request-too-large"),
    };
    let mut upstream = state.upstream.clone();
    upstream.set_query(parts.uri.query());
    let observation = state.shadow.observe(&state.route, &parts.headers, &body);
    let prepared =
        super::apply::prepare_request(&state.route, &body, &observation, state.shadow.config());
    state.shadow.record_testing_prepare(
        prepared.mutated(),
        prepared.trims.len(),
        &prepared.reasons,
    );
    let mut headers = forward_request_headers(&parts.headers);
    if prepared.mutated() {
        headers.remove(CONTENT_LENGTH);
        headers.remove("content-md5");
        headers.remove("digest");
    }
    let response = match state
        .client
        .request(parts.method, upstream)
        .headers(headers)
        .body(prepared.body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "upstream-request-failed"),
    };

    let status = response.status();
    let sse = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    let headers = forward_response_headers(response.headers());
    if status.is_success() && !sse {
        accept_prepared(&prepared.trims);
    }
    let mut acceptance_pending = status.is_success() && sse && !prepared.trims.is_empty();
    let mut sse_probe = Vec::new();
    let trims = prepared.trims;
    let stream = response.bytes_stream().map(move |chunk| {
        if acceptance_pending {
            if let Ok(bytes) = &chunk {
                let remaining = 64 * 1024usize - sse_probe.len().min(64 * 1024);
                sse_probe.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                if contains_complete_sse_data_event(&sse_probe) {
                    accept_prepared(&trims);
                    acceptance_pending = false;
                }
            }
        }
        chunk.map_err(std::io::Error::other)
    });
    let mut outgoing = Response::new(Body::from_stream(stream));
    *outgoing.status_mut() = status;
    *outgoing.headers_mut() = headers;
    outgoing
}

fn accept_prepared(trims: &[crate::tool_result::PreparedTextTrim]) {
    for trim in trims {
        if let Err(error) = crate::tool_result::mark_text_trim_accepted(trim) {
            eprintln!("ctx model gateway could not record an accepted trim: {error}");
        }
    }
}

fn contains_complete_sse_data_event(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    while start < bytes.len() {
        let lf = bytes[start..]
            .windows(2)
            .position(|window| window == b"\n\n")
            .map(|index| (start + index, 2));
        let crlf = bytes[start..]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| (start + index, 4));
        let Some((end, delimiter_len)) = (match (lf, crlf) {
            (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
            (Some(found), None) | (None, Some(found)) => Some(found),
            (None, None) => None,
        }) else {
            return false;
        };
        if bytes[start..end]
            .split(|byte| *byte == b'\n')
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .any(|line| line.starts_with(b"data:"))
        {
            return true;
        }
        start = end + delimiter_len;
    }
    false
}

#[derive(Serialize)]
struct ErrorReceipt {
    error: &'static str,
}

fn error_response(status: StatusCode, error: &'static str) -> Response<Body> {
    let mut response = Json(ErrorReceipt { error }).into_response();
    *response.status_mut() = status;
    response
}

fn forward_request_headers(source: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let connection_tokens = connection_tokens(source);
    for (name, value) in source {
        if !blocked_request_header(name, &connection_tokens) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers
}

fn forward_response_headers(source: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let connection_tokens = connection_tokens(source);
    for (name, value) in source {
        if !hop_by_hop_header(name) && !connection_tokens.contains(name.as_str()) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers
}

fn blocked_request_header(
    name: &HeaderName,
    connection_tokens: &std::collections::BTreeSet<String>,
) -> bool {
    hop_by_hop_header(name)
        || connection_tokens.contains(name.as_str())
        || name == HOST
        || name.as_str().eq_ignore_ascii_case("forwarded")
        || name.as_str().starts_with("x-forwarded-")
        || name.as_str().starts_with("x-ctx-")
}

fn connection_tokens(headers: &HeaderMap) -> std::collections::BTreeSet<String> {
    headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn hop_by_hop_header(name: &HeaderName) -> bool {
    name == CONNECTION
        || name == TRANSFER_ENCODING
        || name == UPGRADE
        || matches!(
            name.as_str(),
            "keep-alive" | "proxy-authenticate" | "proxy-authorization" | "te" | "trailer"
        )
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use axum::extract::State;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, LOCATION};
    use axum::routing::post;
    use futures_util::stream;
    use serde_json::Value;

    use super::*;
    use crate::model_gateway::registry::{ModelRouteMode, ProviderTarget};
    use crate::model_gateway::route::{AuthenticationMode, WireProtocol};
    use crate::surface::SurfaceId;

    #[derive(Clone, Default)]
    struct Capture {
        bodies: Arc<Mutex<Vec<Vec<u8>>>>,
        headers: Arc<Mutex<Vec<HeaderMap>>>,
        uris: Arc<Mutex<Vec<String>>>,
    }

    fn codex_route() -> ModelRoute {
        ModelRoute {
            id: "codex-test".into(),
            surface: SurfaceId::Codex,
            protocol: WireProtocol::OpenAiResponses,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::OpenAi,
            listen_port: 8871,
            mode: ModelRouteMode::Shadow,
        }
    }

    fn claude_route() -> ModelRoute {
        ModelRoute {
            id: "claude-test".into(),
            surface: SurfaceId::ClaudeCode,
            protocol: WireProtocol::AnthropicMessages,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::Anthropic,
            listen_port: 8872,
            mode: ModelRouteMode::Shadow,
        }
    }

    fn testing_codex_route() -> ModelRoute {
        let mut route = codex_route();
        route.mode = ModelRouteMode::Testing;
        route
    }

    fn synthetic_responses_body(label: &str) -> Vec<u8> {
        let output = (0..100)
            .map(|index| format!("{label} synthetic line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::to_vec(&serde_json::json!({"input":[
            {"type":"function_call","call_id":"call","name":"ctx_synthetic_echo","arguments":"{\"contract\":\"ctx-synthetic-v1\"}"},
            {"type":"function_call_output","call_id":"call","output":output}
        ]}))
        .unwrap()
    }

    struct ScopedCtxHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<std::ffi::OsString>,
    }

    impl ScopedCtxHome {
        fn set(path: &std::path::Path) -> Self {
            let lock = crate::test_lock::CTX_ENV_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let previous = std::env::var_os("CTX_HOME");
            std::env::set_var("CTX_HOME", path);
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for ScopedCtxHome {
        fn drop(&mut self) {
            if let Some(value) = self.previous.take() {
                std::env::set_var("CTX_HOME", value);
            } else {
                std::env::remove_var("CTX_HOME");
            }
        }
    }

    fn applied_count() -> i64 {
        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM compress_decisions WHERE applied=1",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap()
    }

    async fn spawn(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), handle)
    }

    async fn capture_request(
        State(capture): State<Capture>,
        request: Request<Body>,
    ) -> Response<Body> {
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, MAX_REQUEST_BYTES).await.unwrap();
        capture.bodies.lock().unwrap().push(body.to_vec());
        capture.uris.lock().unwrap().push(parts.uri.to_string());
        capture.headers.lock().unwrap().push(parts.headers);
        Response::new(Body::from(body))
    }

    async fn gateway_for(upstream: &str) -> (String, tokio::task::JoinHandle<()>) {
        gateway_for_route(upstream, codex_route()).await
    }

    async fn gateway_for_route(
        upstream: &str,
        route: ModelRoute,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let state = Arc::new(
            RelayState::new(route, reqwest::Url::parse(upstream).unwrap(), test_client()).unwrap(),
        );
        spawn(router(state)).await
    }

    #[tokio::test]
    async fn recorded_request_body_is_byte_identical_and_routing_headers_are_stripped() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .route("/v1/responses", post(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;
        let body = include_bytes!(
            "../../tests/fixtures/model_gateway/pass_through/openai-responses-body.json"
        );

        let response = test_client()
            .post(format!("{gateway}/v1/responses?mode=recorded"))
            .header(AUTHORIZATION, "Bearer seeded-secret")
            .header(CONTENT_TYPE, "application/json")
            .header("x-forwarded-host", "evil.example")
            .header("x-ctx-route", "other-route")
            .header("connection", "x-remove-me")
            .header("x-remove-me", "routing-control")
            .body(body.as_slice())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap().as_ref(), body);
        assert_eq!(capture.bodies.lock().unwrap().as_slice(), &[body.to_vec()]);
        assert_eq!(
            capture.uris.lock().unwrap().as_slice(),
            &["/v1/responses?mode=recorded"]
        );
        {
            let headers = capture.headers.lock().unwrap();
            assert_eq!(
                headers[0].get(AUTHORIZATION).unwrap(),
                "Bearer seeded-secret"
            );
            assert!(headers[0].get("x-forwarded-host").is_none());
            assert!(headers[0].get("x-ctx-route").is_none());
            assert!(headers[0].get("x-remove-me").is_none());
        }
        let health: Value = test_client()
            .get(format!("{gateway}/__ctx/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["transformations"], "off");
        assert_eq!(health["shadow"]["requestsObserved"], 1);
        assert_eq!(health["shadow"]["exchangesCorrelated"], 1);
        assert_eq!(health["shadow"]["decisionsComputed"], 1);
        assert_eq!(health["shadow"]["rawRequestsPersisted"], false);
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn anthropic_recorded_body_and_api_key_are_byte_identical() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .route("/v1/messages", post(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) =
            gateway_for_route(&format!("{upstream}/v1/messages"), claude_route()).await;
        let body = include_bytes!(
            "../../tests/fixtures/model_gateway/pass_through/anthropic-messages-body.json"
        );

        let response = test_client()
            .post(format!("{gateway}/v1/messages"))
            .header("x-api-key", "seeded-secret")
            .header("anthropic-version", "2023-06-01")
            .body(body.as_slice())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap().as_ref(), body);
        assert_eq!(capture.bodies.lock().unwrap().as_slice(), &[body.to_vec()]);
        {
            let headers = capture.headers.lock().unwrap();
            assert_eq!(headers[0].get("x-api-key").unwrap(), "seeded-secret");
        }
        let health: Value = test_client()
            .get(format!("{gateway}/__ctx/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["transformations"], "off");
        assert_eq!(health["shadow"]["mode"], "shadow");
        assert_eq!(health["shadow"]["requestsObserved"], 1);
        assert_eq!(health["shadow"]["exchangesCorrelated"], 1);
        assert_eq!(health["shadow"]["decisionsComputed"], 1);
        assert_eq!(health["shadow"]["rawRequestsPersisted"], false);
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn wrong_path_and_method_fail_before_upstream() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .fallback(any(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let wrong_path = test_client()
            .post(format!("{gateway}/v1/messages"))
            .body("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_path.status(), StatusCode::NOT_FOUND);
        let wrong_method = test_client()
            .get(format!("{gateway}/v1/responses"))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert!(capture.bodies.lock().unwrap().is_empty());
        upstream_task.abort();
        gateway_task.abort();
    }

    async fn delayed_sse() -> Response<Body> {
        let chunks = stream::unfold(0, |index| async move {
            match index {
                0 => Some((Ok::<_, Infallible>("data: first\n\n"), 1)),
                1 => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Some((Ok::<_, Infallible>("data: second\n\n"), 2))
                }
                _ => None,
            }
        });
        let mut response = Response::new(Body::from_stream(chunks));
        response
            .headers_mut()
            .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
        response
    }

    #[tokio::test]
    async fn sse_is_streamed_without_waiting_for_completion() {
        let upstream_app = Router::new().route("/v1/responses", post(delayed_sse));
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let started = Instant::now();
        let response = test_client()
            .post(format!("{gateway}/v1/responses"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.bytes_stream();
        let first = stream.next().await.unwrap().unwrap();
        assert!(started.elapsed() < Duration::from_millis(300));
        assert_eq!(first.as_ref(), b"data: first\n\n");
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.as_ref(), b"data: second\n\n");
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn upstream_redirect_is_returned_and_never_followed() {
        let redirected_hits = Arc::new(Mutex::new(0usize));
        let hits = redirected_hits.clone();
        let redirected = Router::new().fallback(any(move || {
            let hits = hits.clone();
            async move {
                *hits.lock().unwrap() += 1;
                StatusCode::OK
            }
        }));
        let (redirected_url, redirected_task) = spawn(redirected).await;
        let location = format!("{redirected_url}/stolen");
        let upstream_app = Router::new().route(
            "/v1/responses",
            post(move || {
                let location = location.clone();
                async move {
                    let mut response = Response::new(Body::empty());
                    *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
                    response
                        .headers_mut()
                        .insert(LOCATION, location.parse().unwrap());
                    response
                }
            }),
        );
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let response = test_client()
            .post(format!("{gateway}/v1/responses"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(*redirected_hits.lock().unwrap(), 0);
        redirected_task.abort();
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn provider_error_status_headers_and_body_are_preserved() {
        let upstream_app = Router::new().route(
            "/v1/responses",
            post(|| async {
                let mut response = Response::new(Body::from("provider-error-body"));
                *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                response
                    .headers_mut()
                    .insert("x-ratelimit-reset", "42".parse().unwrap());
                response
            }),
        );
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let response = test_client()
            .post(format!("{gateway}/v1/responses"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["x-ratelimit-reset"], "42");
        assert_eq!(response.bytes().await.unwrap(), "provider-error-body");
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn health_is_local_and_content_free() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .fallback(any(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let receipt: Value = test_client()
            .get(format!("{gateway}/__ctx/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(receipt["status"], "listener-ready");
        assert_eq!(receipt["upstreamVerified"], false);
        assert_eq!(receipt["transformations"], "off");
        assert_eq!(receipt["authentication"], "api-key");
        assert_eq!(receipt["fixedUpstream"], "https://api.openai.com");
        assert!(capture.bodies.lock().unwrap().is_empty());
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn lifecycle_health_nonce_proves_the_exact_listener_without_upstream_traffic() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .fallback(any(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let state = Arc::new(
            RelayState::new_with_health_nonce(
                codex_route(),
                reqwest::Url::parse(&format!("{upstream}/v1/responses")).unwrap(),
                test_client(),
                Some("00112233445566778899aabbccddeeff".into()),
            )
            .unwrap(),
        );
        let (gateway, gateway_task) = spawn(router(state)).await;

        let receipt: Value = test_client()
            .get(format!("{gateway}/__ctx/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(receipt["instanceNonce"], "00112233445566778899aabbccddeeff");
        assert_eq!(receipt["routeId"], "codex-test");
        assert!(capture.bodies.lock().unwrap().is_empty());
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn oversized_and_browser_origin_requests_never_reach_upstream() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .fallback(any(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for(&format!("{upstream}/v1/responses")).await;

        let oversized = test_client()
            .post(format!("{gateway}/v1/responses"))
            .body(vec![b'x'; MAX_REQUEST_BYTES + 1])
            .send()
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let browser = test_client()
            .post(format!("{gateway}/v1/responses"))
            .header("origin", "https://attacker.example")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(browser.status(), StatusCode::FORBIDDEN);
        assert!(capture.bodies.lock().unwrap().is_empty());
        upstream_task.abort();
        gateway_task.abort();
    }

    #[test]
    fn testing_route_mutates_upstream_and_counts_only_successful_http_acceptance() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _home = ScopedCtxHome::set(temp.path());
        runtime.block_on(async {
            let capture = Capture::default();
            let upstream_app = Router::new()
                .route("/v1/responses", post(capture_request))
                .with_state(capture.clone());
            let (upstream, upstream_task) = spawn(upstream_app).await;
            let (gateway, gateway_task) =
                gateway_for_route(&format!("{upstream}/v1/responses"), testing_codex_route()).await;
            let body = synthetic_responses_body("accepted");
            let response = test_client()
                .post(format!("{gateway}/v1/responses"))
                .body(body.clone())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let returned = response.bytes().await.unwrap();
            assert_ne!(returned.as_ref(), body);
            let upstream_body = capture.bodies.lock().unwrap()[0].clone();
            assert_eq!(returned.as_ref(), upstream_body);
            assert!(String::from_utf8_lossy(&upstream_body).contains("ctx trimmed this output"));
            assert_eq!(applied_count(), 1);
            let conn = crate::db::open_db().unwrap();
            let rewind_id: String = conn
                .query_row(
                    "SELECT rewind_id FROM compress_decisions WHERE applied=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let rewind = crate::db::get_rewind(&conn, &rewind_id).unwrap();
            assert!(rewind.original.starts_with("accepted synthetic line 0"));
            assert!(!rewind.original.contains("ctx trimmed this output"));
            upstream_task.abort();
            gateway_task.abort();
        });
    }

    #[test]
    fn provider_rejection_keeps_rewind_but_never_counts_applied() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _home = ScopedCtxHome::set(temp.path());
        runtime.block_on(async {
            let capture = Capture::default();
            let upstream_app = Router::new()
                .route(
                    "/v1/responses",
                    post(
                        |State(capture): State<Capture>, request: Request<Body>| async move {
                            let (parts, body) = request.into_parts();
                            let body = to_bytes(body, MAX_REQUEST_BYTES).await.unwrap();
                            capture.bodies.lock().unwrap().push(body.to_vec());
                            capture.headers.lock().unwrap().push(parts.headers);
                            let mut response = Response::new(Body::from("rejected"));
                            *response.status_mut() = StatusCode::BAD_REQUEST;
                            response
                        },
                    ),
                )
                .with_state(capture.clone());
            let (upstream, upstream_task) = spawn(upstream_app).await;
            let (gateway, gateway_task) =
                gateway_for_route(&format!("{upstream}/v1/responses"), testing_codex_route()).await;
            let response = test_client()
                .post(format!("{gateway}/v1/responses"))
                .body(synthetic_responses_body("rejected"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(applied_count(), 0);
            let conn = crate::db::open_db().unwrap();
            let rewinds: i64 = conn
                .query_row("SELECT COUNT(*) FROM rewind_store", [], |row| row.get(0))
                .unwrap();
            assert_eq!(rewinds, 1, "prepared recovery survives rejection");
            upstream_task.abort();
            gateway_task.abort();
        });
    }

    #[test]
    fn upstream_transport_failure_never_counts_applied() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _home = ScopedCtxHome::set(temp.path());
        runtime.block_on(async {
            let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let unavailable = unused.local_addr().unwrap();
            drop(unused);
            let (gateway, gateway_task) = gateway_for_route(
                &format!("http://{unavailable}/v1/responses"),
                testing_codex_route(),
            )
            .await;
            let response = test_client()
                .post(format!("{gateway}/v1/responses"))
                .body(synthetic_responses_body("transport-failure"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert_eq!(applied_count(), 0);
            let conn = crate::db::open_db().unwrap();
            let rewinds: i64 = conn
                .query_row("SELECT COUNT(*) FROM rewind_store", [], |row| row.get(0))
                .unwrap();
            assert_eq!(rewinds, 1);
            gateway_task.abort();
        });
    }

    #[test]
    fn sse_acceptance_waits_for_the_first_complete_data_event() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _home = ScopedCtxHome::set(temp.path());
        runtime.block_on(async {
            let upstream_app = Router::new().route(
                "/v1/responses",
                post(|request: Request<Body>| async move {
                    let (_, body) = request.into_parts();
                    let _ = to_bytes(body, MAX_REQUEST_BYTES).await.unwrap();
                    let chunks = stream::unfold(0, |index| async move {
                        match index {
                            0 => Some((Ok::<_, Infallible>(": keepalive\n\n"), 1)),
                            1 => {
                                tokio::time::sleep(Duration::from_millis(300)).await;
                                Some((Ok::<_, Infallible>("data: accepted\n\n"), 2))
                            }
                            _ => None,
                        }
                    });
                    let mut response = Response::new(Body::from_stream(chunks));
                    response
                        .headers_mut()
                        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
                    response
                }),
            );
            let (upstream, upstream_task) = spawn(upstream_app).await;
            let (gateway, gateway_task) =
                gateway_for_route(&format!("{upstream}/v1/responses"), testing_codex_route()).await;
            let response = test_client()
                .post(format!("{gateway}/v1/responses"))
                .body(synthetic_responses_body("sse"))
                .send()
                .await
                .unwrap();
            assert_eq!(applied_count(), 0);
            let mut chunks = response.bytes_stream();
            assert_eq!(chunks.next().await.unwrap().unwrap(), ": keepalive\n\n");
            assert_eq!(applied_count(), 0);
            assert_eq!(chunks.next().await.unwrap().unwrap(), "data: accepted\n\n");
            assert_eq!(applied_count(), 1);
            upstream_task.abort();
            gateway_task.abort();
        });
    }

    #[test]
    fn complete_sse_detection_handles_fragmented_and_crlf_events() {
        assert!(!contains_complete_sse_data_event(b"data: part"));
        assert!(contains_complete_sse_data_event(b"data: part\n\n"));
        assert!(contains_complete_sse_data_event(
            b"event: x\r\ndata: ok\r\n\r\n"
        ));
        assert!(!contains_complete_sse_data_event(b": keepalive\n\n"));
    }
}
