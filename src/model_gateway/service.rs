use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use super::registry::RouteRegistry;
use super::relay::{self, RelayState};

pub async fn serve(route_id: &str, health_nonce: Option<&str>) -> Result<()> {
    let registry = RouteRegistry::load()?;
    let route = registry
        .routes
        .get(route_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("model route {route_id:?} is not registered"))?;
    route.validate()?;

    let upstream = route.upstream_url()?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("build model gateway upstream client")?;
    let state = Arc::new(RelayState::new_with_health_nonce(
        route.clone(),
        upstream,
        client,
        health_nonce.map(str::to_owned),
        super::lifecycle::client_version_for_route(route_id),
    )?);
    let app = relay::router(state);
    let listener = tokio::net::TcpListener::bind(route.listen_address())
        .await
        .with_context(|| {
            format!(
                "bind model route {} on {}",
                route.id,
                route.listen_address()
            )
        })?;

    eprintln!("ctx model gateway route receipt:");
    eprintln!("  route: {}", route.id);
    eprintln!("  listen: {}", route.local_base_url());
    eprintln!("  accepted path: {}", route.endpoint_path());
    eprintln!("  fixed upstream: {}", route.upstream.origin());
    eprintln!("  mode: {}", route.mode.as_str());
    eprintln!(
        "  transformations: {}",
        match route.mode {
            super::registry::ModelRouteMode::Shadow => "off",
            super::registry::ModelRouteMode::Testing => "narrow testing contracts only",
        }
    );
    eprintln!("  credentials persisted by CTX: no");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve local model gateway")
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("ctx model gateway could not install shutdown signal: {error}");
    }
}
