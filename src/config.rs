use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn ctx_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CTX_HOME") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ctx")
}

pub fn ensure_dir() -> Result<()> {
    std::fs::create_dir_all(ctx_dir())?;
    Ok(())
}

pub fn analytics_path() -> PathBuf {
    ctx_dir().join("analytics.jsonl")
}

pub fn db_path() -> PathBuf {
    ctx_dir().join("ctx.db")
}

pub fn models_dir() -> PathBuf {
    ctx_dir().join("models")
}

pub fn minilm_onnx_path() -> PathBuf {
    models_dir().join("all-MiniLM-L6-v2.onnx")
}

pub fn minilm_tokenizer_path() -> PathBuf {
    models_dir().join("tokenizer.json")
}

pub fn behavior_hints_path() -> PathBuf {
    ctx_dir().join("behavior-hints.json")
}

pub fn system_prefix_path() -> PathBuf {
    ctx_dir().join("system_prefix.md")
}

pub fn write_json_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Merge or replace the `ctx` entry under `mcpServers` (Claude Code / Cursor / Desktop config shape).
pub fn merge_ctx_into_mcp_servers(doc: &mut Value, ctx_bin: &str) -> Result<()> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("config root must be a JSON object"))?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(server_obj) = servers.as_object_mut() else {
        anyhow::bail!("mcpServers must be a JSON object");
    };
    server_obj.insert(
        "ctx".to_string(),
        serde_json::json!({
            "command": ctx_bin,
            "args": ["mcp"],
        }),
    );
    Ok(())
}

/// Remove the `ctx` MCP server entry if present. Returns true if a change was made.
pub fn remove_ctx_from_mcp_servers(doc: &mut Value) -> bool {
    let Some(obj) = doc.as_object_mut() else {
        return false;
    };
    let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return false;
    };
    servers.remove("ctx").is_some()
}

pub fn claude_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

/// True when `~/.claude/settings.json` exists (Claude Code CLI or prior ctx setup).
pub fn claude_code_cli_present() -> bool {
    claude_settings_path().is_file()
}

/// Same as [`claude_code_cli_present`] but relative to `home` (for tests).
pub fn claude_code_cli_present_for_home(home: &std::path::Path) -> bool {
    home.join(".claude").join("settings.json").is_file()
}

/// Anthropic Claude Desktop app support directory (contains `claude_desktop_config.json`).
pub fn claude_desktop_support_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    claude_desktop_support_dir_for_home(&home)
}

/// Same as [`claude_desktop_support_dir`] for an explicit home directory (for tests).
pub fn claude_desktop_support_dir_for_home(home: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let p = home.join("Library/Application Support/Claude");
        if p.is_dir() {
            return Some(p);
        }
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            home.join(".config/Claude"),
            home.join(".local/share/Claude"),
        ] {
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = home;
        for base in [dirs::data_dir(), dirs::data_local_dir()] {
            if let Some(app) = base {
                let p = app.join("Claude");
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn claude_desktop_installed_for_home(home: &Path) -> bool {
    claude_desktop_support_dir_for_home(home).is_some()
}

/// `claude_desktop_config.json` path when the Desktop support directory exists.
pub fn claude_desktop_config_path() -> Option<PathBuf> {
    Some(claude_desktop_support_dir()?.join("claude_desktop_config.json"))
}

pub fn claude_desktop_installed() -> bool {
    claude_desktop_support_dir().is_some()
}

/// Roots for Desktop local-agent session logs (may not exist on all installs).
pub fn claude_desktop_session_roots() -> Vec<PathBuf> {
    let Some(base) = claude_desktop_support_dir() else {
        return Vec::new();
    };
    let lam = base.join("local-agent-mode-sessions");
    if lam.is_dir() {
        vec![lam]
    } else {
        Vec::new()
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub active_profile: Option<String>,
    pub proxy_port: Option<u16>,
    /// Port for `ctx dashboard` (used by filter.js POST /api/ingest-request).
    #[serde(default)]
    pub dashboard_port: Option<u16>,
    pub proxy_upstream: Option<String>,
    /// `node_inject` (NODE_OPTIONS), legacy `mitm` (HTTPS_PROXY), or `reverse` (ANTHROPIC_BASE_URL).
    #[serde(default)]
    pub proxy_install_mode: Option<String>,
    pub original_base_url: Option<String>,
    #[serde(default = "default_true")]
    pub auto_profile_enabled: bool,
    #[serde(default = "default_true")]
    pub inject_enabled: bool,
    /// Monthly spend limit in USD -- set to your actual Anthropic billing cap.
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    /// Actual spend this month entered manually from Anthropic billing page.
    /// Overrides the JSONL-computed estimate in the budget bar when set.
    #[serde(default)]
    pub monthly_actual_spend_usd: Option<f64>,
    /// Session total at the moment monthly_actual_spend_usd was last set.
    /// Used to compute the running delta: live = actual + (current - baseline).
    #[serde(default)]
    pub monthly_actual_spend_baseline_usd: Option<f64>,
    /// When `Some(false)`, ingest skips prompt-derived text in SQLite (first message, turn prefixes, embed text, top turns).
    #[serde(default)]
    pub store_prompt_text: Option<bool>,
    /// When `Some(false)`, session embeddings are not computed and existing embedding rows are cleared on ingest.
    #[serde(default)]
    pub embeddings_enabled: Option<bool>,
    /// Gap in minutes between requests before a new session is started. Default: 30.
    #[serde(default)]
    pub session_gap_minutes: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Default on: store truncated prompts for dashboard intelligence.
    pub fn store_prompt_text_enabled(&self) -> bool {
        self.store_prompt_text != Some(false)
    }

    /// Default on: compute embeddings for pattern alerts and similarity.
    pub fn embeddings_enabled(&self) -> bool {
        self.embeddings_enabled != Some(false)
    }

    pub fn load() -> Self {
        let path = ctx_dir().join("config.toml");
        if !path.exists() {
            return Self {
                auto_profile_enabled: true,
                inject_enabled: true,
                ..Default::default()
            };
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(|| Self {
                auto_profile_enabled: true,
                inject_enabled: true,
                ..Default::default()
            })
    }

    pub fn save(&self) -> Result<()> {
        ensure_dir()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(ctx_dir().join("config.toml"), content)?;
        Ok(())
    }
}
