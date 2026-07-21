use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{value, DocumentMut, Item, Table};

use super::{atomic_write, validate_restorable_base_url, FieldState, OwnedConfigField};
use crate::model_gateway::registry::ModelRoute;
use crate::model_gateway::route::AuthenticationMode;

const STRATEGY_HTTP_SSE_V1: &str = "codex-ctx-provider-http-sse-v1";
const STRATEGY_HTTP_SSE_WS_V2: &str = "codex-ctx-provider-http-sse-ws-v2";
const LOCATION: &str = "~/.codex/config.toml:model_provider+model_providers.ctx-model-gateway";
const CTX_PROVIDER: &str = "ctx-model-gateway";

fn path(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}

fn read(path: &Path) -> Result<(bool, DocumentMut)> {
    if !path.exists() {
        return Ok((false, DocumentMut::new()));
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read Codex user config {}", path.display()))?;
    let document = raw
        .parse::<DocumentMut>()
        .with_context(|| format!("parse Codex user config {}", path.display()))?;
    Ok((true, document))
}

fn selected_provider(document: &DocumentMut) -> Result<Option<String>> {
    match document.get("model_provider") {
        None => Ok(None),
        Some(item) => item
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| {
                anyhow::anyhow!("Codex model_provider is not a string; CTX will not replace it")
            }),
    }
}

fn provider_table(document: &DocumentMut) -> Result<Option<&Table>> {
    let Some(providers) = document.get("model_providers") else {
        return Ok(None);
    };
    let providers = providers
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("Codex model_providers is not a table"))?;
    match providers.get(CTX_PROVIDER) {
        None => Ok(None),
        Some(item) => item
            .as_table()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("existing CTX provider id is not a table")),
    }
}

fn provider_table_is_owned(table: &Table, route: &ModelRoute, field: &OwnedConfigField) -> bool {
    table.len() == 5
        && table.get("name").and_then(Item::as_str) == Some("CTX local model gateway")
        && table.get("base_url").and_then(Item::as_str) == Some(route.local_base_url().as_str())
        && table.get("wire_api").and_then(Item::as_str) == Some("responses")
        && table.get("requires_openai_auth").and_then(Item::as_bool) == Some(true)
        && table.get("supports_websockets").and_then(Item::as_bool)
            == Some(field.strategy == STRATEGY_HTTP_SSE_WS_V2)
}

fn validate_profile(document: &DocumentMut, route: &ModelRoute) -> Result<()> {
    if let Some(provider) = selected_provider(document)? {
        if provider != "openai" {
            anyhow::bail!(
                "Codex currently selects model provider {provider:?}; CTX preserved that customized provider"
            );
        }
    }
    match route.authentication {
        AuthenticationMode::ApiKey => {
            if let Some(login) = document.get("forced_login_method").and_then(Item::as_str) {
                if login != "api" {
                    anyhow::bail!(
                        "Codex forced_login_method {login:?} does not match the supported api-key route"
                    );
                }
            }
            if let Some(base) = document.get("openai_base_url") {
                let base = base.as_str().ok_or_else(|| {
                    anyhow::anyhow!("Codex openai_base_url is not a string; CTX preserved it")
                })?;
                validate_restorable_base_url(base, "api.openai.com")?;
            }
        }
        AuthenticationMode::ChatGptLogin => {
            if let Some(login) = document.get("forced_login_method").and_then(Item::as_str) {
                if login != "chatgpt" {
                    anyhow::bail!(
                        "Codex forced_login_method {login:?} does not match the supported ChatGPT-login route"
                    );
                }
            }
            if let Some(base) = document.get("chatgpt_base_url") {
                let base = base.as_str().ok_or_else(|| {
                    anyhow::anyhow!("Codex chatgpt_base_url is not a string; CTX preserved it")
                })?;
                validate_restorable_base_url(base, "chatgpt.com")?;
            }
        }
        _ => anyhow::bail!("Codex model lifecycle supports only api-key or chatgpt-login routes"),
    }
    Ok(())
}

pub(super) fn prepare(route: &ModelRoute, home: &Path) -> Result<OwnedConfigField> {
    let (config_existed, document) = read(&path(home))?;
    validate_profile(&document, route)?;
    if provider_table(&document)?.is_some() {
        anyhow::bail!(
            "Codex already defines model provider {CTX_PROVIDER:?}; CTX will not replace a user-owned table"
        );
    }
    Ok(OwnedConfigField {
        strategy: STRATEGY_HTTP_SSE_WS_V2.into(),
        location: LOCATION.into(),
        config_existed,
        original_value: selected_provider(&document)?,
        ctx_value: CTX_PROVIDER.into(),
    })
}

pub(super) fn apply(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    validate_field(field)?;
    let config_path = path(home);
    let (_, mut document) = read(&config_path)?;
    let selected = selected_provider(&document)?;
    if selected != field.original_value && selected.as_deref() != Some(CTX_PROVIDER) {
        anyhow::bail!(
            "Codex model_provider changed after CTX prepared activation; no file was modified"
        );
    }
    if let Some(existing) = provider_table(&document)? {
        if !provider_table_is_owned(existing, route, field) {
            anyhow::bail!("Codex CTX provider table was modified; no file was changed");
        }
    } else {
        if !document.contains_key("model_providers") {
            document["model_providers"] = Item::Table(Table::new());
        }
        let providers = document["model_providers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Codex model_providers is not a table"))?;
        let mut provider = Table::new();
        provider["name"] = value("CTX local model gateway");
        provider["base_url"] = value(route.local_base_url());
        provider["wire_api"] = value("responses");
        provider["requires_openai_auth"] = value(true);
        provider["supports_websockets"] = value(field.strategy == STRATEGY_HTTP_SSE_WS_V2);
        providers.insert(CTX_PROVIDER, Item::Table(provider));
    }
    document["model_provider"] = value(CTX_PROVIDER);
    atomic_write(&config_path, document.to_string().as_bytes())
}

