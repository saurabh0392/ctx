//! Recoverable M4 ownership transactions for model-gateway client routes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::registry::{ModelRoute, RouteRegistry};
use super::surfaces::{FieldState, OwnedConfigField};

const OWNERSHIP_VERSION: u32 = 1;
const MAX_OWNERSHIP_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecyclePhase {
    Prepared,
    Enabled,
    Bypassed,
}

impl LifecyclePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Enabled => "enabled",
            Self::Bypassed => "bypassed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipReceipt {
    schema_version: u32,
    route: ModelRoute,
    phase: LifecyclePhase,
    config: OwnedConfigField,
    health_nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_version: Option<String>,
    prepared_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bypassed_at: Option<String>,
    credentials_persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipRegistry {
    version: u32,
    routes: BTreeMap<String, OwnershipReceipt>,
}

impl Default for OwnershipRegistry {
    fn default() -> Self {
        Self {
            version: OWNERSHIP_VERSION,
            routes: BTreeMap::new(),
        }
    }
}

impl OwnershipRegistry {
    fn load() -> Result<Self> {
        let path = ownership_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let metadata = std::fs::metadata(&path)?;
        if metadata.len() > MAX_OWNERSHIP_BYTES {
            anyhow::bail!("model gateway ownership registry exceeds its 1 MiB limit");
        }
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read model gateway ownership {}", path.display()))?;
        let registry: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse model gateway ownership {}", path.display()))?;
        registry.validate()?;
        Ok(registry)
    }

