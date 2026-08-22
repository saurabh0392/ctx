//! Protocol-faithful HTTP/SSE/WebSocket relay for model routes.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{to_bytes, Body};
use axum::extract::ws::{
    CloseFrame as AxumCloseFrame, Message as AxumMessage, WebSocket, WebSocketUpgrade,
};
use axum::extract::{FromRequestParts, State};
use axum::http::header::{
    HeaderName, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, TRANSFER_ENCODING, UPGRADE,
};
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::{
    frame::coding::CloseCode as TungsteniteCloseCode, CloseFrame as TungsteniteCloseFrame,
    WebSocketConfig,
};
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

use super::registry::ModelRoute;
use super::shadow::{ShadowEngine, ShadowHealthReceipt};

#[cfg(not(test))]
const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
#[cfg(test)]
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_LOCAL_PROCESSING_TIME: Duration = Duration::from_millis(500);
const UPSTREAM_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WEBSOCKET_WRITE_BUFFER_BYTES: usize = 128 * 1024;
const WEBSOCKET_MAX_WRITE_BUFFER_BYTES: usize = MAX_REQUEST_BYTES + WEBSOCKET_WRITE_BUFFER_BYTES;
const SAFE_WEBSOCKET_RESPONSE_HEADERS: [&str; 4] = [
    "x-reasoning-included",
    "x-models-etag",
    "openai-model",
    "x-codex-turn-state",
];

#[derive(Clone)]
pub(super) struct RelayState {
    route: ModelRoute,
    upstream: reqwest::Url,
    client: reqwest::Client,
    shadow: ShadowEngine,
    health_nonce: Option<String>,
    surface_version: Option<String>,
    evidence_enabled: bool,
    processing_deadline: Duration,
    #[cfg(test)]
    processing_delay: Option<Duration>,
}

impl RelayState {
    #[cfg(test)]
    pub(super) fn new(
        route: ModelRoute,
        upstream: reqwest::Url,
        client: reqwest::Client,
    ) -> anyhow::Result<Self> {
        Self::new_with_health_nonce(route, upstream, client, None, None)
    }

    pub(super) fn new_with_health_nonce(
        route: ModelRoute,
        upstream: reqwest::Url,
        client: reqwest::Client,
        health_nonce: Option<String>,
        surface_version: Option<String>,
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
            surface_version,
            evidence_enabled: cfg!(not(test)),
            processing_deadline: MAX_LOCAL_PROCESSING_TIME,
            #[cfg(test)]
            processing_delay: None,
        })
    }

    fn record(
        &self,
        outcome: &'static str,
        quantity: usize,
        reason_code: Option<&str>,
        chars: Option<(usize, usize)>,
        latency_ms: Option<u64>,
        local_processing_ms: Option<u64>,
    ) {
        if !self.evidence_enabled {
            return;
        }
        let (chars_in, chars_out) =
            chars.map_or((None, None), |(input, output)| (Some(input), Some(output)));
        crate::db::record_model_gateway_event_best_effort(&crate::db::ModelGatewayEvent {
            route_id: &self.route.id,
            surface: self.route.surface.as_str(),
            surface_version: self.surface_version.as_deref(),
            protocol: self.route.protocol.as_str(),
            authentication: self.route.authentication.as_str(),
            fixed_upstream: self.route.upstream.origin(),
            mode: self.route.mode.as_str(),
            outcome,
            quantity,
            reason_code,
            chars_in,
            chars_out,
            latency_ms,
            local_processing_ms,
        });
    }

    #[cfg(test)]
    fn with_test_evidence(mut self) -> Self {
        self.evidence_enabled = true;
        self
    }

    #[cfg(test)]
    fn with_test_processing_timing(mut self, delay: Duration, deadline: Duration) -> Self {
        self.processing_delay = Some(delay);
        self.processing_deadline = deadline;
        self
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
    #[serde(skip_serializing_if = "Option::is_none")]
    client_version: Option<String>,
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
        client_version: state.surface_version.clone(),
        shadow: state.shadow.health(),
    })
}

