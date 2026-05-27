use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

pub fn claude_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
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