    fn validate(&self) -> Result<()> {
        if self.version != OWNERSHIP_VERSION {
            anyhow::bail!(
                "unsupported model gateway ownership version {}",
                self.version
            );
        }
        for (id, receipt) in &self.routes {
            if id != &receipt.route.id || receipt.schema_version != OWNERSHIP_VERSION {
                anyhow::bail!("model gateway ownership identity mismatch");
            }
            receipt.route.validate()?;
            super::supervisor::validate_nonce_for_lifecycle(&receipt.health_nonce)?;
            if receipt.credentials_persisted {
                anyhow::bail!(
                    "invalid model gateway ownership receipt claims persisted credentials"
                );
            }
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        self.validate()?;
        crate::config::ensure_dir()?;
        let path = ownership_path();
        let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        crate::config::protect_private_file(&temporary)?;
        std::fs::rename(&temporary, &path)?;
        crate::config::protect_private_file(&path)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ServiceState {
    Healthy,
    NotRunning,
    IdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RegisteredRouteState {
    Matching,
    RegisteredOnly,
    Missing,
    Modified,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RouteStatus {
    pub(crate) route_id: String,
    pub(crate) surface: String,
    pub(crate) phase: String,
    pub(crate) mode: String,
    pub(crate) authentication: String,
    pub(crate) protocol: String,
    pub(crate) fixed_upstream: String,
    pub(crate) local_base_url: String,
    pub(crate) config_location: Option<String>,
    pub(crate) config_state: FieldState,
    pub(crate) service_state: ServiceState,
    pub(crate) registered_route_state: RegisteredRouteState,
    pub(crate) client_version: Option<String>,
    pub(crate) credentials_persisted: bool,
    pub(crate) cursor_model_path_available: bool,
    pub(crate) process_visibility: &'static str,
    pub(crate) retained_locally: &'static str,
    pub(crate) cloud_relay: bool,
    pub(crate) controlled_path: &'static str,
    pub(crate) unavailable_path: &'static str,
    pub(crate) cache_accounting: &'static str,
    pub(crate) recovery_command: &'static str,
    pub(crate) purge_control: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bypass_command: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthReceipt {
    status: String,
    route_id: String,
    surface: String,
    protocol: String,
    authentication: String,
    fixed_upstream: String,
    instance_nonce: Option<String>,
}

pub async fn enable(route_id: &str, consent: bool) -> Result<()> {
    if !consent {
        anyhow::bail!(
            "model-path enablement requires --yes after reviewing that CTX will see prompts, tool data, source content, and authorization headers in memory and forward them to the fixed provider"
        );
    }
    let route = RouteRegistry::load()?
        .routes
        .get(route_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("model route {route_id:?} is not registered"))?;
    route.validate()?;
    let home = home()?;
    let mut ownership = OwnershipRegistry::load()?;

    let mut receipt = if let Some(existing) = ownership.routes.get(route_id).cloned() {
        if existing.route != route {
            anyhow::bail!(
                "registered route differs from CTX's ownership receipt; no state was changed"
            );
        }
        match existing.phase {
            LifecyclePhase::Enabled => {
                anyhow::bail!("model route {route_id:?} is already enabled")
            }
            LifecyclePhase::Prepared => anyhow::bail!(
                "model route {route_id:?} has an incomplete prepared transaction; run doctor, then bypass before retrying"
            ),
            LifecyclePhase::Bypassed => {
                if super::surfaces::inspect(&route, &home, &existing.config) != FieldState::Original {
                    anyhow::bail!("client route changed after bypass; CTX preserved it and will not re-enable destructively");
                }
                existing
            }
        }
    } else {
        let probe = super::probe::probe(route.surface, true);
        if !probe.installed {
            anyhow::bail!(
                "{} is not installed; no route or client configuration was changed",
                route.surface.as_str()
            );
        }
        let config = super::surfaces::prepare(&route, &home)?;
        OwnershipReceipt {
            schema_version: OWNERSHIP_VERSION,
            route: route.clone(),
            phase: LifecyclePhase::Prepared,
            config,
            health_nonce: new_nonce(),
            client_version: probe.client_version,
            prepared_at: now(),
            enabled_at: None,
            bypassed_at: None,
            credentials_persisted: false,
        }
    };

    receipt.phase = LifecyclePhase::Prepared;
    ownership
        .routes
        .insert(route_id.to_owned(), receipt.clone());
    ownership.save()?;

    if let Err(error) = start_and_verify(&route, &receipt.health_nonce).await {
        return rollback_prepared(route_id, error, &mut ownership);
    }
    if let Err(error) = super::surfaces::apply(&route, &home, &receipt.config) {
        return rollback_prepared(route_id, error, &mut ownership);
    }

    receipt.phase = LifecyclePhase::Enabled;
    receipt.enabled_at = Some(now());
    receipt.bypassed_at = None;
    ownership.routes.insert(route_id.to_owned(), receipt);
    ownership.save()?;

    println!(
        "Enabled {} model-path route {route_id:?}.",
        route.surface.as_str()
    );
    println!("  client route: {}", route.local_base_url());
    println!("  fixed destination: {}", route.upstream.origin());
    println!("  mode: {}", route.mode.as_str());
    println!("  CTX cloud relay: no");
    println!("  credentials persisted: no");
    println!("Bypass immediately: ctx model-gateway bypass {route_id}");
    Ok(())
}

fn rollback_prepared(
    route_id: &str,
    original_error: anyhow::Error,
    ownership: &mut OwnershipRegistry,
) -> Result<()> {
    match super::supervisor::uninstall(route_id) {
        Ok(()) => {
            ownership.routes.remove(route_id);
            ownership.save()?;
            Err(original_error)
        }
        Err(cleanup_error) => anyhow::bail!(
            "{original_error}; service cleanup also failed ({cleanup_error}); CTX retained the prepared ownership receipt for doctor and bypass"
        ),
    }
}

pub fn bypass(route_id: &str) -> Result<()> {
    let home = home()?;
    let mut ownership = OwnershipRegistry::load()?;
    let mut receipt =
        ownership.routes.get(route_id).cloned().ok_or_else(|| {
            anyhow::anyhow!("model route {route_id:?} has no CTX client ownership")
        })?;
    let state = super::surfaces::inspect(&receipt.route, &home, &receipt.config);
    match state {
        FieldState::CtxOwned => super::surfaces::restore(&receipt.route, &home, &receipt.config)?,
        FieldState::Original if receipt.phase != LifecyclePhase::Enabled => {}
        FieldState::UserModified => anyhow::bail!(
            "client model route was modified after CTX activation; CTX preserved the user value and left ownership state for doctor"
        ),
        other => anyhow::bail!("cannot safely bypass route {route_id:?}: client config is {other:?}"),
    }
    receipt.phase = LifecyclePhase::Bypassed;
    receipt.bypassed_at = Some(now());
    ownership.routes.insert(route_id.to_owned(), receipt);
    ownership.save()?;
    record_owned_event(
        ownership.routes.get(route_id).expect("bypassed receipt"),
        "bypassed",
        "user-bypass",
    );
    println!("Bypassed model route {route_id:?}; the client's prior route is restored.");
    println!("The local gateway remains installed for a fast, explicit re-enable.");
    Ok(())
}

pub fn disable(route_id: &str) -> Result<()> {
    let ownership = OwnershipRegistry::load()?;
    let receipt =
        ownership.routes.get(route_id).cloned().ok_or_else(|| {
            anyhow::anyhow!("model route {route_id:?} has no CTX client ownership")
        })?;
    if receipt.phase != LifecyclePhase::Bypassed {
        bypass(route_id)?;
    }
    super::supervisor::uninstall(route_id)?;
    super::registry::remove_exact(&receipt.route)?;
    let mut ownership = OwnershipRegistry::load()?;
    ownership.routes.remove(route_id);
    ownership.save()?;
    println!(
        "Disabled model route {route_id:?}; client, service, and CTX route state are restored."
    );
    Ok(())
}

pub async fn print_status(route_id: Option<&str>, json: bool) -> Result<()> {
    let statuses = collect_status(route_id).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }
    if statuses.is_empty() {
        println!("No model gateway routes are registered or owned.");
        println!("Cursor model-path trimming: unavailable until a documented route is captured.");
        return Ok(());
    }
    for status in statuses {
        println!(
            "{} · {} · {} · {:?}",
            status.surface, status.authentication, status.protocol, status.service_state
        );
        println!("  lifecycle: {} ({})", status.phase, status.mode);
        println!("  client config: {:?}", status.config_state);
        println!("  route registry: {:?}", status.registered_route_state);
        println!("  fixed destination: {}", status.fixed_upstream);
        println!("  credentials persisted: no");
        if let Some(command) = status.bypass_command {
            println!("  bypass: {command}");
        }
    }
    Ok(())
}

pub async fn print_doctor(route_id: Option<&str>, json: bool) -> Result<()> {
    let statuses = collect_status(route_id).await?;
    let healthy = !statuses.is_empty()
        && statuses.iter().all(|status| match status.phase.as_str() {
            "enabled" => {
                status.config_state == FieldState::CtxOwned
                    && status.service_state == ServiceState::Healthy
                    && status.registered_route_state == RegisteredRouteState::Matching
            }
            "bypassed" => {
                status.config_state == FieldState::Original
                    && status.registered_route_state == RegisteredRouteState::Matching
            }
            _ => false,
        });
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "healthy": healthy,
                "routes": statuses,
                "credentialsPersisted": false,
                "cursorModelPathAvailable": false
            }))?
        );
    } else {
        println!(
            "Model gateway doctor: {}",
            if healthy {
                "healthy"
            } else {
                "attention needed"
            }
        );
        for status in statuses {
            println!(
                "  {}: phase={} config={:?} service={:?} registry={:?} destination={}",
                status.route_id,
                status.phase,
                status.config_state,
                status.service_state,
                status.registered_route_state,
                status.fixed_upstream
            );
        }
        println!("  Cursor: unavailable (no verified programmable model route)");
    }
    Ok(())
}

