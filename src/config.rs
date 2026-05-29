use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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

/// Machine-generated behavioral prefix (refreshed on ingest).
pub fn adaptive_prefix_path() -> PathBuf {
    ctx_dir().join("adaptive_prefix.md")
}

pub fn statusline_bin_dir() -> PathBuf {
    ctx_dir().join("bin")
}

pub fn statusline_script_path() -> PathBuf {
    statusline_bin_dir().join("ctx-statusline.sh")
}

/// Write the ctx-managed Claude Code statusLine script (allowance bridge).
pub fn install_statusline_script(dashboard_port: u16) -> Result<()> {
    ensure_dir()?;
    let dir = statusline_bin_dir();
    std::fs::create_dir_all(&dir)?;
    let script = include_str!("../scripts/ctx-statusline.sh");
    let script = script.replace("__DASHBOARD_PORT__", &dashboard_port.to_string());
    let path = statusline_script_path();
    std::fs::write(&path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
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

/// Commands referenced by a Claude Code `hooks` matcher entry (flat `command` or nested `hooks`).
fn hook_commands_from_entry(entry: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(cmd) = entry.get("command").and_then(|c| c.as_str()) {
        out.push(cmd.to_string());
    }
    if let Some(arr) = entry.get("hooks").and_then(|h| h.as_array()) {
        for h in arr {
            if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                out.push(cmd.to_string());
            }
        }
    }
    out
}

/// Remove ctx-managed `PreToolUse` (`ctx hook`) and `Stop` (`ctx gain --brief`) hook entries from settings JSON.
/// Returns true when the document was modified.
pub fn strip_ctx_managed_hooks_from_settings(settings: &mut Value) -> bool {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return false;
    };
    let mut changed = false;
    if let Some(arr) = hooks.get_mut("PreToolUse").and_then(|a| a.as_array_mut()) {
        let before = arr.len();
        arr.retain(|entry| {
            !hook_commands_from_entry(entry)
                .iter()
                .any(|c| c.contains("ctx") && c.contains(" hook"))
        });
        if arr.len() != before {
            changed = true;
        }
    }
    if let Some(arr) = hooks.get_mut("Stop").and_then(|a| a.as_array_mut()) {
        let before = arr.len();
        arr.retain(|entry| {
            !hook_commands_from_entry(entry)
                .iter()
                .any(|c| c.contains("gain --brief"))
        });
        if arr.len() != before {
            changed = true;
        }
    }
    changed
}

/// Cursor IDE global storage database (MCP server registry cache lives here).
pub fn cursor_state_vscdb_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    #[cfg(target_os = "macos")]
    {
        let p = home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let p = home.join(".config/Cursor/User/globalStorage/state.vscdb");
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(app) = dirs::data_dir() {
            let p = app.join("Cursor/User/globalStorage/state.vscdb");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Drop `user-ctx` from Cursor's cached MCP server id list. Returns `Ok(true)` when a row was updated.
pub fn remove_user_ctx_from_cursor_known_mcp_ids() -> Result<bool> {
    use rusqlite::OptionalExtension;
    let Some(db_path) = cursor_state_vscdb_path() else {
        return Ok(false);
    };
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    let val: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            ["mcpService.knownServerIds"],
            |r| r.get(0),
        )
        .optional()?;
    let Some(json) = val else {
        return Ok(false);
    };
    let Ok(mut arr) = serde_json::from_str::<Vec<String>>(&json) else {
        return Ok(false);
    };
    let before = arr.len();
    arr.retain(|s| s != "user-ctx");
    if arr.len() == before {
        return Ok(false);
    }
    let new_json = serde_json::to_string(&arr)?;
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
        rusqlite::params!["mcpService.knownServerIds", new_json],
    )?;
    Ok(true)
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

/// True when Claude Code project JSONL logs exist under `~/.claude/projects/`.
pub fn claude_projects_has_jsonl() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let projects_dir = home.join(".claude").join("projects");
    let Ok(entries) = std::fs::read_dir(&projects_dir) else {
        return false;
    };
    for proj in entries.flatten() {
        let p = proj.path();
        if !p.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&p) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".jsonl") && !name.contains("compact") {
                return true;
            }
        }
    }
    false
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

/// How ctx filters MCP tools in Claude Code settings.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum FilterMode {
    /// Hide stripped-server tools via `permissions.deny`; MCP servers stay connected.
    #[default]
    Soft,
    /// Block non-allowlisted servers via `allowedMcpServers` (maximum savings, connectors drop).
    Strict,
    /// No ctx-managed filter rules.
    Off,
}

impl FilterMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "soft" => Some(Self::Soft),
            "strict" => Some(Self::Strict),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Strict => "strict",
            Self::Off => "off",
        }
    }
}