async fn relay(State(state): State<Arc<RelayState>>, request: Request<Body>) -> Response<Body> {
    if request.headers().contains_key("origin") {
        return error_response(StatusCode::FORBIDDEN, "browser-origin-not-allowed");
    }
    if request.uri().path() != state.route.endpoint_path() {
        if state
            .route
            .supports_auxiliary_path(request.method(), request.uri().path())
        {
            return relay_auxiliary(state, request).await;
        }
        return error_response(StatusCode::NOT_FOUND, "route-path-not-allowed");
    }
    if request.method() == Method::GET {
        let upgrade_requested = request
            .headers()
            .get(CONNECTION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
            && request
                .headers()
                .get(UPGRADE)
                .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
        if !upgrade_requested {
            return error_response(StatusCode::METHOD_NOT_ALLOWED, "method-not-allowed");
        }
        if !state.route.supports_websockets() {
            return error_response(StatusCode::METHOD_NOT_ALLOWED, "websocket-not-supported");
        }
        return relay_websocket(state, request).await;
    }
    if request.method() != Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "method-not-allowed");
    }

    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(_) => return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request-too-large"),
    };
    let started = Instant::now();
    let mut upstream = state.upstream.clone();
    upstream.set_query(parts.uri.query());
    let (prepared, local_processing_ms) = prepare_outbound(&state, &parts.headers, &body).await;
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
        Err(_) => {
            state.record(
                "transport-failure",
                1,
                Some("upstream-request-failed"),
                None,
                Some(elapsed_ms(started)),
                Some(local_processing_ms),
            );
            return error_response(StatusCode::BAD_GATEWAY, "upstream-request-failed");
        }
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
        let latency_ms = elapsed_ms(started);
        state.record(
            "accepted",
            1,
            Some("http-success"),
            None,
            Some(latency_ms),
            Some(local_processing_ms),
        );
        accept_prepared(&state, &prepared.trims, latency_ms, local_processing_ms);
    } else if !status.is_success() {
        state.record(
            "provider-rejected",
            1,
            Some("provider-non-success"),
            None,
            Some(elapsed_ms(started)),
            Some(local_processing_ms),
        );
    }
    let mut acceptance_pending = status.is_success() && sse;
    let mut sse_probe = Vec::new();
    let trims = prepared.trims;
    let receipt_state = state.clone();
    let stream = response.bytes_stream().map(move |chunk| {
        if acceptance_pending {
            if let Ok(bytes) = &chunk {
                let remaining = 64 * 1024usize - sse_probe.len().min(64 * 1024);
                sse_probe.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                if contains_complete_sse_data_event(&sse_probe) {
                    let latency_ms = elapsed_ms(started);
                    receipt_state.record(
                        "accepted",
                        1,
                        Some("sse-first-data"),
                        None,
                        Some(latency_ms),
                        Some(local_processing_ms),
                    );
                    accept_prepared(&receipt_state, &trims, latency_ms, local_processing_ms);
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

async fn relay_auxiliary(state: Arc<RelayState>, request: Request<Body>) -> Response<Body> {
    let (parts, _) = request.into_parts();
    let mut upstream = state.upstream.clone();
    upstream.set_path(parts.uri.path());
    upstream.set_query(parts.uri.query());
    let mut headers = forward_request_headers(&parts.headers);
    headers.remove(CONTENT_LENGTH);
    headers.remove("content-md5");
    headers.remove("digest");
    let response = match state
        .client
        .request(parts.method, upstream)
        .headers(headers)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return error_response(StatusCode::BAD_GATEWAY, "upstream-request-failed");
        }
    };
    let status = response.status();
    let headers = forward_response_headers(response.headers());
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let mut outgoing = Response::new(Body::from_stream(stream));
    *outgoing.status_mut() = status;
    *outgoing.headers_mut() = headers;
    outgoing
}

async fn relay_websocket(state: Arc<RelayState>, request: Request<Body>) -> Response<Body> {
    let (mut parts, _) = request.into_parts();
    let downstream_headers = parts.headers.clone();
    let query = parts.uri.query().map(ToOwned::to_owned);
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "websocket-upgrade-invalid");
        }
    };

    let mut upstream = state.upstream.clone();
    upstream.set_query(query.as_deref());
    if set_websocket_scheme(&mut upstream).is_err() {
        return error_response(StatusCode::BAD_GATEWAY, "websocket-upstream-invalid");
    }
    let mut upstream_request = match upstream.as_str().into_client_request() {
        Ok(request) => request,
        Err(_) => {
            return error_response(StatusCode::BAD_GATEWAY, "websocket-upstream-invalid");
        }
    };
    let upstream_headers = forward_websocket_request_headers(&downstream_headers);
    for (name, value) in &upstream_headers {
        upstream_request
            .headers_mut()
            .append(name.clone(), value.clone());
    }

    let config = WebSocketConfig {
        write_buffer_size: WEBSOCKET_WRITE_BUFFER_BYTES,
        max_write_buffer_size: WEBSOCKET_MAX_WRITE_BUFFER_BYTES,
        max_message_size: Some(MAX_REQUEST_BYTES),
        max_frame_size: Some(MAX_REQUEST_BYTES),
        ..Default::default()
    };
    let connected = tokio::time::timeout(
        UPSTREAM_WEBSOCKET_CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(upstream_request, Some(config), false),
    )
    .await;
    let (upstream_socket, upstream_response) = match connected {
        Ok(Ok(connected)) => connected,
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
            return error_response(response.status(), "websocket-upgrade-rejected");
        }
        Ok(Err(_)) | Err(_) => {
            return error_response(StatusCode::BAD_GATEWAY, "websocket-upstream-connect-failed");
        }
    };

    let selected_protocol = upstream_response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let upgrade = upgrade
        .write_buffer_size(WEBSOCKET_WRITE_BUFFER_BYTES)
        .max_write_buffer_size(WEBSOCKET_MAX_WRITE_BUFFER_BYTES)
        .max_message_size(MAX_REQUEST_BYTES)
        .max_frame_size(MAX_REQUEST_BYTES);
    let relay_state = state.clone();
    let processing_headers = downstream_headers;
    let mut response = if let Some(protocol) = selected_protocol {
        upgrade.protocols([protocol]).on_upgrade(move |downstream| {
            pump_websocket(relay_state, processing_headers, downstream, upstream_socket)
        })
    } else {
        upgrade.on_upgrade(move |downstream| {
            pump_websocket(relay_state, processing_headers, downstream, upstream_socket)
        })
    };
    for name in SAFE_WEBSOCKET_RESPONSE_HEADERS {
        if let Some(value) = upstream_response.headers().get(name) {
            response
                .headers_mut()
                .append(HeaderName::from_static(name), value.clone());
        }
    }
    response
}