pub fn is_owned(route_id: &str) -> Result<bool> {
    Ok(OwnershipRegistry::load()?.routes.contains_key(route_id))
}

pub(super) fn client_version_for_route(route_id: &str) -> Option<String> {
    OwnershipRegistry::load()
        .ok()?
        .routes
        .get(route_id)
        .and_then(|receipt| receipt.client_version.clone())
}

pub(crate) async fn dashboard_status() -> Result<Vec<RouteStatus>> {
    collect_status(None).await
}

pub fn disable_all_for_uninstall() -> Result<()> {
    let ids = OwnershipRegistry::load()?
        .routes
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for id in ids {
        disable(&id)?;
    }
    Ok(())
}

pub async fn refresh_owned_services() -> Result<()> {
    let home = home()?;
    let mut ownership = OwnershipRegistry::load()?;
    let ids = ownership.routes.keys().cloned().collect::<Vec<_>>();
    for id in ids {
        let Some(mut receipt) = ownership.routes.get(&id).cloned() else {
            continue;
        };
        if receipt.phase == LifecyclePhase::Prepared {
            anyhow::bail!(
                "model route {id:?} has an incomplete prepared transaction; run `ctx model-gateway doctor {id}` and bypass it before reinstall"
            );
        }
        let refreshed = super::supervisor::install(&receipt.route, &receipt.health_nonce);
        let verified = if refreshed.is_ok() {
            wait_for_service(&receipt.route, &receipt.health_nonce).await
        } else {
            refreshed
        };
        if let Err(error) = verified {
            if receipt.phase == LifecyclePhase::Enabled
                && super::surfaces::inspect(&receipt.route, &home, &receipt.config)
                    == FieldState::CtxOwned
            {
                super::surfaces::restore(&receipt.route, &home, &receipt.config)?;
                receipt.phase = LifecyclePhase::Bypassed;
                receipt.bypassed_at = Some(now());
                ownership.routes.insert(id.clone(), receipt);
                ownership.save()?;
            }
            let _ = super::supervisor::uninstall(&id);
            return Err(error).with_context(|| {
                format!(
                    "refresh model route {id:?}; the prior client route was restored when CTX owned it"
                )
            });
        }
    }
    Ok(())
}

