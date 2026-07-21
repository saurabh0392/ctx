use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{value, DocumentMut, Item, Table};

use super::{atomic_write, validate_restorable_base_url, FieldState, OwnedConfigField};
use crate::model_gateway::registry::ModelRoute;
use crate::model_gateway::route::AuthenticationMode;

const STRATEGY: &str = "codex-ctx-provider-http-sse-v1";
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

fn provider_table_is_owned(table: &Table, route: &ModelRoute) -> bool {
    table.len() == 5
        && table.get("name").and_then(Item::as_str) == Some("CTX local model gateway")
        && table.get("base_url").and_then(Item::as_str) == Some(route.local_base_url().as_str())
        && table.get("wire_api").and_then(Item::as_str) == Some("responses")
        && table.get("requires_openai_auth").and_then(Item::as_bool) == Some(true)
        && table.get("supports_websockets").and_then(Item::as_bool) == Some(false)
}

fn validate_profile(document: &DocumentMut, route: &ModelRoute) -> Result<()> {
    if route.authentication != AuthenticationMode::ApiKey {
        anyhow::bail!(
            "Codex ChatGPT-login routing remains held until its fixed backend and WebSocket/auth refresh contract pass live capture; M4 currently enables only api-key routes"
        );
    }
    if let Some(provider) = selected_provider(document)? {
        if provider != "openai" {
            anyhow::bail!(
                "Codex currently selects model provider {provider:?}; CTX preserved that customized provider"
            );
        }
    }
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
        strategy: STRATEGY.into(),
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
        if !provider_table_is_owned(existing, route) {
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
        // The M1-M4 relay supports HTTP/SSE. Explicitly keep Codex off the Responses WebSocket
        // transport rather than pretending the existing relay can terminate it.
        provider["supports_websockets"] = value(false);
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
    if !provider_table(&document)?.is_some_and(|table| provider_table_is_owned(table, route)) {
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
                && provider_table_is_owned(table, route) =>
        {
            FieldState::CtxOwned
        }
        (Ok(selected), Ok(None)) if selected == field.original_value => FieldState::Original,
        (Ok(_), Ok(_)) => FieldState::UserModified,
        _ => FieldState::Unsupported,
    }
}

fn validate_field(field: &OwnedConfigField) -> Result<()> {
    if field.strategy != STRATEGY || field.location != LOCATION || field.ctx_value != CTX_PROVIDER {
        anyhow::bail!("Codex ownership receipt uses an unsupported strategy");
    }
    Ok(())
}