/// MITM proxy operating mode (`ctx proxy install --mode`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Hooks + soft filter only; proxy not wired (default).
    #[default]
    Off,
    /// MITM filters request bodies; hooks own profile, inject, coach, trace.
    Complement,
    /// MITM runs full gate pipeline; install strips ctx hooks/deny.
    Standalone,
    /// MITM filters tools + analytics only (hooks optional).
    FilterOnly,
}

impl ProxyMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "off" => Some(Self::Off),
            "complement" => Some(Self::Complement),
            "standalone" => Some(Self::Standalone),
            "filter_only" => Some(Self::FilterOnly),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Complement => "complement",
            Self::Standalone => "standalone",
            Self::FilterOnly => "filter_only",
        }
    }

    pub fn mitm_active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub active_profile: Option<String>,
    /// MCP filter strategy written to ~/.claude/settings.json (default: soft).
    #[serde(default)]
    pub filter_mode: FilterMode,
    /// Server IDs or tool prefixes temporarily un-denied for the current session(s).
    #[serde(default)]
    pub session_expansion: Vec<String>,
    pub proxy_port: Option<u16>,
    /// Port for `ctx dashboard` (used by filter.js POST /api/ingest-request).
    #[serde(default)]
    pub dashboard_port: Option<u16>,
    pub proxy_upstream: Option<String>,
    /// MITM mode: off | complement | standalone | filter_only.
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    /// Deprecated; migrated to `proxy_mode` on load (legacy: native_hooks, mitm, reverse).
    #[serde(default, skip_serializing)]
    pub proxy_install_mode: Option<String>,
    pub original_base_url: Option<String>,
    #[serde(default = "default_true")]
    pub auto_profile_enabled: bool,
    #[serde(default = "default_true")]
    pub inject_enabled: bool,
    /// When true, `UserPromptSubmit` reads session JSONL and injects coaching via `additionalContext`.
    #[serde(default = "default_true")]
    pub coaching_enabled: bool,
    /// When true, append `adaptive_prefix.md` (from session index) after the static system prefix.
    #[serde(default = "default_true")]
    pub adaptive_prefix_enabled: bool,
    /// Override max character budget for the adaptive block (default: model-based, max 2000).
    #[serde(default)]
    pub adaptive_prefix_max_chars: Option<usize>,
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
    /// Per-feature A/B ratios. Omitted features default to 100 (always on).
    #[serde(default)]
    pub ab_test: Option<AbTestConfig>,
    /// When true, dashboard shows the Experiment tab (also via `?dev=1` or localStorage).
    #[serde(default)]
    pub dev_mode: bool,
    /// Named preset bundling profile + inject + coaching + adaptive toggles.
    #[serde(default)]
    pub modes: HashMap<String, ModeConfig>,
    /// Active mode name (when set via `ctx mode` or dashboard).
    #[serde(default)]
    pub active_mode: Option<String>,
    /// When true, apply self-tuning recommendations after ingest (disables features with no A/B benefit).
    #[serde(default)]
    pub auto_apply_recommendations: bool,
    /// Gates for usage-based personal / category profile generation.
    #[serde(default)]
    pub profile_thresholds: ProfileThresholds,
    /// Minimum vote-share for the winning profile (0–1) when multiple profiles compete.
    #[serde(default = "default_similarity_min_confidence")]
    pub similarity_min_confidence: f32,
    /// Minimum mean embedding similarity among sessions that voted for the winner (0–1).
    #[serde(default = "default_similarity_min_avg_match")]
    pub similarity_min_avg_match: f32,
    /// When true, derive per-prompt tool overlay from similar session embeddings.
    #[serde(default)]
    pub semantic_tool_mix_enabled: bool,
    #[serde(default = "default_semantic_tool_mix_min_similarity")]
    pub semantic_tool_mix_min_similarity: f32,
    #[serde(default = "default_semantic_tool_mix_min_neighbor_fraction")]
    pub semantic_tool_mix_min_neighbor_fraction: f32,
    #[serde(default = "default_semantic_tool_mix_top_k")]
    pub semantic_tool_mix_top_k: usize,
    /// Semantic overlay from the latest hook (refreshed each UserPromptSubmit).
    #[serde(default)]
    pub session_semantic_tools: Vec<String>,
}

/// Thresholds for automatic profile generation from MCP usage history.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProfileThresholds {
    #[serde(default = "default_min_tool_invocations")]
    pub min_tool_invocations: u32,
    #[serde(default = "default_min_distinct_servers")]
    pub min_distinct_servers: u32,
    #[serde(default = "default_min_sessions_with_mcp")]
    pub min_sessions_with_mcp: u32,
    #[serde(default = "default_lookback_days")]
    pub lookback_days: u32,
    #[serde(default = "default_min_tool_invocations_categories")]
    pub min_tool_invocations_categories: u32,
    /// Minimum invocation count to include a tool in auto-generated `keep_tools` lists.
    #[serde(default = "default_min_tool_invocations_per_tool")]
    pub min_tool_invocations_per_tool: u32,
}

