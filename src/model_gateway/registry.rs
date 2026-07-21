//! Immutable, credential-free route registry for the transformation-off M1 runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model_gateway::route::{AuthenticationMode, WireProtocol};
use crate::surface::SurfaceId;

const REGISTRY_VERSION: u32 = 2;
const MIN_UNPRIVILEGED_PORT: u16 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderTarget {
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelRouteMode {
    #[default]
    Shadow,
    Testing,
}

impl ModelRouteMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "shadow" => Some(Self::Shadow),
            "testing" => Some(Self::Testing),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::Testing => "testing",
        }
    }
}

impl ProviderTarget {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai" | "open-ai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn origin(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com",
            Self::Anthropic => "https://api.anthropic.com",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub id: String,
    pub surface: SurfaceId,
    pub protocol: WireProtocol,
    pub authentication: AuthenticationMode,
    pub upstream: ProviderTarget,
    pub listen_port: u16,
    #[serde(default)]
    pub mode: ModelRouteMode,
}

impl ModelRoute {
    pub fn listen_address(&self) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.listen_port)
    }

    pub fn endpoint_path(&self) -> &'static str {
        match self.protocol {
            WireProtocol::OpenAiResponses => "/v1/responses",
            WireProtocol::OpenAiChatCompletions => "/v1/chat/completions",
            WireProtocol::AnthropicMessages => "/v1/messages",
            WireProtocol::Unknown => "/__unsupported__",
        }
    }

    pub fn upstream_url(&self) -> Result<reqwest::Url> {
        reqwest::Url::parse(&format!(
            "{}{}",
            self.upstream.origin(),
            self.endpoint_path()
        ))
        .context("build compiled-in model upstream URL")
    }

    pub fn local_base_url(&self) -> String {
        match self.surface {
            SurfaceId::Codex => format!("http://{}/v1", self.listen_address()),
            SurfaceId::ClaudeCode | SurfaceId::Cursor => {
                format!("http://{}", self.listen_address())
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        if self.listen_port < MIN_UNPRIVILEGED_PORT {
            anyhow::bail!("model route port must be between {MIN_UNPRIVILEGED_PORT} and 65535");
        }
        match (self.surface, self.protocol, self.upstream) {
            (SurfaceId::ClaudeCode, WireProtocol::AnthropicMessages, ProviderTarget::Anthropic)
            | (SurfaceId::Codex, WireProtocol::OpenAiResponses, ProviderTarget::OpenAi) => {}
            (SurfaceId::Cursor, _, _) => anyhow::bail!(
                "Cursor model routing is held until a supported local protocol boundary is captured"
            ),
            _ => anyhow::bail!(
                "surface/protocol/upstream combination has no M0 compatibility decision"
            ),
        }
        match (self.surface, self.authentication) {
            (
                SurfaceId::ClaudeCode,
                AuthenticationMode::ApiKey
                | AuthenticationMode::BearerToken
                | AuthenticationMode::Subscription,
            )
            | (
                SurfaceId::Codex,
                AuthenticationMode::ApiKey
                | AuthenticationMode::ChatGptLogin
                | AuthenticationMode::CustomProvider,
            ) => {}
            _ => anyhow::bail!("authentication mode is not a candidate for this surface"),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRegistry {
    #[serde(default = "registry_version")]
    pub version: u32,
    #[serde(default)]
    pub routes: BTreeMap<String, ModelRoute>,
}

fn registry_version() -> u32 {
    REGISTRY_VERSION
}

impl Default for RouteRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            routes: BTreeMap::new(),
        }
    }
}

impl RouteRegistry {
    pub fn load() -> Result<Self> {
        let path = registry_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("inspect model route registry {}", path.display()))?;
        if metadata.len() > 1024 * 1024 {
            anyhow::bail!("model route registry exceeds the 1 MiB safety limit");
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read model route registry {}", path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parse model route registry {}", path.display()))?;
        migrate_v1(&mut value)?;
        let registry: Self = serde_json::from_value(value)
            .with_context(|| format!("parse model route registry {}", path.display()))?;
        registry.validate()?;
        Ok(registry)
    }

    fn validate(&self) -> Result<()> {
        if self.version != REGISTRY_VERSION {
            anyhow::bail!(
                "unsupported model route registry version {} (expected {REGISTRY_VERSION})",
                self.version
            );
        }
        let mut ports = BTreeSet::new();
        for (key, route) in &self.routes {
            route.validate()?;
            if key != &route.id {
                anyhow::bail!("model route registry key does not match its immutable id");
            }
            if !ports.insert(route.listen_port) {
                anyhow::bail!("model routes cannot share a listener port");
            }
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        self.validate()?;
        crate::config::ensure_dir()?;
        let path = registry_path();
        let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
        let encoded = serde_json::to_vec_pretty(self)?;
        std::fs::write(&temporary, encoded)
            .with_context(|| format!("write model route temp {}", temporary.display()))?;
        crate::config::protect_private_file(&temporary)?;
        std::fs::rename(&temporary, &path)
            .with_context(|| format!("replace model route registry {}", path.display()))?;
        crate::config::protect_private_file(&path)?;
        Ok(())
    }
}