async fn collect_status(route_id: Option<&str>) -> Result<Vec<RouteStatus>> {
    let registry = RouteRegistry::load()?;
    let ownership = OwnershipRegistry::load()?;
    let mut ids = BTreeSet::new();
    if let Some(route_id) = route_id {
        ids.insert(route_id.to_owned());
    } else {
        ids.extend(registry.routes.keys().cloned());
        ids.extend(ownership.routes.keys().cloned());
    }
    let home = home()?;
    let mut statuses = Vec::new();
    for id in ids {
        let owned = ownership.routes.get(&id);
        let route = owned
            .map(|receipt| &receipt.route)
            .or_else(|| registry.routes.get(&id))
            .ok_or_else(|| anyhow::anyhow!("model route {id:?} is not registered or owned"))?;
        let (phase, config_location, config_state, client_version, service_state) = match owned {
            Some(receipt) => (
                receipt.phase.as_str().to_owned(),
                Some(receipt.config.location.clone()),
                super::surfaces::inspect(route, &home, &receipt.config),
                receipt.client_version.clone(),
                inspect_service(route, Some(&receipt.health_nonce)).await,
            ),
            None => (
                "registered".into(),
                None,
                FieldState::NotOwned,
                None,
                inspect_service(route, None).await,
            ),
        };
        let registered_route_state = match (owned, registry.routes.get(&id)) {
            (Some(receipt), Some(registered)) if registered == &receipt.route => {
                RegisteredRouteState::Matching
            }
            (Some(_), Some(_)) => RegisteredRouteState::Modified,
            (Some(_), None) => RegisteredRouteState::Missing,
            (None, Some(_)) => RegisteredRouteState::RegisteredOnly,
            (None, None) => RegisteredRouteState::Missing,
        };
        statuses.push(RouteStatus {
            route_id: id.clone(),
            surface: route.surface.as_str().into(),
            phase,
            mode: route.mode.as_str().into(),
            authentication: route.authentication.as_str().into(),
            protocol: route.protocol.as_str().into(),
            fixed_upstream: route.upstream.origin().into(),
            local_base_url: route.local_base_url(),
            config_location,
            config_state,
            service_state,
            registered_route_state,
            client_version,
            credentials_persisted: false,
            cursor_model_path_available: false,
            process_visibility: "prompts, instructions, tool definitions and results, source content, and authorization headers in memory while forwarding",
            retained_locally: "content-free route receipts, plus exact originals when a trim is prepared before send; only provider-accepted trims count as applied",
            cloud_relay: false,
            controlled_path: match route.surface {
                crate::surface::SurfaceId::Codex => "local tool results present in OpenAI Responses requests sent through this exact route",
                crate::surface::SurfaceId::ClaudeCode => "client-side tool results present in Anthropic Messages requests sent through this exact route",
                crate::surface::SurfaceId::Cursor => "none; Cursor has no verified programmable model route",
            },
            unavailable_path: match route.surface {
                crate::surface::SurfaceId::Codex => "OpenAI-hosted tools, direct routes, ChatGPT-login routing, and WebSocket traffic",
                crate::surface::SurfaceId::ClaudeCode => "Anthropic-hosted or provider-managed tools and traffic not sent through this route",
                crate::surface::SurfaceId::Cursor => "all model traffic until a supported route is documented and captured",
            },
            cache_accounting: "not yet measured; character savings are not a cache-adjusted cost claim",
            recovery_command: "ctx expand <rewind-id>",
            purge_control: "Settings > Privacy and data > Purge originals",
            bypass_command: owned.map(|_| format!("ctx model-gateway bypass {id}")),
        });
    }
    Ok(statuses)
}