fn set_websocket_scheme(url: &mut reqwest::Url) -> Result<(), ()> {
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err(()),
    };
    url.set_scheme(scheme).map_err(|_| ())
}

struct PendingWebSocketTurn {
    started: Instant,
    local_processing_ms: u64,
    trims: Option<Vec<crate::tool_result::PreparedTextTrim>>,
    accepted: bool,
}

async fn pump_websocket(
    state: Arc<RelayState>,
    processing_headers: HeaderMap,
    mut downstream: WebSocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let mut active: Option<PendingWebSocketTurn> = None;
    let mut queued = VecDeque::new();
    loop {
        tokio::select! {
            downstream_message = downstream.next() => {
                let Some(downstream_message) = downstream_message else { break };
                let message = match downstream_message {
                    Ok(message) => message,
                    Err(_) => break,
                };
                let closes = matches!(message, AxumMessage::Close(_));
                if let AxumMessage::Text(text) = message {
                    if is_response_create(&text) {
                        let started = Instant::now();
                        let (prepared, local_processing_ms) =
                            prepare_outbound(&state, &processing_headers, text.as_bytes()).await;
                        let mut trims = prepared.trims;
                        let outbound = match String::from_utf8(prepared.body) {
                            Ok(outbound) => outbound,
                            Err(_) => {
                                trims.clear();
                                state.record(
                                    "held-whole",
                                    1,
                                    Some("websocket-transform-invalid-utf8"),
                                    None,
                                    None,
                                    Some(local_processing_ms),
                                );
                                text.clone()
                            }
                        };
                        if upstream.send(TungsteniteMessage::Text(outbound)).await.is_err() {
                            state.record(
                                "transport-failure",
                                1,
                                Some("websocket-upstream-send-failed"),
                                None,
                                Some(elapsed_ms(started)),
                                Some(local_processing_ms),
                            );
                            break;
                        }
                        let turn = PendingWebSocketTurn {
                            started,
                            local_processing_ms,
                            trims: Some(trims),
                            accepted: false,
                        };
                        if active.is_none() {
                            active = Some(turn);
                        } else {
                            queued.push_back(turn);
                        }
                    } else if upstream.send(TungsteniteMessage::Text(text)).await.is_err() {
                        break;
                    }
                } else if upstream.send(axum_to_tungstenite(message)).await.is_err() {
                    break;
                }
                if closes {
                    break;
                }
            }
            upstream_message = upstream.next() => {
                let Some(upstream_message) = upstream_message else { break };
                let message = match upstream_message {
                    Ok(message) => message,
                    Err(_) => {
                        record_unaccepted_transport_failures(&state, &active, &queued);
                        active = None;
                        queued.clear();
                        break;
                    }
                };
                let closes = matches!(message, TungsteniteMessage::Close(_));
                if let TungsteniteMessage::Text(text) = &message {
                    observe_websocket_event(&state, text, &mut active, &mut queued);
                }
                if let Some(message) = tungstenite_to_axum(message) {
                    if downstream.send(message).await.is_err() {
                        break;
                    }
                }
                if closes {
                    break;
                }
            }
        }
    }
    record_unaccepted_held_whole(&state, active.iter().chain(queued.iter()));
}