pub fn registry_path() -> PathBuf {
    crate::config::ctx_dir().join("model-gateway-routes.json")
}

pub fn add(
    id: &str,
    surface: &str,
    protocol: &str,
    authentication: &str,
    upstream: &str,
    port: u16,
    mode: &str,
) -> Result<()> {
    let route = ModelRoute {
        id: id.to_owned(),
        surface: parse_surface(surface)?,
        protocol: parse_protocol(protocol)?,
        authentication: parse_authentication(authentication)?,
        upstream: ProviderTarget::parse(upstream).ok_or_else(|| {
            anyhow::anyhow!("unknown upstream {upstream:?} (use openai or anthropic)")
        })?,
        listen_port: port,
        mode: ModelRouteMode::parse(mode).ok_or_else(|| {
            anyhow::anyhow!("unknown model route mode {mode:?} (use shadow or testing)")
        })?,
    };
    route.validate()?;
    let mut registry = RouteRegistry::load()?;
    if registry.routes.contains_key(id) {
        anyhow::bail!("model route {id:?} already exists; remove it before replacing it");
    }
    if registry
        .routes
        .values()
        .any(|existing| existing.listen_port == port)
    {
        anyhow::bail!("listener port {port} already belongs to another model route");
    }
    registry.routes.insert(id.to_owned(), route.clone());
    registry.save()?;
    println!("Registered {} model route {id:?}.", route.mode.as_str());
    println!("  client base URL: {}", route.local_base_url());
    println!("  accepted path: {}", route.endpoint_path());
    println!("  fixed upstream: {}", route.upstream.origin());
    println!("No client configuration was changed and no credentials were stored.");
    Ok(())
}

pub fn remove(id: &str) -> Result<()> {
    validate_id(id)?;
    let mut registry = RouteRegistry::load()?;
    if registry.routes.remove(id).is_none() {
        anyhow::bail!("model route {id:?} is not registered");
    }
    registry.save()?;
    println!("Removed model route {id:?}; no client configuration was changed.");
    Ok(())
}

pub fn list() -> Result<()> {
    let registry = RouteRegistry::load()?;
    if registry.routes.is_empty() {
        println!("No model gateway routes registered.");
        return Ok(());
    }
    for route in registry.routes.values() {
        println!(
            "{}\t{}\t{}\t{}\tbase:{}\tpath:{}\tmode:{}",
            route.id,
            route.surface.as_str(),
            route.protocol.as_str(),
            route.authentication.as_str(),
            route.local_base_url(),
            route.endpoint_path(),
            route.mode.as_str()
        );
        println!("  fixed upstream: {}", route.upstream.origin());
    }
    Ok(())
}

fn parse_surface(value: &str) -> Result<SurfaceId> {
    SurfaceId::parse(value).ok_or_else(|| {
        anyhow::anyhow!("unknown surface {value:?} (use claude-code, cursor, or codex)")
    })
}

fn parse_protocol(value: &str) -> Result<WireProtocol> {
    match value {
        "openai-responses" => Ok(WireProtocol::OpenAiResponses),
        "openai-chat-completions" => Ok(WireProtocol::OpenAiChatCompletions),
        "anthropic-messages" => Ok(WireProtocol::AnthropicMessages),
        _ => anyhow::bail!(
            "unknown protocol {value:?} (use openai-responses, openai-chat-completions, or anthropic-messages)"
        ),
    }
}

fn parse_authentication(value: &str) -> Result<AuthenticationMode> {
    match value {
        "api-key" => Ok(AuthenticationMode::ApiKey),
        "bearer-token" => Ok(AuthenticationMode::BearerToken),
        "chatgpt-login" => Ok(AuthenticationMode::ChatGptLogin),
        "subscription" => Ok(AuthenticationMode::Subscription),
        "custom-provider" => Ok(AuthenticationMode::CustomProvider),
        _ => anyhow::bail!(
            "unknown authentication mode {value:?} (use api-key, bearer-token, chatgpt-login, subscription, or custom-provider)"
        ),
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        anyhow::bail!("route id must be 1-64 lowercase letters, digits, or hyphens");
    }
    Ok(())
}