fn record_owned_event(receipt: &OwnershipReceipt, outcome: &'static str, reason: &'static str) {
    crate::db::record_model_gateway_event_best_effort(&crate::db::ModelGatewayEvent {
        route_id: &receipt.route.id,
        surface: receipt.route.surface.as_str(),
        surface_version: receipt.client_version.as_deref(),
        protocol: receipt.route.protocol.as_str(),
        authentication: receipt.route.authentication.as_str(),
        fixed_upstream: receipt.route.upstream.origin(),
        mode: receipt.route.mode.as_str(),
        outcome,
        quantity: 1,
        reason_code: Some(reason),
        chars_in: None,
        chars_out: None,
        latency_ms: None,
        local_processing_ms: None,
    });
}

async fn start_and_verify(route: &ModelRoute, nonce: &str) -> Result<()> {
    super::supervisor::install(route, nonce)?;
    wait_for_service(route, nonce).await
}

async fn wait_for_service(route: &ModelRoute, nonce: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if inspect_service(route, Some(nonce)).await == ServiceState::Healthy {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "model gateway service did not produce the exact route health receipt within 5 seconds; no client configuration was changed"
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn inspect_service(route: &ModelRoute, nonce: Option<&str>) -> ServiceState {
    let client = match reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_millis(600))
        .build()
    {
        Ok(client) => client,
        Err(_) => return ServiceState::NotRunning,
    };
    let url = format!("http://{}/__ctx/health", route.listen_address());
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return ServiceState::NotRunning,
    };
    let health = match response.json::<HealthReceipt>().await {
        Ok(health) => health,
        Err(_) => return ServiceState::IdentityMismatch,
    };
    let exact = health.status == "listener-ready"
        && health.route_id == route.id
        && health.surface == route.surface.as_str()
        && health.protocol == route.protocol.as_str()
        && health.authentication == route.authentication.as_str()
        && health.fixed_upstream == route.upstream.origin()
        && nonce.is_none_or(|nonce| health.instance_nonce.as_deref() == Some(nonce));
    if exact {
        ServiceState::Healthy
    } else {
        ServiceState::IdentityMismatch
    }
}

fn ownership_path() -> PathBuf {
    crate::config::ctx_dir().join("model-gateway-ownership.json")
}

fn home() -> Result<PathBuf> {
    crate::config::home_dir_for_paths()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve user home for model gateway lifecycle"))
}