fn is_response_create(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|kind| kind.as_str())
                .map(str::to_owned)
        })
        .as_deref()
        == Some("response.create")
}

fn observe_websocket_event(
    state: &RelayState,
    text: &str,
    active: &mut Option<PendingWebSocketTurn>,
    queued: &mut VecDeque<PendingWebSocketTurn>,
) {
    let event_type = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|kind| kind.as_str())
                .map(str::to_owned)
        });
    let Some(turn) = active.as_mut() else { return };
    let rejected = matches!(
        event_type.as_deref(),
        Some("error" | "response.failed" | "response.incomplete")
    );
    if rejected && !turn.accepted {
        state.record(
            "provider-rejected",
            1,
            Some("websocket-error-event"),
            None,
            Some(elapsed_ms(turn.started)),
            Some(turn.local_processing_ms),
        );
    } else if !turn.accepted {
        let latency_ms = elapsed_ms(turn.started);
        state.record(
            "accepted",
            1,
            Some("websocket-first-event"),
            None,
            Some(latency_ms),
            Some(turn.local_processing_ms),
        );
        if let Some(trims) = turn.trims.take() {
            accept_prepared(state, &trims, latency_ms, turn.local_processing_ms);
        }
        turn.accepted = true;
    }
    let terminal = rejected || event_type.as_deref() == Some("response.completed");
    if terminal {
        *active = queued.pop_front();
    }
}

fn record_unaccepted_transport_failures(
    state: &RelayState,
    active: &Option<PendingWebSocketTurn>,
    queued: &VecDeque<PendingWebSocketTurn>,
) {
    for turn in active
        .iter()
        .chain(queued.iter())
        .filter(|turn| !turn.accepted)
    {
        state.record(
            "transport-failure",
            1,
            Some("websocket-upstream-receive-failed"),
            None,
            Some(elapsed_ms(turn.started)),
            Some(turn.local_processing_ms),
        );
    }
}

fn record_unaccepted_held_whole<'a>(
    state: &RelayState,
    turns: impl Iterator<Item = &'a PendingWebSocketTurn>,
) {
    for turn in turns.filter(|turn| !turn.accepted) {
        state.record(
            "held-whole",
            1,
            Some("websocket-closed-before-acceptance"),
            None,
            Some(elapsed_ms(turn.started)),
            Some(turn.local_processing_ms),
        );
    }
}

fn axum_to_tungstenite(message: AxumMessage) -> TungsteniteMessage {
    match message {
        AxumMessage::Text(text) => TungsteniteMessage::Text(text),
        AxumMessage::Binary(bytes) => TungsteniteMessage::Binary(bytes),
        AxumMessage::Ping(bytes) => TungsteniteMessage::Ping(bytes),
        AxumMessage::Pong(bytes) => TungsteniteMessage::Pong(bytes),
        AxumMessage::Close(frame) => {
            TungsteniteMessage::Close(frame.map(|frame| TungsteniteCloseFrame {
                code: TungsteniteCloseCode::from(frame.code),
                reason: frame.reason,
            }))
        }
    }
}

fn tungstenite_to_axum(message: TungsteniteMessage) -> Option<AxumMessage> {
    match message {
        TungsteniteMessage::Text(text) => Some(AxumMessage::Text(text)),
        TungsteniteMessage::Binary(bytes) => Some(AxumMessage::Binary(bytes)),
        TungsteniteMessage::Ping(bytes) => Some(AxumMessage::Ping(bytes)),
        TungsteniteMessage::Pong(bytes) => Some(AxumMessage::Pong(bytes)),
        TungsteniteMessage::Close(frame) => {
            Some(AxumMessage::Close(frame.map(|frame| AxumCloseFrame {
                code: frame.code.into(),
                reason: frame.reason,
            })))
        }
        TungsteniteMessage::Frame(_) => None,
    }
}

