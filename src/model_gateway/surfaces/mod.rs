//! Reversible, field-scoped configuration adapters for coding-agent surfaces.
//!
//! These adapters deliberately own only a documented model base-URL field. They never copy an
//! entire client configuration into CTX state because those files may contain credentials.

mod claude_code;
mod codex;
mod cursor;

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::registry::ModelRoute;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedConfigField {
    pub strategy: String,
    pub location: String,
    pub config_existed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_value: Option<String>,
    pub ctx_value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldState {
    NotOwned,
    CtxOwned,
    Original,
    UserModified,
    MissingConfiguration,
    Unsupported,
}

pub fn prepare(route: &ModelRoute, home: &Path) -> Result<OwnedConfigField> {
    match route.surface {
        crate::surface::SurfaceId::Codex => codex::prepare(route, home),
        crate::surface::SurfaceId::ClaudeCode => claude_code::prepare(route, home),
        crate::surface::SurfaceId::Cursor => cursor::prepare(route, home),
    }
}

pub fn apply(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    match route.surface {
        crate::surface::SurfaceId::Codex => codex::apply(route, home, field),
        crate::surface::SurfaceId::ClaudeCode => claude_code::apply(route, home, field),
        crate::surface::SurfaceId::Cursor => cursor::apply(route, home, field),
    }
}

pub fn restore(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> Result<()> {
    match route.surface {
        crate::surface::SurfaceId::Codex => codex::restore(route, home, field),
        crate::surface::SurfaceId::ClaudeCode => claude_code::restore(route, home, field),
        crate::surface::SurfaceId::Cursor => cursor::restore(route, home, field),
    }
}

pub fn inspect(route: &ModelRoute, home: &Path, field: &OwnedConfigField) -> FieldState {
    match route.surface {
        crate::surface::SurfaceId::Codex => codex::inspect(route, home, field),
        crate::surface::SurfaceId::ClaudeCode => claude_code::inspect(route, home, field),
        crate::surface::SurfaceId::Cursor => cursor::inspect(route, home, field),
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use anyhow::Context;

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("client configuration has no parent directory"))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create client config directory {}", parent.display()))?;
    let existing_permissions = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let temporary = path.with_extension(format!("ctx.tmp.{}", std::process::id()));
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("write temporary client config {}", temporary.display()))?;
    if let Some(permissions) = existing_permissions {
        std::fs::set_permissions(&temporary, permissions)?;
    } else {
        crate::config::protect_private_file(&temporary)?;
    }
    std::fs::rename(&temporary, path)
        .with_context(|| format!("replace client config {}", path.display()))?;
    Ok(())
}

pub(super) fn validate_restorable_base_url(value: &str, allowed_host: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(value).map_err(|_| {
        anyhow::anyhow!("existing model base URL is malformed; CTX will not own it")
    })?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(allowed_host)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!(
            "existing model base URL is not the credential-free official {allowed_host} route; CTX preserved it"
        );
    }
    Ok(())
}