impl Default for ProfileThresholds {
    fn default() -> Self {
        Self {
            min_tool_invocations: default_min_tool_invocations(),
            min_distinct_servers: default_min_distinct_servers(),
            min_sessions_with_mcp: default_min_sessions_with_mcp(),
            lookback_days: default_lookback_days(),
            min_tool_invocations_categories: default_min_tool_invocations_categories(),
            min_tool_invocations_per_tool: default_min_tool_invocations_per_tool(),
        }
    }
}

fn default_min_tool_invocations() -> u32 {
    20
}
fn default_min_distinct_servers() -> u32 {
    3
}
fn default_min_sessions_with_mcp() -> u32 {
    2
}
fn default_lookback_days() -> u32 {
    30
}
fn default_min_tool_invocations_categories() -> u32 {
    80
}
fn default_min_tool_invocations_per_tool() -> u32 {
    3
}
fn default_similarity_min_confidence() -> f32 {
    0.35
}
fn default_similarity_min_avg_match() -> f32 {
    0.5
}
fn default_semantic_tool_mix_min_similarity() -> f32 {
    0.75
}
fn default_semantic_tool_mix_min_neighbor_fraction() -> f32 {
    0.6
}
fn default_semantic_tool_mix_top_k() -> usize {
    10
}

/// One context mode: profile + feature toggles.
#[derive(Serialize, Deserialize, Clone)]
pub struct ModeConfig {
    pub profile: String,
    #[serde(default = "default_true")]
    pub inject_enabled: bool,
    #[serde(default = "default_true")]
    pub coaching_enabled: bool,
    #[serde(default = "default_true")]
    pub adaptive_prefix_enabled: bool,
}

/// Per-feature A/B percentages (0 = always control, 100 = always treatment).
#[derive(Serialize, Deserialize, Clone)]
pub struct AbTestConfig {
    #[serde(default = "default_hundred")]
    pub profile_pct: u8,
    #[serde(default = "default_hundred")]
    pub inject_pct: u8,
    #[serde(default = "default_hundred")]
    pub adaptive_pct: u8,
    #[serde(default = "default_hundred")]
    pub coaching_pct: u8,
}

fn default_hundred() -> u8 {
    100
}

fn default_true() -> bool {
    true
}

impl Default for AbTestConfig {
    fn default() -> Self {
        Self {
            profile_pct: 100,
            inject_pct: 100,
            adaptive_pct: 100,
            coaching_pct: 100,
        }
    }
}

impl AbTestConfig {
    pub fn effective() -> Self {
        Config::load().ab_test.clone().unwrap_or_default()
    }
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
        let mut cfg = if !path.exists() {
            Self {
                filter_mode: FilterMode::Soft,
                auto_profile_enabled: true,
                inject_enabled: true,
                coaching_enabled: true,
                adaptive_prefix_enabled: true,
                ..Default::default()
            }
        } else {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_else(|| Self {
                    auto_profile_enabled: true,
                    inject_enabled: true,
                    coaching_enabled: true,
                    adaptive_prefix_enabled: true,
                    ..Default::default()
                })
        };
        cfg.migrate_proxy_mode();
        cfg
    }

    /// Map legacy `proxy_install_mode` strings to `proxy_mode` when unset.
    fn migrate_proxy_mode(&mut self) {
        if self.proxy_mode != ProxyMode::Off {
            return;
        }
        let Some(ref old) = self.proxy_install_mode else {
            return;
        };
        self.proxy_mode = match old.as_str() {
            "complement" | "mitm" => ProxyMode::Complement,
            "standalone" => ProxyMode::Standalone,
            "filter_only" => ProxyMode::FilterOnly,
            // native_hooks, reverse, node_inject, unknown → off
            _ => ProxyMode::Off,
        };
    }

    pub fn save(&self) -> Result<()> {
        ensure_dir()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(ctx_dir().join("config.toml"), content)?;
        Ok(())
    }
}

#[cfg(test)]
mod hook_strip_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_removes_ctx_hook_and_gain_brief() {
        let mut doc = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": "/home/x/.local/bin/ctx hook" }]
                    }
                ],
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "/home/x/.cargo/bin/ctx gain --brief" }] }
                ]
            }
        });
        assert!(strip_ctx_managed_hooks_from_settings(&mut doc));
        let hooks = doc["hooks"].as_object().unwrap();
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 0);
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 0);
    }
}