async fn prepare_outbound(
    state: &Arc<RelayState>,
    headers: &HeaderMap,
    body: &[u8],
) -> (super::apply::PreparedModelRequest, u64) {
    state.record("attempted", 1, None, None, None, None);
    let processing_started = Instant::now();
    let processing_shadow = state.shadow.clone();
    let processing_route = state.route.clone();
    let processing_headers = headers.clone();
    let processing_body = body.to_vec();
    let processing_cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = processing_cancelled.clone();
    #[cfg(test)]
    let processing_delay = state.processing_delay;
    let processing = tokio::task::spawn_blocking(move || {
        #[cfg(test)]
        if let Some(delay) = processing_delay {
            std::thread::sleep(delay);
        }
        if worker_cancelled.load(Ordering::Acquire) {
            return (
                super::correlate::CorrelationOutcome::default(),
                super::apply::PreparedModelRequest::unchanged(&processing_body),
            );
        }
        let observation =
            processing_shadow.observe(&processing_route, &processing_headers, &processing_body);
        let prepared = super::apply::prepare_request_with_cancellation(
            &processing_route,
            &processing_body,
            &observation,
            processing_shadow.config(),
            &worker_cancelled,
        );
        processing_shadow.record_testing_prepare(
            prepared.mutated(),
            prepared.trims.len(),
            &prepared.reasons,
        );
        (observation, prepared)
    });
    let (observation, prepared, processing_failure) =
        match tokio::time::timeout(state.processing_deadline, processing).await {
            Ok(Ok((observation, prepared))) => (observation, prepared, None),
            Ok(Err(_)) => (
                super::correlate::CorrelationOutcome::default(),
                super::apply::PreparedModelRequest::unchanged(body),
                Some("processing-task-failed"),
            ),
            Err(_) => {
                processing_cancelled.store(true, Ordering::Release);
                (
                    super::correlate::CorrelationOutcome::default(),
                    super::apply::PreparedModelRequest::unchanged(body),
                    Some("transform-deadline"),
                )
            }
        };
    let local_processing_ms = elapsed_ms(processing_started);
    if let Some(reason) = processing_failure {
        state.record(
            "held-whole",
            1,
            Some(reason),
            None,
            None,
            Some(local_processing_ms),
        );
    } else if observation.exchanges.is_empty() && observation.reasons.is_empty() {
        state.record(
            "unknown",
            1,
            Some("no-tool-result-observed"),
            None,
            None,
            None,
        );
    }
    for (reason, quantity) in &observation.reasons {
        let outcome = if *reason == super::correlate::CoverageReason::AlreadyShortened {
            "already-shortened"
        } else {
            "unknown"
        };
        state.record(outcome, *quantity, Some(reason.as_str()), None, None, None);
    }
    if processing_failure.is_none()
        && state.route.mode == super::registry::ModelRouteMode::Shadow
        && !observation.exchanges.is_empty()
    {
        state.record(
            "held-whole",
            observation.exchanges.len(),
            Some("shadow-mode"),
            None,
            None,
            Some(local_processing_ms),
        );
    } else if processing_failure.is_none() {
        for (reason, quantity) in &prepared.reasons {
            if reason != "already-shortened" {
                state.record(
                    "held-whole",
                    *quantity,
                    Some(reason),
                    None,
                    None,
                    Some(local_processing_ms),
                );
            }
        }
    }
    (prepared, local_processing_ms)
}

