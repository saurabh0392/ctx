//! Real-time dashboard notifications via SSE (broadcast on hook events + ingest).

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DashboardEvent {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_type: Option<String>,
}

fn bus() -> &'static broadcast::Sender<DashboardEvent> {
    static BUS: OnceLock<broadcast::Sender<DashboardEvent>> = OnceLock::new();
    BUS.get_or_init(|| {
        let (tx, _) = broadcast::channel(256);
        tx
    })
}

/// Broadcast to all connected SSE clients (no-op if none connected).
pub fn notify(event: DashboardEvent) {
    let _ = bus().send(event);
}

pub fn subscribe() -> broadcast::Receiver<DashboardEvent> {
    bus().subscribe()
}

pub async fn api_events_stream() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                    return Some((Ok(Event::default().event("dashboard").data(data)), rx));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    let connected = stream::once(async { Ok(Event::default().event("connected").data("{}")) });

    let merged = stream::select(connected, stream);
    Sse::new(merged).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

#[derive(Deserialize)]
pub struct PushBody {
    pub kind: String,
    pub hook_type: Option<String>,
}

pub async fn api_dashboard_push(axum::Json(body): axum::Json<PushBody>) -> axum::http::StatusCode {
    notify(DashboardEvent {
        kind: body.kind,
        hook_type: body.hook_type,
    });
    axum::http::StatusCode::NO_CONTENT
}