pub(super) fn restore(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    validate_field(field)?;
    let config_path = path(home);
    let (_, mut document) = read(&config_path)?;
    if selected_provider(&document)?.as_deref() != Some(CTX_PROVIDER) {
        anyhow::bail!(
            "Codex model_provider was changed by the user after activation; CTX preserved it and refused destructive restoration"
        );
    }
    if !provider_table(&document)?.is_some_and(|table| provider_table_is_owned(table, route, field))
    {
        anyhow::bail!(
            "Codex CTX provider table was changed by the user after activation; CTX preserved it"
        );
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex model_providers is not a table"))?;
    providers.remove(CTX_PROVIDER);
    if providers.is_empty() {
        document.remove("model_providers");
    }
    match &field.original_value {
        Some(original) => document["model_provider"] = value(original),
        None => {
            document.remove("model_provider");
        }
    }
    let rendered = document.to_string();
    if !field.config_existed && rendered.trim().is_empty() {
        std::fs::remove_file(&config_path).with_context(|| {
            format!("remove CTX-created Codex config {}", config_path.display())
        })?;
        return Ok(());
    }
    atomic_write(&config_path, rendered.as_bytes())
}

pub(super) fn inspect(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> FieldState {
    if validate_field(field).is_err() {
        return FieldState::Unsupported;
    }
    let Ok((_, document)) = read(&path(home)) else {
        return FieldState::MissingConfiguration;
    };
    let selected = selected_provider(&document);
    let table = provider_table(&document);
    match (selected, table) {
        (Ok(selected), Ok(Some(table)))
            if selected.as_deref() == Some(CTX_PROVIDER)
                && provider_table_is_owned(table, route, field) =>
        {
            FieldState::CtxOwned
        }
        (Ok(selected), Ok(None)) if selected == field.original_value => FieldState::Original,
        (Ok(_), Ok(_)) => FieldState::UserModified,
        _ => FieldState::Unsupported,
    }
}

fn validate_field(field: &OwnedConfigField) -> Result<()> {
    if !matches!(
        field.strategy.as_str(),
        STRATEGY_HTTP_SSE_V1 | STRATEGY_HTTP_SSE_WS_V2
    ) || field.location != LOCATION
        || field.ctx_value != CTX_PROVIDER
    {
        anyhow::bail!("Codex ownership receipt uses an unsupported strategy");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::registry::{ModelRouteMode, ProviderTarget};
    use crate::model_gateway::route::WireProtocol;
    use crate::surface::SurfaceId;

    fn chatgpt_route() -> ModelRoute {
        ModelRoute {
            id: "codex-chatgpt".into(),
            surface: SurfaceId::Codex,
            protocol: WireProtocol::OpenAiResponses,
            authentication: AuthenticationMode::ChatGptLogin,
            upstream: ProviderTarget::OpenAiChatGpt,
            listen_port: 8873,
            mode: ModelRouteMode::Shadow,
        }
    }

    #[test]
    fn chatgpt_route_adds_ws_provider_and_restores_without_auth_state() {
        let home = tempfile::tempdir().unwrap();
        let config_dir = home.path().join(".codex");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config = config_dir.join("config.toml");
        std::fs::write(
            &config,
            "# preserve me\nmodel = \"gpt-5\"\nforced_login_method = \"chatgpt\"\n[mcp_servers.keep]\nurl = \"https://example.com/mcp\"\n",
        )
        .unwrap();

        let route = chatgpt_route();
        let field = prepare(&route, home.path()).unwrap();
        assert_eq!(field.strategy, STRATEGY_HTTP_SSE_WS_V2);
        apply(&route, home.path(), &field).unwrap();

        let enabled = std::fs::read_to_string(&config).unwrap();
        assert!(enabled.contains("# preserve me"));
        assert!(enabled.contains("model_provider = \"ctx-model-gateway\""));
        assert!(enabled.contains("http://127.0.0.1:8873/backend-api/codex"));
        assert!(enabled.contains("requires_openai_auth = true"));
        assert!(enabled.contains("supports_websockets = true"));
        assert!(enabled.contains("[mcp_servers.keep]"));
        assert!(!enabled.to_ascii_lowercase().contains("access_token"));
        assert_eq!(inspect(&route, home.path(), &field), FieldState::CtxOwned);

        restore(&route, home.path(), &field).unwrap();
        let restored = std::fs::read_to_string(&config).unwrap();
        assert!(restored.contains("# preserve me"));
        assert!(restored.contains("model = \"gpt-5\""));
        assert!(restored.contains("forced_login_method = \"chatgpt\""));
        assert!(restored.contains("[mcp_servers.keep]"));
        assert!(!restored.contains("ctx-model-gateway"));
    }

    #[test]
    fn chatgpt_route_refuses_an_api_forced_profile() {
        let home = tempfile::tempdir().unwrap();
        let config_dir = home.path().join(".codex");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "forced_login_method = \"api\"\n",
        )
        .unwrap();
        assert!(prepare(&chatgpt_route(), home.path()).is_err());
    }
}