fn new_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::registry::{ModelRouteMode, ProviderTarget};
    use crate::model_gateway::route::{AuthenticationMode, WireProtocol};
    use crate::surface::SurfaceId;

    fn codex_route() -> ModelRoute {
        ModelRoute {
            id: "codex-api".into(),
            surface: SurfaceId::Codex,
            protocol: WireProtocol::OpenAiResponses,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::OpenAi,
            listen_port: 18871,
            mode: ModelRouteMode::Shadow,
        }
    }

    fn claude_route() -> ModelRoute {
        ModelRoute {
            id: "claude-api".into(),
            surface: SurfaceId::ClaudeCode,
            protocol: WireProtocol::AnthropicMessages,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::Anthropic,
            listen_port: 18872,
            mode: ModelRouteMode::Shadow,
        }
    }

    #[test]
    fn codex_transaction_preserves_comments_and_refuses_user_edit() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join(".codex/config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "# keep this\nmodel = \"gpt-5\"\nopenai_base_url = \"https://api.openai.com/v1\"\n",
        )
        .unwrap();
        let route = codex_route();
        let field = super::super::surfaces::prepare(&route, temp.path()).unwrap();
        super::super::surfaces::apply(&route, temp.path(), &field).unwrap();
        let enabled = std::fs::read_to_string(&config).unwrap();
        assert!(enabled.contains("# keep this"));
        assert!(enabled.contains("model = \"gpt-5\""));
        assert!(enabled.contains("http://127.0.0.1:18871/v1"));

        super::super::surfaces::restore(&route, temp.path(), &field).unwrap();
        let restored = std::fs::read_to_string(&config).unwrap();
        assert!(restored.contains("# keep this"));
        assert!(restored.contains("model = \"gpt-5\""));
        assert!(restored.contains("https://api.openai.com/v1"));
        assert!(!restored.contains("ctx-model-gateway"));

        super::super::surfaces::apply(&route, temp.path(), &field).unwrap();
        let enabled = std::fs::read_to_string(&config).unwrap();

        let changed = enabled.replace("http://127.0.0.1:18871/v1", "https://example.test/v1");
        std::fs::write(&config, changed).unwrap();
        assert!(super::super::surfaces::restore(&route, temp.path(), &field).is_err());
        assert!(std::fs::read_to_string(&config)
            .unwrap()
            .contains("https://example.test/v1"));
    }

    #[test]
    fn claude_transaction_preserves_unrelated_settings_and_auth_without_storing_it() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join(".claude/settings.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            r#"{"env":{"ANTHROPIC_API_KEY":"seeded-secret"},"hooks":{"keep":true}}"#,
        )
        .unwrap();
        let route = claude_route();
        let field = super::super::surfaces::prepare(&route, temp.path()).unwrap();
        let receipt = serde_json::to_string(&field).unwrap();
        assert!(!receipt.contains("seeded-secret"));
        super::super::surfaces::apply(&route, temp.path(), &field).unwrap();
        let enabled: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(enabled["hooks"]["keep"], true);
        assert_eq!(enabled["env"]["ANTHROPIC_API_KEY"], "seeded-secret");
        super::super::surfaces::restore(&route, temp.path(), &field).unwrap();
        let restored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(restored["hooks"]["keep"], true);
        assert_eq!(restored["env"]["ANTHROPIC_API_KEY"], "seeded-secret");
        assert!(restored["env"].get("ANTHROPIC_BASE_URL").is_none());
    }

    #[test]
    fn ownership_receipt_contains_no_seeded_secret_or_whole_config() {
        let route = codex_route();
        let receipt = OwnershipReceipt {
            schema_version: OWNERSHIP_VERSION,
            route,
            phase: LifecyclePhase::Prepared,
            config: OwnedConfigField {
                strategy: "codex-ctx-provider-http-sse-v1".into(),
                location: "~/.codex/config.toml:model_provider+model_providers.ctx-model-gateway"
                    .into(),
                config_existed: true,
                original_value: Some("openai".into()),
                ctx_value: "ctx-model-gateway".into(),
            },
            health_nonce: "00112233445566778899aabbccddeeff".into(),
            client_version: Some("codex 1.0".into()),
            prepared_at: now(),
            enabled_at: None,
            bypassed_at: None,
            credentials_persisted: false,
        };
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(!encoded.contains("seeded-secret"));
        assert!(!encoded.to_ascii_lowercase().contains("authorization"));
        assert!(!encoded.to_ascii_lowercase().contains("api_key"));
    }
}