fn accept_prepared(
    state: &RelayState,
    trims: &[crate::tool_result::PreparedTextTrim],
    latency_ms: u64,
    local_processing_ms: u64,
) {
    for trim in trims {
        match crate::tool_result::mark_text_trim_accepted(trim) {
            Ok(true) => state.record(
                "applied",
                1,
                Some("upstream-accepted"),
                Some(trim.character_receipt()),
                Some(latency_ms),
                Some(local_processing_ms),
            ),
            Ok(false) => {}
            Err(error) => {
                eprintln!("ctx model gateway could not record an accepted trim: {error}");
            }
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
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

fn forward_websocket_request_headers(source: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let connection_tokens = connection_tokens(source);
    for (name, value) in source {
        if !blocked_request_header(name, &connection_tokens)
            && (!name.as_str().starts_with("sec-websocket-")
                || name.as_str().eq_ignore_ascii_case("sec-websocket-protocol"))
            && !name.as_str().eq_ignore_ascii_case("origin")
        {
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
    use axum::routing::{get, post};
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
        websocket_messages: Arc<Mutex<Vec<String>>>,
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

    fn chatgpt_route() -> ModelRoute {
        ModelRoute {
            id: "codex-chatgpt-test".into(),
            surface: SurfaceId::Codex,
            protocol: WireProtocol::OpenAiResponses,
            authentication: AuthenticationMode::ChatGptLogin,
            upstream: ProviderTarget::OpenAiChatGpt,
            listen_port: 8873,
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

    fn testing_chatgpt_route() -> ModelRoute {
        let mut route = chatgpt_route();
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

    async fn capture_websocket(
        State(capture): State<Capture>,
        headers: HeaderMap,
        websocket: WebSocketUpgrade,
    ) -> Response<Body> {
        capture.headers.lock().unwrap().push(headers);
        let socket_capture = capture.clone();
        let mut response =
            websocket
                .protocols(["responses"])
                .on_upgrade(move |mut socket| async move {
                    while let Some(message) = socket.next().await {
                        let Ok(message) = message else { break };
                        match message {
                            AxumMessage::Text(text) => {
                                socket_capture.websocket_messages.lock().unwrap().push(text);
                                if socket
                                    .send(AxumMessage::Text(
                                        r#"{"type":"response.created"}"#.into(),
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                if socket
                                    .send(AxumMessage::Text(
                                        r#"{"type":"response.completed"}"#.into(),
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            AxumMessage::Binary(bytes) => {
                                if socket.send(AxumMessage::Binary(bytes)).await.is_err() {
                                    break;
                                }
                            }
                            AxumMessage::Ping(bytes) => {
                                if socket.send(AxumMessage::Pong(bytes)).await.is_err() {
                                    break;
                                }
                            }
                            AxumMessage::Pong(_) => {}
                            AxumMessage::Close(frame) => {
                                let _ = socket.send(AxumMessage::Close(frame)).await;
                                break;
                            }
                        }
                    }
                });
        response
            .headers_mut()
            .insert("x-reasoning-included", "true".parse().unwrap());
        response
            .headers_mut()
            .insert("openai-model", "gpt-test".parse().unwrap());
        response
    }

    async fn gateway_for(upstream: &str) -> (String, tokio::task::JoinHandle<()>) {
        gateway_for_route(upstream, codex_route()).await
    }

    async fn gateway_for_route(
        upstream: &str,
        route: ModelRoute,
    ) -> (String, tokio::task::JoinHandle<()>) {
        gateway_for_route_with_evidence(upstream, route, false).await
    }

    async fn gateway_for_route_with_evidence(
        upstream: &str,
        route: ModelRoute,
        evidence: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let mut state =
            RelayState::new(route, reqwest::Url::parse(upstream).unwrap(), test_client()).unwrap();
        // The 500 ms production budget is a latency guard, not the behaviour under test. A cold CI
        // runner (Windows in particular) can spend longer than that on the first transform, which
        // cancels it and turns every "did it trim" assertion into a coin flip. Tests that do
        // exercise the deadline set it themselves via with_test_processing_timing.
        state.processing_deadline = Duration::from_secs(30);
        if evidence {
            state = state.with_test_evidence();
        }
        let state = Arc::new(state);
        spawn(router(state)).await
    }

    #[tokio::test]
    async fn chatgpt_websocket_preserves_auth_frames_metadata_and_multi_turn_order() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .route("/backend-api/codex/responses", get(capture_websocket))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for_route(
            &format!("{upstream}/backend-api/codex/responses"),
            chatgpt_route(),
        )
        .await;
        let mut request = format!(
            "{}/backend-api/codex/responses?mode=responses",
            gateway.replacen("http://", "ws://", 1)
        )
        .into_client_request()
        .unwrap();
        request
            .headers_mut()
            .insert(AUTHORIZATION, "Bearer seeded-secret".parse().unwrap());
        request
            .headers_mut()
            .insert("chatgpt-account-id", "acct-test".parse().unwrap());
        request
            .headers_mut()
            .insert("x-forwarded-host", "evil.example".parse().unwrap());
        request
            .headers_mut()
            .insert("x-ctx-route", "other-route".parse().unwrap());
        request
            .headers_mut()
            .insert("sec-websocket-protocol", "responses".parse().unwrap());

        let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
        assert_eq!(response.headers()["sec-websocket-protocol"], "responses");
        assert_eq!(response.headers()["x-reasoning-included"], "true");
        assert_eq!(response.headers()["openai-model"], "gpt-test");

        let mut sent = Vec::new();
        for turn in 1..=2 {
            let request = serde_json::json!({
                "type": "response.create",
                "model": "gpt-test",
                "input": [{"role": "user", "content": format!("turn-{turn}")}]
            })
            .to_string();
            sent.push(request.clone());
            socket
                .send(TungsteniteMessage::Text(request))
                .await
                .unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                TungsteniteMessage::Text(r#"{"type":"response.created"}"#.into())
            );
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                TungsteniteMessage::Text(r#"{"type":"response.completed"}"#.into())
            );
        }
        socket
            .send(TungsteniteMessage::Binary(vec![0, 1, 2, 255]))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            TungsteniteMessage::Binary(vec![0, 1, 2, 255])
        );
        socket.close(None).await.unwrap();

        assert_eq!(*capture.websocket_messages.lock().unwrap(), sent);
        let headers = capture.headers.lock().unwrap();
        assert_eq!(headers[0][AUTHORIZATION], "Bearer seeded-secret");
        assert_eq!(headers[0]["chatgpt-account-id"], "acct-test");
        assert!(headers[0].get("x-forwarded-host").is_none());
        assert!(headers[0].get("x-ctx-route").is_none());
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn chatgpt_models_metadata_is_fixed_get_only_and_preserves_auth_and_query() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .route("/backend-api/codex/models", get(capture_request))
            .fallback(any(capture_request))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for_route(
            &format!("{upstream}/backend-api/codex/responses"),
            chatgpt_route(),
        )
        .await;

        let response = test_client()
            .get(format!(
                "{gateway}/backend-api/codex/models?client_version=0.145.0"
            ))
            .header(AUTHORIZATION, "Bearer seeded-secret")
            .header("chatgpt-account-id", "acct-test")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            capture.uris.lock().unwrap().as_slice(),
            &["/backend-api/codex/models?client_version=0.145.0"]
        );
        {
            let headers = capture.headers.lock().unwrap();
            assert_eq!(headers[0][AUTHORIZATION], "Bearer seeded-secret");
            assert_eq!(headers[0]["chatgpt-account-id"], "acct-test");
        }

        let wrong_method = test_client()
            .post(format!("{gateway}/backend-api/codex/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_method.status(), StatusCode::NOT_FOUND);
        let held_endpoint = test_client()
            .get(format!("{gateway}/backend-api/codex/realtime/calls"))
            .send()
            .await
            .unwrap();
        assert_eq!(held_endpoint.status(), StatusCode::NOT_FOUND);
        assert_eq!(capture.uris.lock().unwrap().len(), 1);
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn chatgpt_websocket_rejects_browser_origin_before_upstream() {
        let capture = Capture::default();
        let upstream_app = Router::new()
            .route("/backend-api/codex/responses", get(capture_websocket))
            .with_state(capture.clone());
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for_route(
            &format!("{upstream}/backend-api/codex/responses"),
            chatgpt_route(),
        )
        .await;

        let response = test_client()
            .get(format!("{gateway}/backend-api/codex/responses"))
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("origin", "https://attacker.example")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(capture.headers.lock().unwrap().is_empty());
        upstream_task.abort();
        gateway_task.abort();
    }

    #[tokio::test]
    async fn websocket_upgrade_rejection_preserves_status_but_not_provider_body() {
        let upstream_app = Router::new().route(
            "/backend-api/codex/responses",
            get(|| async {
                let mut response = Response::new(Body::from("seeded-provider-secret"));
                *response.status_mut() = StatusCode::UNAUTHORIZED;
                response
            }),
        );
        let (upstream, upstream_task) = spawn(upstream_app).await;
        let (gateway, gateway_task) = gateway_for_route(
            &format!("{upstream}/backend-api/codex/responses"),
            chatgpt_route(),
        )
        .await;
        let url = format!(
            "{}/backend-api/codex/responses",
            gateway.replacen("http://", "ws://", 1)
        );

        let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("expected an HTTP WebSocket rejection");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response.body().as_deref().unwrap_or_default();
        assert!(!String::from_utf8_lossy(body).contains("seeded-provider-secret"));
        assert!(String::from_utf8_lossy(body).contains("websocket-upgrade-rejected"));
        upstream_task.abort();
        gateway_task.abort();
    }

    #[test]
    fn chatgpt_websocket_testing_route_applies_only_after_provider_acceptance() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _home = ScopedCtxHome::set(temp.path());
        runtime.block_on(async {
            let capture = Capture::default();
            let upstream_app = Router::new()
                .route("/backend-api/codex/responses", get(capture_websocket))
                .with_state(capture.clone());
            let (upstream, upstream_task) = spawn(upstream_app).await;
            let (gateway, gateway_task) = gateway_for_route_with_evidence(
                &format!("{upstream}/backend-api/codex/responses"),
                testing_chatgpt_route(),
                true,
            )
            .await;
            let url = format!(
                "{}/backend-api/codex/responses",
                gateway.replacen("http://", "ws://", 1)
            );
            let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
            let mut body: Value = serde_json::from_slice(&synthetic_responses_body("websocket"))
                .expect("synthetic Responses request");
            body.as_object_mut()
                .unwrap()
                .insert("type".into(), Value::String("response.create".into()));

            let original = body.to_string();
            socket
                .send(TungsteniteMessage::Text(original.clone()))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap();
            assert_eq!(applied_count(), 1);
            socket.next().await.unwrap().unwrap();
            socket.close(None).await.unwrap();

            let forwarded = capture.websocket_messages.lock().unwrap()[0].clone();
            assert_ne!(forwarded, original);
            assert!(forwarded.contains("ctx trimmed this output"));
            let conn = crate::db::open_db().unwrap();
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].accepted, 1);
            assert_eq!(evidence[0].applied, 1);
            let rewind_id: String = conn
                .query_row(
                    "SELECT rewind_id FROM compress_decisions WHERE applied=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let rewind = crate::db::get_rewind(&conn, &rewind_id).unwrap();
            assert!(rewind.original.contains("websocket synthetic line 99"));
            upstream_task.abort();
            gateway_task.abort();
        });
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
                Some("codex-test 1.0".into()),
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
        assert_eq!(receipt["clientVersion"], "codex-test 1.0");
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
            let (gateway, gateway_task) = gateway_for_route_with_evidence(
                &format!("{upstream}/v1/responses"),
                testing_codex_route(),
                true,
            )
            .await;
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
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence.len(), 1);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].accepted, 1);
            assert_eq!(evidence[0].applied, 1);
            assert!(evidence[0].chars_saved > 0);
            assert!(evidence[0].p95_local_processing_ms.is_some());
            let integrity = crate::db::model_gateway_integrity(&conn);
            assert_eq!(integrity[0].applied_decisions, 1);
            assert_eq!(integrity[0].applied_without_recovery, 0);
            let recovery = crate::db::model_gateway_recovery_summary(&conn);
            assert_eq!(recovery.prepared_recovery_copies, 1);
            assert_eq!(recovery.unapplied_recovery_copies, 0);
            let raw: String = conn
                .query_row(
                    "SELECT GROUP_CONCAT(COALESCE(reason_code,''), ',') FROM model_gateway_events",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!raw.contains("accepted synthetic line"));
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
            let (gateway, gateway_task) = gateway_for_route_with_evidence(
                &format!("{upstream}/v1/responses"),
                testing_codex_route(),
                true,
            )
            .await;
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
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].provider_rejected, 1);
            assert_eq!(evidence[0].accepted, 0);
            assert_eq!(evidence[0].applied, 0);
            let recovery = crate::db::model_gateway_recovery_summary(&conn);
            assert_eq!(recovery.prepared_recovery_copies, 1);
            assert_eq!(recovery.unapplied_recovery_copies, 1);
            upstream_task.abort();
            gateway_task.abort();
        });
    }

    #[test]
    fn transform_deadline_sends_the_exact_original_and_never_counts_applied() {
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
            let state = RelayState::new(
                testing_codex_route(),
                reqwest::Url::parse(&format!("{upstream}/v1/responses")).unwrap(),
                test_client(),
            )
            .unwrap()
            .with_test_evidence()
            .with_test_processing_timing(Duration::from_millis(40), Duration::from_millis(5));
            let (gateway, gateway_task) = spawn(router(Arc::new(state))).await;
            let original = synthetic_responses_body("deadline");

            let response = test_client()
                .post(format!("{gateway}/v1/responses"))
                .body(original.clone())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.bytes().await.unwrap().as_ref(), original);
            assert_eq!(capture.bodies.lock().unwrap().as_slice(), &[original]);
            assert_eq!(applied_count(), 0);

            // The timed-out blocking worker is detached but cooperatively cancelled before it can
            // begin observation or durable preparation.
            tokio::time::sleep(Duration::from_millis(60)).await;
            let conn = crate::db::open_db().unwrap();
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].accepted, 1);
            assert_eq!(evidence[0].applied, 0);
            assert_eq!(evidence[0].held_whole, 1);
            assert_eq!(evidence[0].transform_deadlines, 1);
            assert_eq!(evidence[0].processing_failures, 0);
            assert!(evidence[0]
                .recent_reason_codes
                .contains(&"transform-deadline".to_string()));
            let recovery = crate::db::model_gateway_recovery_summary(&conn);
            assert_eq!(recovery.prepared_recovery_copies, 0);
            assert_eq!(recovery.unapplied_recovery_copies, 0);
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
            let (gateway, gateway_task) = gateway_for_route_with_evidence(
                &format!("http://{unavailable}/v1/responses"),
                testing_codex_route(),
                true,
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
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].transport_failures, 1);
            assert_eq!(evidence[0].accepted, 0);
            assert_eq!(evidence[0].applied, 0);
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
            let (gateway, gateway_task) = gateway_for_route_with_evidence(
                &format!("{upstream}/v1/responses"),
                testing_codex_route(),
                true,
            )
            .await;
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
            let conn = crate::db::open_db().unwrap();
            let evidence = crate::db::model_gateway_route_summaries(&conn);
            assert_eq!(evidence[0].attempted, 1);
            assert_eq!(evidence[0].accepted, 1);
            assert_eq!(evidence[0].applied, 1);
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
