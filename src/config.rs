use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CTX_OWNERSHIP_MARKER: &str = ".ctx-owned-state";

pub fn ctx_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CTX_HOME") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ctx")
}

pub fn ensure_dir() -> Result<()> {
    let directory = ctx_dir();
    std::fs::create_dir_all(&directory)?;
    protect_private_directory(&directory)?;
    let marker = directory.join(CTX_OWNERSHIP_MARKER);
    if !marker.exists() {
        std::fs::write(&marker, "CTX owns this state directory.\n")?;
    }
    protect_private_file(&marker)?;
    Ok(())
}

pub fn protect_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn protect_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if path.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The user home used to resolve `~/.claude`, `~/.cursor`, and similar agent-config paths, with a
/// test-only override. `dirs::home_dir()` reads the Win32 known-folder API and ignores `HOME`, so
/// integration tests cannot isolate the real home by setting `HOME` on Windows. `CTX_TEST_HOME`
/// (set by the tests, never in production) redirects these helpers at a temp dir on every
/// platform. Lib unit tests keep using `CTX_HOME` under `cfg(test)`.
pub fn home_dir_for_paths() -> Option<PathBuf> {
    #[cfg(test)]
    if let Ok(p) = std::env::var("CTX_HOME") {
        return Some(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("CTX_TEST_HOME") {
        return Some(PathBuf::from(p));
    }
    dirs::home_dir()
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

/// Local learned outcome model (Act 1). Per repo/profile, retrained on ingest.
pub fn retention_model_path() -> PathBuf {
    ctx_dir().join("retention-model.json")
}

/// Append-only history of trained model versions, for the Improving dashboard view.
pub fn retention_model_history_path() -> PathBuf {
    ctx_dir().join("retention-model-history.jsonl")
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
    #[cfg(windows)]
    {
        statusline_bin_dir().join("ctx-statusline.ps1")
    }
    #[cfg(not(windows))]
    {
        statusline_bin_dir().join("ctx-statusline.sh")
    }
}

/// Write the ctx-managed Claude Code statusLine script (allowance bridge). PowerShell on Windows
/// (no bash), a POSIX shell script elsewhere.
pub fn install_statusline_script(dashboard_port: u16) -> Result<()> {
    ensure_dir()?;
    let dir = statusline_bin_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(windows)]
    let script = include_str!("../scripts/ctx-statusline.ps1");
    #[cfg(not(windows))]
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

/// Like [`write_json_atomic`], but skips the write entirely when the pretty-printed value already
/// matches what is on disk. Returns true if it wrote, false if it skipped.
///
/// This is the cache-safety guard (CTX-28). In soft filter mode the UserPromptSubmit hook resyncs
/// `~/.claude/settings.json` on every prompt, so without this an unchanged deny set would still
/// rewrite the file each turn. The rewrite is cache-neutral when the bytes match, but skipping it
/// makes the invariant explicit and code-enforced: the cached `tools` prefix only ever changes when
/// the effective tool set genuinely changes, never as a side effect of a no-op resync. Serialization
/// is deterministic for a given value, and the prior file was written by the same serializer, so a
/// logically unchanged document compares equal. If anything differs (or the file is unreadable) we
/// fall through and write, so the guard can never leave stale content.
pub fn write_json_atomic_if_changed(
    path: &std::path::Path,
    value: &serde_json::Value,
) -> Result<bool> {
    let next = serde_json::to_string_pretty(value)?;
    if let Ok(current) = std::fs::read_to_string(path) {
        if current == next {
            return Ok(false);
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &next)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
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
    let home = home_dir_for_paths()?;
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
    // Test isolation via home_dir_for_paths(): never touch the real ~/.claude in tests. Some tests
    // exercise settings writers (sync, friction recovery) that would otherwise clobber the live
    // PostToolUse collection hook.
    home_dir_for_paths()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

/// Path to `~/.claude.json`, the user-scope Claude Code config that holds `mcpServers`. Distinct
/// from `claude_settings_path` (which is `~/.claude/settings.json`, hooks and permissions).
pub fn claude_json_path() -> PathBuf {
    home_dir_for_paths()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude.json")
}

/// True when `~/.claude/settings.json` exists (Claude Code CLI or prior ctx setup).
pub fn claude_code_cli_present() -> bool {
    claude_settings_path().is_file()
}

/// Path to the user-level Cursor hooks file (`~/.cursor/hooks.json`), where ctx registers its
/// live Cursor postToolUse hook (ADR 0018).
pub fn cursor_hooks_path() -> PathBuf {
    home_dir_for_paths()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cursor")
        .join("hooks.json")
}

/// True when Claude Code project JSONL logs exist under `~/.claude/projects/`.
pub fn claude_projects_has_jsonl() -> bool {
    let Some(home) = home_dir_for_paths() else {
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
    let home = home_dir_for_paths()?;
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

/// User-facing compression preset. During the Act 0 collection window the default is
/// `off`: ctx records the decision it *would* make in shadow mode but never modifies
/// tool output. Activation moves to `safe` then `full` only once labels prove it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompressPreset {
    /// No user-facing compression. Shadow collection still runs (zero UX risk).
    #[default]
    Off,
    /// Trim git, test, and grep output only. The proven-safe-first set.
    Safe,
    /// Trim every supported tool output, including Read and MCP.
    Full,
}

impl CompressPreset {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" => Some(Self::Off),
            "safe" => Some(Self::Safe),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Safe => "safe",
            Self::Full => "full",
        }
    }

    /// Whether this preset permits user-facing compression for a compress kind label
    /// (the labels emitted by `compress::shadow::kind_str`).
    pub fn allows_kind(self, kind: &str) -> bool {
        match self {
            Self::Off => false,
            Self::Safe => matches!(
                kind,
                "git-status" | "git-diff" | "git-log" | "test" | "grep"
            ),
            Self::Full => true,
        }
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
    /// MCP server prefixes the developer pruned from the tool menu (CTX-64). Persistent, unlike
    /// `session_expansion`: a pruned server stays hidden across sessions until un-pruned or reached
    /// for. In soft mode this adds a server wildcard to `permissions.deny`; a reach re-adds the
    /// server for the session via `session_expansion`, which overrides this list.
    #[serde(default)]
    pub pruned_servers: Vec<String>,
    /// Port for `ctx dashboard` (used by filter.js POST /api/ingest-request).
    #[serde(default)]
    pub dashboard_port: Option<u16>,
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
    /// When false, experiment pre-ctx phase: strip intervention hooks and filters (ingest only).
    #[serde(default = "default_true")]
    pub experiment_hooks_enabled: bool,
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
    /// When true, the PostToolUse hook runs at all (master switch for shadow + apply).
    #[serde(default = "default_true")]
    pub compress_enabled: bool,
    /// User-facing compression preset (off | safe | full). Default off during collection.
    #[serde(default)]
    pub compress_preset: CompressPreset,
    /// When true, the hook records the would-do retention decision for every tool result
    /// into `compress_decisions` (Act 0 self-labeling). Independent of `compress_preset`.
    #[serde(default = "default_true")]
    pub compress_shadow_enabled: bool,
    /// Bypass the Act 1 evidence gate and activate any preset-allowed tool immediately.
    /// Off by default: activation is earned from the user's own labels.
    #[serde(default)]
    pub compress_force_active: bool,
    /// Deliberate before/after trial (SAU-150). Tool names in this list are trimmed live
    /// even while `compress_preset` stays off, so a single tool can be measured (trimmed vs
    /// baseline) without the evidence gate, which cannot pass before any trimmed data exists.
    /// This is the only way to generate the "after" arm. Additive: each tool's comparison is scored
    /// on its own runs, so any number of tools can build their trimmed arms at once.
    #[serde(default)]
    pub compress_trial_tools: Vec<String>,
    /// Automatic bounded burn-in (ADR 0012 / CTX-23). When on, a compressible tool starts trimming
    /// on its own once it has a solid clean baseline arm but no trimmed arm yet, building the
    /// "after" arm the causal gate needs. This is the autopilot on-ramp that replaces the
    /// hand-written `compress_trial_tools` list: burn-in respects the preset (never trims when
    /// autopilot is off), is bounded to `min_trimmed` runs, and hands off to the causal gate, which
    /// keeps clean tools trimming and stops harmful ones. Default on. A bad trim only costs a
    /// re-read, never the underlying data.
    #[serde(default = "default_true")]
    pub compress_auto_trial: bool,
    /// Edit-intent guard for Read (ADR 0001 / CTX-8). When on, a Read is only trim-eligible if it
    /// is a reference read (a file the agent is not positioned to edit); working reads of editable
    /// project files are never trimmed, even under a trial or after the activation gate. Default on;
    /// turning it off is an experiment knob to measure how much harm the guard prevents.
    #[serde(default = "default_true")]
    pub compress_read_edit_guard: bool,
    /// Thinking-intent signal for Read (ADR 0004 / CTX-11). When on, the controller reads the
    /// agent's most recent extended-thinking from the session transcript and protects a Read the
    /// static guard would trim if that thinking shows edit-intent for the file. Claude Code only;
    /// purely protective (it never trims more). The signal is also recorded in shadow features so
    /// its real-world prevalence can be measured before it is relied on.
    #[serde(default = "default_true")]
    pub compress_intent_log: bool,
    /// Randomized exploration rate for Phase 2 per-decision proof (ADR 0009 / CTX-15). On each
    /// trim-eligible decision (a tool that would actually drop lines under a trial or after
    /// activation), with this probability ctx leaves the output untrimmed and tags it as a control
    /// sample. The rest are tagged treatment. Comparing the two arms gives an unbiased, per-tool
    /// causal estimate of trimming on the user's own work, which is the only honest way to let the
    /// model earn slices later. Cost is forgone savings on the control fraction, never added risk.
    /// 0.0 disables exploration. Default 0.20.
    #[serde(default = "default_explore_rate")]
    pub compress_explore_rate: f64,
    /// Randomized exploration rate for Read decisions only (ADR 0009 / CTX-15). Re-enabled now that
    /// path-role logging is live so the observational needed-whole target can get a causal check on
    /// reads without turning exploration back on for every tool. Default 0.20. Other tools still use
    /// `compress_explore_rate`, which stays off by default.
    #[serde(default = "default_explore_read_rate")]
    pub compress_explore_read_rate: f64,
    /// Let the file-aware retention model propose a trim for a working read the static guard would
    /// hold back (ADR 0032 / CTX-46 increment 3). Default OFF. Even when on, the model can only
    /// *propose*: the proposed read still has to clear the same preset, burn-in, and causal
    /// activation gate as any other trim, and a model score alone can never make a trim apply. The
    /// proposal is also confined to repos that have enough of their own labels and to a model that
    /// has beaten the kind-only twin on holdout AUC. Off until the per-repo signal is proven.
    #[serde(default)]
    pub compress_model_propose: bool,
    /// Session-grounded retention (v2): score lines by task frame after v1 format pass.
    #[serde(default)]
    pub compress_sgr_enabled: bool,
    /// Cross-turn dedup for identical tool output blocks (v2.1).
    #[serde(default = "default_true")]
    pub compress_sgr_dedup: bool,
    /// Adjust compress target by debug/scan mode (v2).
    #[serde(default = "default_true")]
    pub compress_adaptive_budget: bool,
    /// Only compress when raw output exceeds this many chars.
    #[serde(default = "default_compress_max_output_chars")]
    pub compress_max_output_chars: usize,
    /// Target size after compression.
    #[serde(default = "default_compress_target_chars")]
    pub compress_target_chars: usize,
    /// Built-in tool names eligible for compression (MCP tools always eligible when enabled).
    #[serde(default = "default_compress_tools")]
    pub compress_tools: Vec<String>,
    #[serde(default = "default_true")]
    pub compress_redact_secrets: bool,
    #[serde(default = "default_true")]
    pub compress_preserve_errors: bool,
    /// SPIKE (exploratory, off by default): let the earn-it gate govern every tool instead of the
    /// static `compress_tools` allow-list. When true, `tool_allowed` treats any tool as eligible
    /// except those in the deny-set. The preset / burn-in / causal-activation gate in `agent::decide`
    /// still decides whether an eligible tool actually trims, so this only widens the pool of tools
    /// that can *earn* a trim; it does not force any trim. Defaults on: earn-it governs every tool
    /// except the deny-set (recovery tools and mutations). Set false to fall back to the allow-list.
    #[serde(default = "default_true")]
    pub compress_trim_all: bool,
    /// Tools that are never trimmed, in either mode. ctx's own server (`mcp__ctx__*`) is always
    /// denied in code because it holds the recovery tools (ctx_expand); this list is the
    /// configurable extension and its default spells out that recovery surface as a safety net.
    #[serde(default = "default_compress_deny_tools")]
    pub compress_deny_tools: Vec<String>,
    /// Maximum number of verbatim originals retained for one-command recovery.
    #[serde(default = "default_rewind_retention_entries")]
    pub rewind_retention_entries: usize,
    /// Maximum combined bytes of verbatim original/trimmed text retained locally.
    #[serde(default = "default_rewind_retention_bytes")]
    pub rewind_retention_bytes: u64,
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
// Recall-first defaults tuned on the real corpus (CTX-65 / M-C): a leave-one-out kNN over session
// embeddings covered 96% of the servers a held-out session actually used at K=5 while cutting the
// menu ~37%. A missed tool is a reach that access-friction re-adds (CTX-66), so we bias toward
// recall: a low neighbor fraction (union-like) and a mild similarity floor over a tight top-K.
fn default_semantic_tool_mix_min_similarity() -> f32 {
    0.3
}
fn default_semantic_tool_mix_min_neighbor_fraction() -> f32 {
    0.2
}
fn default_semantic_tool_mix_top_k() -> usize {
    5
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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AbTestConfig {
    #[serde(default = "default_hundred")]
    pub profile_pct: u8,
    #[serde(default = "default_hundred")]
    pub inject_pct: u8,
    #[serde(default = "default_hundred")]
    pub adaptive_pct: u8,
    #[serde(default = "default_hundred")]
    pub coaching_pct: u8,
    #[serde(default = "default_hundred")]
    pub compress_pct: u8,
    /// When compress is on, 50/50 v1-only vs v1+SGR retention.
    #[serde(default = "default_hundred")]
    pub compress_sgr_pct: u8,
    /// Semantic tool mix overlay (vector neighbors → un-deny tools per prompt).
    #[serde(default = "default_hundred")]
    pub tool_mix_pct: u8,
}

fn default_hundred() -> u8 {
    100
}

fn default_compress_max_output_chars() -> usize {
    12_000
}

fn default_compress_target_chars() -> usize {
    2_500
}

fn default_compress_tools() -> Vec<String> {
    vec!["Bash".into(), "Read".into(), "Grep".into(), "Glob".into()]
}

/// The never-trim set: ctx's own recovery tools. The `mcp__ctx__` prefix rule in `tool_allowed`
/// already covers the whole server, so this is a redundant safety net that keeps the guarantee for
/// ctx_expand and friends readable in config and intact even if the prefix rule ever changes.
fn default_compress_deny_tools() -> Vec<String> {
    vec![
        "mcp__ctx__ctx_expand".into(),
        "mcp__ctx__ctx_status".into(),
        "mcp__ctx__ctx_waste".into(),
    ]
}

fn default_rewind_retention_entries() -> usize {
    500
}

fn default_rewind_retention_bytes() -> u64 {
    100 * 1024 * 1024
}

fn default_true() -> bool {
    true
}

/// Default randomized-exploration rate. Off (ADR 0012): Phase 2 was shelved because the control
/// arm never gathered enough data to support a per-decision causal claim, while withholding trims
/// cost real savings. The plumbing stays so exploration can be re-enabled deliberately later, but
/// it no longer runs by default. The honest before/after gate in `activation.rs` is what earns a
/// tool now.
fn default_explore_rate() -> f64 {
    0.0
}

fn default_explore_read_rate() -> f64 {
    0.20
}

impl Default for AbTestConfig {
    fn default() -> Self {
        Self {
            profile_pct: 100,
            inject_pct: 100,
            adaptive_pct: 100,
            coaching_pct: 100,
            compress_pct: 100,
            compress_sgr_pct: 100,
            tool_mix_pct: 100,
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

    /// Whether user-facing compression should apply for a compress kind label given the
    /// current preset. Act 1 layers a per-tool evidence gate on top of this via
    /// [`crate::compress::activation`].
    pub fn compress_applies_kind(&self, kind: &str) -> bool {
        self.compress_enabled && self.compress_preset.allows_kind(kind)
    }

    /// Whether this exact tool is under a deliberate trim trial (SAU-150). A trialed tool is
    /// trimmed live regardless of preset and the evidence gate, so we can collect the "after"
    /// arm of the causal before/after. Still requires `compress_enabled`.
    pub fn compress_trialing(&self, tool_name: &str) -> bool {
        self.compress_enabled && self.compress_trial_tools.iter().any(|t| t == tool_name)
    }

    pub fn load() -> Self {
        let path = ctx_dir().join("config.toml");
        let mut cfg = if !path.exists() {
            Self {
                // Profile filtering is off by default (CTX-43, ADR 0027). It is the one pillar ctx
                // never proved safe on the user's own work, it saved ~nothing in practice, and it
                // could strip a tool the agent then needed. It stays available as an opt-in
                // (`ctx use <profile>` / `ctx filter mode soft`), just not the shipped default.
                filter_mode: FilterMode::Off,
                active_profile: Some("all".to_string()),
                auto_profile_enabled: false,
                inject_enabled: true,
                coaching_enabled: true,
                adaptive_prefix_enabled: true,
                compress_enabled: true,
                // Act 0 collection is on by default: it never changes tool output, it only
                // records the would-do decision so the system can learn. The derived
                // `Default` would leave this false, which silently disables all learning.
                compress_shadow_enabled: true,
                // Safety guard on by default (ADR 0001): never trim a Read the agent may edit.
                compress_read_edit_guard: true,
                // Thinking-intent signal on by default (ADR 0004): protective and self-measuring.
                compress_intent_log: true,
                // Automatic burn-in on by default (ADR 0012): the autopilot on-ramp that lets tools
                // earn without a hand-written trial list.
                compress_auto_trial: true,
                // Exploration off by default (ADR 0012): Phase 2 was shelved; plumbing kept, idle.
                compress_explore_rate: default_explore_rate(),
                // Read-only exploration on by default now that path-role logging is live (CTX-45).
                compress_explore_read_rate: default_explore_read_rate(),
                experiment_hooks_enabled: true,
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
                    compress_enabled: true,
                    compress_shadow_enabled: true,
                    compress_read_edit_guard: true,
                    compress_intent_log: true,
                    compress_auto_trial: true,
                    compress_explore_rate: default_explore_rate(),
                    compress_explore_read_rate: default_explore_read_rate(),
                    experiment_hooks_enabled: true,
                    ..Default::default()
                })
        };
        cfg.migrate_compress_defaults();
        if cfg.migrate_experiment_hooks_enabled() {
            let _ = cfg.save();
        }
        cfg
    }

    /// Fresh installs used `Default` for bool fields (false). Re-enable hooks when no experiment plan is active.
    fn migrate_experiment_hooks_enabled(&mut self) -> bool {
        if self.experiment_hooks_enabled {
            return false;
        }
        if crate::experiment_plan::plan_path().exists() {
            return false;
        }
        self.experiment_hooks_enabled = true;
        true
    }

    /// Repair compress fields written as zero/empty by `Default` before serde defaults applied on save.
    fn migrate_compress_defaults(&mut self) {
        if self.compress_max_output_chars == 0
            || self.compress_target_chars == 0
            || self.compress_tools.is_empty()
        {
            self.compress_max_output_chars = default_compress_max_output_chars();
            self.compress_target_chars = default_compress_target_chars();
            self.compress_tools = default_compress_tools();
            self.compress_redact_secrets = true;
            self.compress_preserve_errors = true;
        }
        if self.rewind_retention_entries == 0 {
            self.rewind_retention_entries = default_rewind_retention_entries();
        }
        if self.rewind_retention_bytes == 0 {
            self.rewind_retention_bytes = default_rewind_retention_bytes();
        }
    }

    pub fn save(&self) -> Result<()> {
        ensure_dir()?;
        let content = toml::to_string_pretty(self)?;
        let path = ctx_dir().join("config.toml");
        std::fs::write(&path, content)?;
        protect_private_file(&path)?;
        Ok(())
    }
}

#[cfg(test)]
mod hook_strip_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrate_reenables_hooks_without_experiment_plan() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let mut cfg = Config {
            experiment_hooks_enabled: false,
            ..Default::default()
        };
        assert!(cfg.migrate_experiment_hooks_enabled());
        assert!(cfg.experiment_hooks_enabled);
    }

    #[test]
    fn fresh_install_enables_shadow_collection() {
        // A fresh config (no file on disk) must have Act 0 shadow collection on, otherwise
        // the system silently records nothing and never learns. The derived `Default`
        // leaves bools false, so this is a real regression guard.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let cfg = Config::load();
        assert!(
            cfg.compress_shadow_enabled,
            "fresh install must collect shadow decisions by default"
        );
    }

    #[test]
    fn fresh_install_ships_with_filtering_off() {
        // Profile filtering is deprecated as a default (CTX-43, ADR 0027): a fresh install must
        // not strip any MCP tools. It stays available as an opt-in, just not on out of the box.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let cfg = Config::load();
        assert_eq!(
            cfg.filter_mode,
            FilterMode::Off,
            "fresh install must not filter MCP tools"
        );
        assert_eq!(cfg.active_profile.as_deref(), Some("all"));
        assert!(
            !cfg.auto_profile_enabled,
            "auto-profile selection stays off so filtering never turns itself on"
        );
    }

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

#[cfg(test)]
mod cache_safe_write_tests {
    use super::*;
    use serde_json::json;

    // CTX-28: the soft-mode hook resyncs settings every prompt. The guard must not touch the file
    // when the content is unchanged, so the cached tools prefix stays byte-stable.
    #[test]
    fn unchanged_value_is_not_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let doc = json!({ "permissions": { "deny": ["mcp__a__*", "mcp__b__*"] } });

        assert!(
            write_json_atomic_if_changed(&path, &doc).unwrap(),
            "first write must create the file"
        );
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();

        // Re-serializing the same logical document must compare equal and skip the write, leaving the
        // file (and its mtime) untouched. We rebuild the value via a round-trip to mimic the hook,
        // which reads the file, re-applies rules, and serializes again.
        let reparsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            !write_json_atomic_if_changed(&path, &reparsed).unwrap(),
            "an identical resync must be skipped"
        );
        let second = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(first, second, "skipped write must not touch the file mtime");
    }

    #[test]
    fn genuine_change_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");

        let before = json!({ "permissions": { "deny": ["mcp__a__*"] } });
        assert!(write_json_atomic_if_changed(&path, &before).unwrap());

        // A real tool-set change (different deny set) must be persisted, otherwise filtering would
        // silently stop reflecting the active profile.
        let after = json!({ "permissions": { "deny": ["mcp__a__*", "mcp__b__*"] } });
        assert!(
            write_json_atomic_if_changed(&path, &after).unwrap(),
            "a changed deny set must be written"
        );
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["permissions"]["deny"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn writes_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let doc = json!({ "permissions": { "deny": [] } });
        assert!(
            write_json_atomic_if_changed(&path, &doc).unwrap(),
            "missing file must be created"
        );
        assert!(path.exists());
    }
}