fn migrate_v1(value: &mut serde_json::Value) -> Result<()> {
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    if version != 1 {
        return Ok(());
    }
    if value
        .get("routes")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|routes| routes.values())
        .any(|route| {
            route
                .get("transformationsEnabled")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
    {
        anyhow::bail!("legacy model route attempted to enable transformations outside M3 mode");
    }
    let root = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("model route registry root must be an object"))?;
    root.insert("version".into(), serde_json::json!(REGISTRY_VERSION));
    if let Some(routes) = root
        .get_mut("routes")
        .and_then(serde_json::Value::as_object_mut)
    {
        for route in routes.values_mut() {
            if let Some(route) = route.as_object_mut() {
                route.remove("transformationsEnabled");
                route
                    .entry("mode")
                    .or_insert_with(|| serde_json::json!("shadow"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_route(id: &str, port: u16) -> ModelRoute {
        ModelRoute {
            id: id.into(),
            surface: SurfaceId::Codex,
            protocol: WireProtocol::OpenAiResponses,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::OpenAi,
            listen_port: port,
            mode: ModelRouteMode::Shadow,
        }
    }

    #[test]
    fn cursor_fails_closed_while_testing_is_explicit() {
        let mut route = codex_route("codex", 8871);
        route.surface = SurfaceId::Cursor;
        assert!(route.validate().is_err());
        route.surface = SurfaceId::Codex;
        route.mode = ModelRouteMode::Testing;
        assert!(route.validate().is_ok());
    }

    #[test]
    fn legacy_routes_migrate_to_shadow_but_legacy_true_is_rejected() {
        let mut safe = serde_json::json!({
            "version": 1,
            "routes": {"codex": {
                "id":"codex", "surface":"codex", "protocol":"openai-responses",
                "authentication":"api-key", "upstream":"openai", "listenPort":8871,
                "transformationsEnabled":false
            }}
        });
        migrate_v1(&mut safe).unwrap();
        let migrated: RouteRegistry = serde_json::from_value(safe).unwrap();
        assert_eq!(migrated.version, 2);
        assert_eq!(migrated.routes["codex"].mode, ModelRouteMode::Shadow);

        let mut unsafe_route = serde_json::json!({
            "version": 1,
            "routes": {"codex": {"transformationsEnabled":true}}
        });
        assert!(migrate_v1(&mut unsafe_route).is_err());
    }

    #[test]
    fn duplicate_listener_ports_are_rejected() {
        let mut registry = RouteRegistry::default();
        registry
            .routes
            .insert("one".into(), codex_route("one", 8871));
        registry
            .routes
            .insert("two".into(), codex_route("two", 8871));
        assert!(registry.validate().is_err());
    }

    #[test]
    fn upstreams_are_compiled_in_and_cannot_be_parsed_from_a_url() {
        assert_eq!(
            ProviderTarget::parse("openai"),
            Some(ProviderTarget::OpenAi)
        );
        assert!(ProviderTarget::parse("https://evil.example").is_none());
        assert_eq!(
            codex_route("codex", 8871).upstream_url().unwrap().as_str(),
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(
            codex_route("codex", 8871).local_base_url(),
            "http://127.0.0.1:8871/v1"
        );
    }

    #[test]
    fn registry_round_trip_is_private_and_contains_no_credentials() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let old_home = std::env::var_os("CTX_HOME");
        std::env::set_var("CTX_HOME", temp.path());

        add(
            "codex-api",
            "codex",
            "openai-responses",
            "api-key",
            "openai",
            8871,
            "shadow",
        )
        .unwrap();
        let loaded = RouteRegistry::load().unwrap();
        let encoded = std::fs::read_to_string(registry_path()).unwrap();
        remove("codex-api").unwrap();

        if let Some(value) = old_home {
            std::env::set_var("CTX_HOME", value);
        } else {
            std::env::remove_var("CTX_HOME");
        }
        assert_eq!(loaded.routes.len(), 1);
        assert!(!encoded.to_ascii_lowercase().contains("token"));
        assert!(!encoded.to_ascii_lowercase().contains("secret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(temp.path().join("model-gateway-routes.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
