use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::{atomic_write, validate_restorable_base_url, FieldState, OwnedConfigField};
use crate::model_gateway::registry::ModelRoute;

const STRATEGY: &str = "claude-user-anthropic-base-url-v1";
const LOCATION: &str = "~/.claude/settings.json";

fn path(home: &Path) -> PathBuf {
    home.join(".claude/settings.json")
}

fn read(path: &Path) -> Result<(bool, Value)> {
    if !path.exists() {
        return Ok((false, Value::Object(Map::new())));
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read Claude Code settings {}", path.display()))?;
    let document = serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("parse Claude Code settings {}", path.display()))?;
    if !document.is_object() {
        anyhow::bail!("Claude Code settings root is not an object");
    }
    Ok((true, document))
}

fn env(document: &Value) -> Result<Option<&Map<String, Value>>> {
    match document.get("env") {
        None => Ok(None),
        Some(Value::Object(env)) => Ok(Some(env)),
        Some(_) => {
            anyhow::bail!("Claude Code settings env is not an object; CTX will not replace it")
        }
    }
}

fn current(document: &Value) -> Result<Option<String>> {
    match env(document)?.and_then(|env| env.get("ANTHROPIC_BASE_URL")) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => {
            anyhow::bail!("Claude Code ANTHROPIC_BASE_URL is not a string; CTX will not replace it")
        }
    }
}

fn validate_profile(document: &Value) -> Result<()> {
    let cloud = env(document)?.is_some_and(|env| {
        [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ]
        .iter()
        .any(|key| env.get(*key).is_some_and(truthy))
    });
    if cloud {
        anyhow::bail!(
            "Claude Code uses a cloud-provider route; M4 supports only the direct Anthropic Messages route"
        );
    }
    Ok(())
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::String(value) => !matches!(value.as_str(), "" | "0" | "false" | "False" | "FALSE"),
        Value::Number(value) => value.as_i64() != Some(0),
        _ => false,
    }
}

fn set_current(document: &mut Value, value: Option<&str>) -> Result<()> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Claude Code settings root is not an object"))?;
    if !root.contains_key("env") {
        root.insert("env".into(), Value::Object(Map::new()));
    }
    let env = root
        .get_mut("env")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("Claude Code settings env is not an object"))?;
    match value {
        Some(value) => {
            env.insert("ANTHROPIC_BASE_URL".into(), Value::String(value.into()));
        }
        None => {
            env.remove("ANTHROPIC_BASE_URL");
            if env.is_empty() {
                root.remove("env");
            }
        }
    }
    Ok(())
}

pub(super) fn prepare(route: &ModelRoute, home: &Path) -> Result<OwnedConfigField> {
    let (config_existed, document) = read(&path(home))?;
    validate_profile(&document)?;
    let original_value = current(&document)?;
    if original_value.as_deref().is_some_and(|value| {
        value.starts_with("http://127.0.0.1:") || value.starts_with("http://localhost:")
    }) {
        anyhow::bail!(
            "Claude Code already has a loopback ANTHROPIC_BASE_URL; CTX will not take ownership of another local route"
        );
    }
    if let Some(original) = &original_value {
        validate_restorable_base_url(original, "api.anthropic.com")?;
    }
    Ok(OwnedConfigField {
        strategy: STRATEGY.into(),
        location: LOCATION.into(),
        config_existed,
        original_value,
        ctx_value: route.local_base_url(),
    })
}

pub(super) fn apply(_route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    validate_field(field)?;
    let config_path = path(home);
    let (_, mut document) = read(&config_path)?;
    let found = current(&document)?;
    if found != field.original_value && found.as_deref() != Some(field.ctx_value.as_str()) {
        anyhow::bail!(
            "Claude Code ANTHROPIC_BASE_URL changed after CTX prepared activation; no file was modified"
        );
    }
    set_current(&mut document, Some(&field.ctx_value))?;
    let rendered = serde_json::to_vec_pretty(&document)?;
    atomic_write(&config_path, &rendered)
}

pub(super) fn restore(_route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    validate_field(field)?;
    let config_path = path(home);
    let (_, mut document) = read(&config_path)?;
    if current(&document)?.as_deref() != Some(field.ctx_value.as_str()) {
        anyhow::bail!(
            "Claude Code ANTHROPIC_BASE_URL was changed by the user after activation; CTX preserved it and refused destructive restoration"
        );
    }
    set_current(&mut document, field.original_value.as_deref())?;
    if !field.config_existed && document.as_object().is_some_and(Map::is_empty) {
        std::fs::remove_file(&config_path).with_context(|| {
            format!(
                "remove CTX-created Claude Code settings {}",
                config_path.display()
            )
        })?;
        return Ok(());
    }
    let rendered = serde_json::to_vec_pretty(&document)?;
    atomic_write(&config_path, &rendered)
}

pub(super) fn inspect(_route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> FieldState {
    if validate_field(field).is_err() {
        return FieldState::Unsupported;
    }
    let Ok((_, document)) = read(&path(home)) else {
        return FieldState::MissingConfiguration;
    };
    match current(&document) {
        Ok(value) if value.as_deref() == Some(field.ctx_value.as_str()) => FieldState::CtxOwned,
        Ok(value) if value == field.original_value => FieldState::Original,
        Ok(_) => FieldState::UserModified,
        Err(_) => FieldState::Unsupported,
    }
}

fn validate_field(field: &OwnedConfigField) -> Result<()> {
    if field.strategy != STRATEGY || field.location != LOCATION {
        anyhow::bail!("Claude Code ownership receipt uses an unsupported strategy");
    }
    Ok(())
}
