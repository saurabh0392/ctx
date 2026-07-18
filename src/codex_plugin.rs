//! Materialize and register the bundled CTX Codex plugin.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

const MARKETPLACE_NAME: &str = "ctx";
const PLUGIN_NAME: &str = "ctx";

pub fn codex_available() -> bool {
    Command::new("codex")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn marketplace_root() -> PathBuf {
    crate::config::ctx_dir().join("codex-marketplace")
}

fn write(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body).with_context(|| format!("write {}", path.display()))
}

pub fn materialize() -> Result<PathBuf> {
    let root = marketplace_root();
    let plugin = root.join("plugins").join(PLUGIN_NAME);
    write(
        &plugin.join(".codex-plugin/plugin.json"),
        include_str!("../plugins/ctx/.codex-plugin/plugin.json"),
    )?;
    let mut mcp: serde_json::Value =
        serde_json::from_str(include_str!("../plugins/ctx/.mcp.json"))?;
    if let Some(slot) = mcp.pointer_mut("/mcpServers/ctx/command") {
        *slot = serde_json::json!(std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or_else(|| "ctx".into()));
    }
    write(
        &plugin.join(".mcp.json"),
        &serde_json::to_string_pretty(&mcp)?,
    )?;
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../plugins/ctx/hooks/hooks.json"),
    )?;
    write(
        &plugin.join("hooks/run-ctx.sh"),
        include_str!("../plugins/ctx/hooks/run-ctx.sh"),
    )?;
    write(
        &plugin.join("hooks/run-ctx.ps1"),
        include_str!("../plugins/ctx/hooks/run-ctx.ps1"),
    )?;
    write(
        &plugin.join("skills/ctx/SKILL.md"),
        include_str!("../plugins/ctx/skills/ctx/SKILL.md"),
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = plugin.join("hooks/run-ctx.sh");
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(script, perms)?;
    }

    let marketplace = serde_json::json!({
        "name": MARKETPLACE_NAME,
        "interface": { "displayName": "CTX" },
        "plugins": [{
            "name": PLUGIN_NAME,
            "source": { "source": "local", "path": "./plugins/ctx" },
            "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
            "category": "Productivity"
        }]
    });
    write(
        &root.join(".agents/plugins/marketplace.json"),
        &serde_json::to_string_pretty(&marketplace)?,
    )?;
    Ok(root)
}

pub fn install_if_present() -> Result<bool> {
    if !codex_available() {
        return Ok(false);
    }
    let root = materialize()?;
    let marketplace = Command::new("codex")
        .args(["plugin", "marketplace", "add"])
        .arg(&root)
        .args(["--json"])
        .output()
        .context("run codex plugin marketplace add")?;
    if !marketplace.status.success() {
        // Re-adding an existing local marketplace is harmless. Only proceed when Codex confirms
        // the expected marketplace is already present; otherwise surface the real error.
        let listed = Command::new("codex")
            .args(["plugin", "marketplace", "list", "--json"])
            .output()?;
        let listing = String::from_utf8_lossy(&listed.stdout);
        if !listed.status.success() || !listing.contains(MARKETPLACE_NAME) {
            anyhow::bail!(
                "Codex could not register the CTX marketplace: {}",
                String::from_utf8_lossy(&marketplace.stderr).trim()
            );
        }
    }

    let install = Command::new("codex")
        .args(["plugin", "add", "ctx@ctx", "--json"])
        .output()
        .context("run codex plugin add")?;
    if !install.status.success() {
        let listed = Command::new("codex")
            .args(["plugin", "list", "--json"])
            .output()?;
        let listing = String::from_utf8_lossy(&listed.stdout);
        if !listed.status.success()
            || !listing.contains("\"name\":\"ctx\"") && !listing.contains("\"name\": \"ctx\"")
        {
            anyhow::bail!(
                "Codex could not install the CTX plugin: {}",
                String::from_utf8_lossy(&install.stderr).trim()
            );
        }
    }
    Ok(true)
}

pub fn uninstall_if_owned() -> Result<bool> {
    if !codex_available() && !marketplace_root().exists() {
        return Ok(false);
    }
    if codex_available() {
        let _ = Command::new("codex")
            .args(["plugin", "remove", "ctx@ctx", "--json"])
            .status();
        let _ = Command::new("codex")
            .args([
                "plugin",
                "marketplace",
                "remove",
                MARKETPLACE_NAME,
                "--json",
            ])
            .status();
    }
    let root = marketplace_root();
    if root.exists() {
        std::fs::remove_dir_all(&root)
            .with_context(|| format!("remove CTX-owned plugin bundle {}", root.display()))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialized_marketplace_is_self_contained() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let root = materialize().unwrap();
        assert!(root.join(".agents/plugins/marketplace.json").is_file());
        assert!(root.join("plugins/ctx/hooks/hooks.json").is_file());
        let mcp = std::fs::read_to_string(root.join("plugins/ctx/.mcp.json")).unwrap();
        assert!(mcp.contains("ctx"));
    }
}
