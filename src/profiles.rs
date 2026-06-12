use anyhow::{bail, Result};
use colored::Colorize;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::config::{Config, ProfileThresholds};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Profile {
    pub display: String,
    pub description: String,
    /// MCP tool name prefixes to keep. Empty = keep everything (no filtering) when `keep_tools` is also empty.
    pub keep: Vec<String>,
    /// Explicit MCP tool names to keep. When non-empty, overrides server-prefix `keep` for filtering.
    #[serde(default)]
    pub keep_tools: Vec<String>,
    /// Fragments matched against the working directory path (case-insensitive).
    /// This is the primary auto-select signal -- more reliable than text keywords.
    #[serde(default)]
    pub path_patterns: Vec<String>,
    /// Fallback: keywords in the system prompt text (case-insensitive).
    /// Only used when no cwd can be extracted.
    #[serde(default)]
    pub triggers: Vec<String>,
}

// Observed tool counts per MCP server in this Claude Code environment
pub const SERVER_COUNTS: &[(&str, usize)] = &[
    ("mcp__claude_ai_Atlassian__", 22),
    ("mcp__claude_ai_Figma__", 21),
    ("mcp__claude_ai_Data_Shippo__", 25),
    ("mcp__claude_ai_Fullstory__", 18),
    ("mcp__claude_ai_Slack__", 15),
    ("mcp__claude_ai_Gmail__", 10),
    ("mcp__claude_ai_Google_Drive__", 7),
    ("mcp__claude_ai_AWS_Marketplace__", 5),
    ("mcp__claude_ai_Google_Calendar__", 2),
    ("mcp__claude_ai_Linear__", 2),
    ("mcp__claude_ai_Shippo_MCP_Dev__", 3),
    ("mcp__claude_ai_Shippo_MCP_DEV_QA__", 3),
    ("mcp__claude_ai_Adobe_Marketing_Agent__", 2),
    ("mcp__claude_ai_Canva__", 2),
    ("mcp__claude_ai_Clay__", 2),
    ("mcp__claude_ai_Cloudflare_Developer_Platform__", 2),
    ("mcp__claude_ai_Docusign__", 2),
    ("mcp__claude_ai_Fireflies__", 2),
    ("mcp__claude_ai_Incident_io__", 2),
    ("mcp__claude_ai_Intuit_Mailchimp__", 2),
    ("mcp__claude_ai_Microsoft_365__", 2),
    ("mcp__claude_ai_Miro__", 2),
    ("mcp__claude_ai_Moody_s__", 2),
    ("mcp__claude_ai_NetSuite__", 2),
    ("mcp__claude_ai_NetSuite_Sandbox__", 2),
    ("mcp__claude_ai_Notion__", 2),
    ("mcp__claude_ai_Postman__", 2),
    ("mcp__claude_ai_Ramp__", 2),
    ("mcp__claude_ai_Stripe__", 2),
    ("mcp__claude_ai_Todo__", 2),
    ("mcp__claude_ai_Tropic__", 2),
    ("mcp__claude_ai_Webflow__", 2),
    ("mcp__claude_ai_Zapier__", 2),
    ("mcp__claude_ai_ZoomInfo__", 2),
    ("mcp__claude_ai_Zoom_for_Claude__", 2),
];

pub const TOTAL_TOOLS: usize = 156;
const TOKENS_PER_TOOL: usize = 600;

/// Slugs written by `generate_from_config` — pruned on regenerate when no longer observed.
const AUTO_GENERATED_PROFILE_SLUGS: &[&str] = &[
    "data", "design", "work", "finance", "files", "infra", "comms", "other", "shippo",
];

/// Built-in template slugs shipped in code (hidden from list until usage profiles exist).
const BUILTIN_TEMPLATE_SLUGS: &[&str] = &["carrier", "data", "design", "minimal"];

/// Whether a profile should appear in `ctx profile list` and the dashboard.
pub fn is_profile_visible(slug: &str, active: &str) -> bool {
    if slug == "all" {
        return true;
    }
    if slug == active {
        return true;
    }
    let custom = slugs_from_profiles_toml();
    if custom.contains(slug) {
        return true;
    }
    // Hide code templates until the user has usage-generated profiles in profiles.toml.
    if custom.is_empty() && !has_observed_mcp_history() {
        return false;
    }
    !BUILTIN_TEMPLATE_SLUGS.contains(&slug)
}

/// Sorted slugs for list/dashboard display.
pub fn visible_profile_slugs(active: &str) -> Vec<String> {
    let mut slugs: Vec<String> = load_all()
        .keys()
        .filter(|s| is_profile_visible(s, active))
        .cloned()
        .collect();
    slugs.sort_by(|a, b| {
        generated_profile_sort_key(a)
            .cmp(&generated_profile_sort_key(b))
            .then(a.cmp(b))
    });
    slugs
}

fn lookback_cutoff(days: u32) -> String {
    (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339()
}

fn observed_tools_with_lookback(lookback_days: u32) -> Vec<crate::db::ObservedToolRow> {
    let Ok(conn) = crate::db::open_db() else {
        return Vec::new();
    };
    let _ = crate::db::ensure_schema(&conn);
    crate::db::observed_tools(&conn, &lookback_cutoff(lookback_days)).unwrap_or_default()
}

fn tools_for_personal(lookback_days: u32, min_invocations: u32) -> Vec<String> {
    let observed = observed_tools_with_lookback(lookback_days);
    let mut keep: HashSet<String> = observed
        .iter()
        .filter(|r| r.count >= min_invocations as u64)
        .map(|r| r.tool_name.clone())
        .collect();

    let rule_signals = crate::rule_signals::collect_mcp_signals();
    for tool in rule_signals.tool_names {
        keep.insert(tool);
    }
    for prefix in rule_signals.server_prefixes {
        keep.insert(prefix.clone());
        for row in &observed {
            if row.tool_name.starts_with(&prefix) || row.server_prefix.starts_with(&prefix) {
                keep.insert(row.tool_name.clone());
            }
        }
    }

    let mut out: Vec<String> = keep.into_iter().collect();
    out.sort();
    out
}

fn tools_for_prefixes(
    prefixes: &[String],
    lookback_days: u32,
    min_invocations: u32,
) -> Vec<String> {
    observed_tools_with_lookback(lookback_days)
        .into_iter()
        .filter(|r| {
            r.count >= min_invocations as u64
                && prefixes.iter().any(|p| {
                    r.server_prefix.starts_with(p.as_str())
                        || p.starts_with(r.server_prefix.as_str())
                })
        })
        .map(|r| r.tool_name)
        .collect()
}

fn tool_count_for_prefix(prefix: &str) -> usize {
    SERVER_COUNTS
        .iter()
        .find(|(k, _)| k.starts_with(prefix) || prefix.starts_with(*k))
        .map(|(_, c)| *c)
        .unwrap_or(3)
}

/// Estimated kept / removed / token savings for a profile slug (hook + dashboard estimates).
pub fn filter_impact_for_slug(slug: &str) -> (usize, usize, usize) {
    let all = load_all();
    let total = dynamic_total_tools();
    if total == 0 {
        return (0, 0, 0);
    }
    if let Some(p) = all.get(slug) {
        let kept = p.tool_count();
        (kept, total.saturating_sub(kept), p.savings_vs_all())
    } else {
        (total, 0, 0)
    }
}

/// Approximate tool schema count offered per MCP server prefix for a profile.
pub fn tools_sent_by_server_prefix(slug: &str) -> HashMap<String, usize> {
    let all = load_all();
    let Some(p) = all.get(slug) else {
        return HashMap::new();
    };
    let lookback = Config::load().profile_thresholds.lookback_days;
    let cutoff = lookback_cutoff(lookback);
    let mut out = HashMap::new();
    if p.uses_tool_level() {
        for tool in &p.keep_tools {
            if let Some(prefix) = crate::filter::server_prefix_from_tool(tool) {
                *out.entry(prefix).or_default() += 1;
            }
        }
        return out;
    }
    for prefix in &p.keep {
        let n = if let Ok(conn) = crate::db::open_db() {
            crate::db::tools_under_prefix(&conn, prefix, &cutoff)
                .map(|t| t.len())
                .unwrap_or(0)
        } else {
            0
        };
        let n = if n > 0 {
            n
        } else {
            tool_count_for_prefix(prefix)
        };
        if n > 0 {
            out.insert(prefix.clone(), n);
        }
    }
    out
}

/// Total distinct MCP tool schemas from indexed usage (0 until ingest records tool names).
pub fn dynamic_total_tools() -> usize {
    let lookback = Config::load().profile_thresholds.lookback_days;
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        if let Ok(n) = crate::db::distinct_observed_tool_count(&conn, &lookback_cutoff(lookback)) {
            if n > 0 {
                return n;
            }
        }
    }
    let prefixes = collect_observed_prefixes();
    if prefixes.is_empty() {
        return 0;
    }
    prefixes.iter().map(|p| tool_count_for_prefix(p)).sum()
}

/// True when tool/token estimates are based on indexed MCP usage, not placeholders.
pub fn tool_metrics_ready() -> bool {
    dynamic_total_tools() > 0
}

/// True when the DB has any MCP usage signal (invoked tools or request server lists).
pub fn has_observed_mcp_history() -> bool {
    !collect_observed_prefixes().is_empty()
}

/// MCP usage metrics over the configured lookback window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageStats {
    pub tool_invocations: u32,
    pub distinct_servers: u32,
    pub sessions_with_mcp: u32,
}

pub fn usage_stats() -> UsageStats {
    let lookback = Config::load().profile_thresholds.lookback_days;
    usage_stats_with_lookback(lookback)
}

pub fn usage_stats_with_lookback(lookback_days: u32) -> UsageStats {
    let mut stats = UsageStats::default();
    if !crate::db::db_exists() {
        return stats;
    }
    let Ok(conn) = crate::db::open_db() else {
        return stats;
    };
    let _ = crate::db::ensure_schema(&conn);
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(lookback_days as i64)).to_rfc3339();

    if let Ok(n) = conn.query_row(
        "SELECT COUNT(*) FROM tool_invocations WHERE ts >= ?1",
        rusqlite::params![cutoff],
        |r| r.get::<_, i64>(0),
    ) {
        stats.tool_invocations = n.max(0) as u32;
    }
    if let Ok(n) = conn.query_row(
        "SELECT COUNT(DISTINCT server_prefix) FROM tool_invocations WHERE ts >= ?1",
        rusqlite::params![cutoff],
        |r| r.get::<_, i64>(0),
    ) {
        stats.distinct_servers = n.max(0) as u32;
    }
    if let Ok(n) = conn.query_row(
        "SELECT COUNT(DISTINCT session_id) FROM tool_invocations WHERE ts >= ?1 AND session_id IS NOT NULL",
        rusqlite::params![cutoff],
        |r| r.get::<_, i64>(0),
    ) {
        stats.sessions_with_mcp = n.max(0) as u32;
    }

    persist_usage_meta(&conn, &stats);
    stats
}

fn persist_usage_meta(conn: &rusqlite::Connection, stats: &UsageStats) {
    let thresholds = Config::load().profile_thresholds;
    let ready = personal_ready_with_thresholds(stats, &thresholds);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('personal_profile_ready', ?1)",
        [if ready { "1" } else { "0" }],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('usage_tool_invocations', ?1)",
        [stats.tool_invocations.to_string()],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('usage_distinct_servers', ?1)",
        [stats.distinct_servers.to_string()],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('usage_sessions_with_mcp', ?1)",
        [stats.sessions_with_mcp.to_string()],
    );
}

pub fn personal_ready(stats: &UsageStats) -> bool {
    personal_ready_with_thresholds(stats, &Config::load().profile_thresholds)
}

pub fn personal_ready_with_thresholds(stats: &UsageStats, t: &ProfileThresholds) -> bool {
    stats.tool_invocations >= t.min_tool_invocations
        && stats.distinct_servers >= t.min_distinct_servers
        && stats.sessions_with_mcp >= t.min_sessions_with_mcp
}

pub fn categories_ready(stats: &UsageStats) -> bool {
    stats.tool_invocations
        >= Config::load()
            .profile_thresholds
            .min_tool_invocations_categories
}

pub fn personal_readiness_json() -> serde_json::Value {
    let stats = usage_stats();
    let t = Config::load().profile_thresholds;
    let ready = personal_ready(&stats);
    serde_json::json!({
        "ready": ready,
        "tool_invocations": stats.tool_invocations,
        "min_tool_invocations": t.min_tool_invocations,
        "distinct_servers": stats.distinct_servers,
        "min_distinct_servers": t.min_distinct_servers,
        "sessions_with_mcp": stats.sessions_with_mcp,
        "min_sessions_with_mcp": t.min_sessions_with_mcp,
        "categories_ready": categories_ready(&stats),
        "min_tool_invocations_categories": t.min_tool_invocations_categories,
        "min_tool_invocations_per_tool": t.min_tool_invocations_per_tool,
    })
}

fn has_category_profiles_in_toml() -> bool {
    slugs_from_profiles_toml()
        .iter()
        .any(|s| AUTO_GENERATED_PROFILE_SLUGS.contains(&s.as_str()))
}

/// Write or update `[personal]` in profiles.toml from MCP tool usage. Returns true when written.
pub fn upsert_personal_from_usage(force: bool) -> Result<bool> {
    let stats = usage_stats();
    if !force && !personal_ready(&stats) {
        return Ok(false);
    }
    let t = Config::load().profile_thresholds;
    let lookback = t.lookback_days;
    let tools = tools_for_personal(lookback, t.min_tool_invocations_per_tool);
    if tools.is_empty() {
        return Ok(false);
    }

    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?).unwrap_or_default()
    } else {
        HashMap::new()
    };
    existing.insert(
        "personal".into(),
        Profile {
            display: "Personal".into(),
            description: format!(
                "Auto-built from MCP usage and standing instructions (last {} days, ≥{} invocations/tool unless rules mention the server)",
                lookback, t.min_tool_invocations_per_tool
            ),
            keep: vec![],
            keep_tools: tools,
            ..Default::default()
        },
    );
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;

    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let ts = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('last_personal_update_at', ?1)",
            [ts],
        );
    }

    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(true)
}

impl Profile {
    pub fn uses_tool_level(&self) -> bool {
        !self.keep_tools.is_empty()
    }

    pub fn filtering_enabled(&self) -> bool {
        self.uses_tool_level() || !self.keep.is_empty()
    }

    pub fn keeps_tool(&self, tool_name: &str) -> bool {
        if !tool_name.starts_with("mcp__") {
            return true;
        }
        if !self.filtering_enabled() {
            return true;
        }
        if self.uses_tool_level() {
            return self
                .keep_tools
                .iter()
                .any(|k| tool_name == k.as_str() || tool_name.starts_with(k.as_str()));
        }
        self.keep
            .iter()
            .any(|prefix| tool_name.starts_with(prefix.as_str()))
    }

    pub fn tool_count(&self) -> usize {
        if !self.filtering_enabled() {
            return dynamic_total_tools();
        }
        if self.uses_tool_level() {
            return self.keep_tools.len();
        }
        let lookback = Config::load().profile_thresholds.lookback_days;
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);
            let cutoff = lookback_cutoff(lookback);
            let mut total = 0usize;
            for prefix in &self.keep {
                if let Ok(tools) = crate::db::tools_under_prefix(&conn, prefix, &cutoff) {
                    if !tools.is_empty() {
                        total += tools.len();
                        continue;
                    }
                }
                total += tool_count_for_prefix(prefix);
            }
            if total > 0 {
                return total;
            }
        }
        self.keep
            .iter()
            .map(|prefix| tool_count_for_prefix(prefix))
            .sum()
    }

    pub fn server_count(&self) -> usize {
        if self.uses_tool_level() {
            let mut prefixes: HashSet<String> = HashSet::new();
            for t in &self.keep_tools {
                if let Some(p) = crate::filter::server_prefix_from_tool(t) {
                    prefixes.insert(p);
                }
            }
            return prefixes.len();
        }
        if self.keep.is_empty() {
            return collect_observed_prefixes().len();
        }
        self.keep.len()
    }

    pub fn token_cost(&self) -> usize {
        self.tool_count() * TOKENS_PER_TOOL
    }

    pub fn savings_vs_all(&self) -> usize {
        (dynamic_total_tools() * TOKENS_PER_TOOL).saturating_sub(self.token_cost())
    }

    pub fn savings_pct(&self) -> f32 {
        let total = dynamic_total_tools() as f32;
        if total <= 0.0 {
            return 0.0;
        }
        1.0 - (self.tool_count() as f32 / total)
    }

    pub fn filters_tool(&self, tool_name: &str) -> bool {
        if !self.filtering_enabled() {
            return false;
        }
        if !tool_name.starts_with("mcp__") {
            return false;
        }
        !self.keeps_tool(tool_name)
    }

    pub fn matches_path(&self, cwd: &str) -> bool {
        if self.path_patterns.is_empty() {
            return false;
        }
        let lower = cwd.to_lowercase();
        self.path_patterns
            .iter()
            .any(|p| lower.contains(p.as_str()))
    }

    pub fn matches_system_prompt(&self, system: &str) -> bool {
        if self.triggers.is_empty() {
            return false;
        }
        let lower = system.to_lowercase();
        self.triggers.iter().any(|t| lower.contains(t.as_str()))
    }
}

fn defaults() -> HashMap<String, Profile> {
    let mut m = HashMap::new();

    m.insert(
        "carrier".into(),
        Profile {
            display: "Carrier Integration".into(),
            description: "Jira, Confluence, Slack, Gmail, Shippo data, Linear".into(),
            keep: vec![
                "mcp__claude_ai_Atlassian__".into(),
                "mcp__claude_ai_Slack__".into(),
                "mcp__claude_ai_Gmail__".into(),
                "mcp__claude_ai_Data_Shippo__".into(),
                "mcp__claude_ai_Linear__".into(),
                "mcp__claude_ai_Shippo_MCP_Dev__".into(),
                "mcp__claude_ai_Shippo_MCP_DEV_QA__".into(),
            ],
            path_patterns: vec![
                "carrier-integrations".into(),
                "carrier_integrations".into(),
                "carrier-platform".into(),
                "carrier_adapter".into(),
                "carrier-specs".into(),
                "ccap".into(),
                "cip".into(),
                "ciqs".into(),
                "ontrac".into(),
                "amazon_shipping".into(),
            ],
            triggers: vec![
                "carrier integration".into(),
                "cif ".into(),
                "shippo carrier".into(),
            ],
            ..Default::default()
        },
    );

    m.insert(
        "design".into(),
        Profile {
            display: "Design".into(),
            keep: vec![
                "mcp__claude_ai_Figma__".into(),
                "mcp__claude_ai_Canva__".into(),
                "mcp__claude_ai_Miro__".into(),
                "mcp__claude_ai_Slack__".into(),
                "mcp__claude_ai_Google_Drive__".into(),
                "mcp__claude_ai_Notion__".into(),
            ],
            description: "Figma, Canva, Miro, Notion, Slack, Google Drive".into(),
            path_patterns: vec![
                "design".into(),
                "frontend".into(),
                "ui-".into(),
                "figma".into(),
                "marketing".into(),
            ],
            triggers: vec!["figma".into(), "design system".into(), "wireframe".into()],
            ..Default::default()
        },
    );

    m.insert(
        "data".into(),
        Profile {
            display: "Data Analysis".into(),
            description: "Shippo data tools, Atlassian, Slack, Gmail".into(),
            keep: vec![
                "mcp__claude_ai_Data_Shippo__".into(),
                "mcp__claude_ai_Atlassian__".into(),
                "mcp__claude_ai_Slack__".into(),
                "mcp__claude_ai_Gmail__".into(),
                "mcp__claude_ai_Shippo_MCP_Dev__".into(),
            ],
            path_patterns: vec![
                "databricks".into(),
                "shippo_py3".into(),
                "reconciliation".into(),
                "analytics".into(),
                "data-platform".into(),
                "shippo-databricks".into(),
                "feron".into(),
            ],
            triggers: vec![
                "databricks".into(),
                "dbt ".into(),
                "data warehouse".into(),
                "sql query".into(),
            ],
            ..Default::default()
        },
    );

    m.insert(
        "minimal".into(),
        Profile {
            display: "Minimal".into(),
            description: "Slack and Gmail only".into(),
            keep: vec![
                "mcp__claude_ai_Slack__".into(),
                "mcp__claude_ai_Gmail__".into(),
            ],
            ..Default::default()
        },
    );

    m.insert(
        "all".into(),
        Profile {
            display: "All Tools".into(),
            description: "No filtering (current default without ctx)".into(),
            ..Default::default()
        },
    );

    m
}

/// Extract the working directory from a Claude Code system prompt.
/// Claude Code injects "Primary working directory: /path/to/dir" (and similar variants).
pub fn extract_working_directory_from_system(system: &str) -> Option<String> {
    for line in system.lines() {
        let lower = line.trim().to_lowercase();
        let path = if let Some(rest) = lower.strip_prefix("primary working directory:") {
            rest.trim()
        } else if let Some(rest) = lower.strip_prefix("working directory:") {
            rest.trim()
        } else if let Some(rest) = lower.strip_prefix("cwd:") {
            rest.trim()
        } else {
            continue;
        };
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }
    None
}

/// Match from embedding similarity voting over past sessions.
#[derive(Debug, Clone)]
pub struct ProfileMatch {
    pub slug: String,
    /// Share of weighted votes for the winning profile (0–1).
    pub confidence: f32,
    /// Sessions that voted for the winning profile.
    pub based_on: usize,
    /// Mean embedding similarity among those voters.
    pub avg_match: f32,
}

fn similarity_auto_trigger(m: &ProfileMatch, confirmed: bool) -> String {
    let base = format!("similarity:{:.2}·{}", m.avg_match, m.based_on);
    if confirmed {
        format!("{base}:confirmed")
    } else {
        base
    }
}

/// Vote profile from similar past sessions (hook traces preferred over session.profile).
pub fn select_by_similarity(cwd: &str, prompt: &str, active_slug: &str) -> Option<ProfileMatch> {
    let cfg = Config::load();
    if !cfg.embeddings_enabled() {
        return None;
    }
    let Ok(conn) = crate::db::open_db() else {
        return None;
    };
    let _ = crate::db::ensure_schema(&conn);

    let embedding_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    if embedding_rows < 2 {
        return None;
    }

    let visible: HashSet<String> = visible_profile_slugs(active_slug).into_iter().collect();
    let query_text = format!("[dir: {}] {}", cwd.trim(), prompt.trim());
    let embedding = crate::embedder::embed_text(&query_text).ok()?;
    let sims = crate::embedder::similar_sessions_by_query(&conn, &embedding, 10, None).ok()?;
    if sims.is_empty() {
        return None;
    }

    let mut scores: HashMap<String, (f32, usize, f32)> = HashMap::new();
    let mut total_weight = 0f32;
    for (session_pk, sim) in &sims {
        let Some((profile, weight)) = profile_vote_for_session(&conn, *session_pk, *sim) else {
            continue;
        };
        if profile.is_empty() || profile == "all" || !visible.contains(&profile) {
            continue;
        }
        let entry = scores.entry(profile).or_insert((0.0, 0, 0.0));
        entry.0 += weight;
        entry.1 += 1;
        entry.2 += *sim;
        total_weight += weight;
    }

    if scores.is_empty() {
        return None;
    }

    let (best_profile, (best_score, based_on, sim_sum)) = scores.into_iter().max_by(|a, b| {
        a.1 .0
            .partial_cmp(&b.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;

    if based_on < 2 {
        return None;
    }
    let confidence = if total_weight > 0.0 {
        (best_score / total_weight).min(1.0)
    } else {
        0.0
    };
    if confidence < cfg.similarity_min_confidence {
        return None;
    }
    let avg_match = (sim_sum / based_on as f32).clamp(0.0, 1.0);
    if avg_match < cfg.similarity_min_avg_match {
        return None;
    }

    Some(ProfileMatch {
        slug: best_profile,
        confidence,
        based_on,
        avg_match,
    })
}

fn profile_vote_for_session(
    conn: &rusqlite::Connection,
    session_pk: i64,
    sim: f32,
) -> Option<(String, f32)> {
    let external: Option<String> = conn
        .query_row(
            "SELECT external_key FROM sessions WHERE id = ?1",
            rusqlite::params![session_pk],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();

    if let Some(ext) = external.as_deref() {
        let hook_vote = conn.query_row(
            "SELECT profile, COALESCE(SUM(tokens_saved), 0) FROM hook_traces \
             WHERE (session_id = ?1 OR ?1 LIKE '%' || session_id || '%') \
               AND profile IS NOT NULL AND profile != '' AND profile != 'all' \
             GROUP BY profile ORDER BY SUM(tokens_saved) DESC LIMIT 1",
            rusqlite::params![ext],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        );
        if let Ok((profile, tokens_saved)) = hook_vote {
            let w = sim * (tokens_saved.max(0) as f32 + 1.0);
            return Some((profile, w));
        }
    }

    let (profile, tokens_saved): (String, i64) = conn
        .query_row(
            "SELECT COALESCE(profile,''), tokens_saved FROM sessions WHERE id = ?1",
            rusqlite::params![session_pk],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;
    if profile.is_empty() || profile == "all" {
        return None;
    }
    let w = sim * (tokens_saved.max(0) as f32 + 1.0);
    Some((profile, w))
}

fn auto_select_by_path(cwd: &str, prompt: &str, active_slug: &str) -> Option<(String, String)> {
    let profiles = load_all();
    let visible = visible_profile_slugs(active_slug);
    let cwd_lower = cwd.to_lowercase();

    for slug in &visible {
        if slug == "all" {
            continue;
        }
        if let Some(p) = profiles.get(slug) {
            if p.matches_path(&cwd_lower) {
                let dir_label = cwd
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .last()
                    .unwrap_or(cwd)
                    .to_string();
                if slug != active_slug {
                    return Some((slug.clone(), format!("cwd:{dir_label}")));
                }
                return Some((slug.clone(), format!("cwd:{dir_label}:confirmed")));
            }
        }
    }

    let combined = format!("{cwd_lower} {prompt}");
    for slug in &visible {
        if slug == "all" {
            continue;
        }
        if let Some(p) = profiles.get(slug) {
            if p.matches_system_prompt(&combined) {
                let matched_trigger = p
                    .triggers
                    .iter()
                    .find(|t| combined.contains(t.as_str()))
                    .cloned()
                    .unwrap_or_default();
                if slug != active_slug {
                    return Some((slug.clone(), format!("keyword:{matched_trigger}")));
                }
                return Some((slug.clone(), format!("keyword:{matched_trigger}:confirmed")));
            }
        }
    }
    None
}

/// Find the best profile for cwd + prompt. Similarity first, then visible-profile path fallback.
/// Returns a match even when the best profile is already active (`:confirmed` trigger suffix).
pub fn auto_select(cwd: &str, prompt: &str, active_slug: &str) -> Option<(String, String)> {
    if let Some(m) = select_by_similarity(cwd, prompt, active_slug) {
        let trigger = similarity_auto_trigger(&m, m.slug == active_slug);
        return Some((m.slug, trigger));
    }
    auto_select_by_path(cwd, prompt, active_slug)
}

pub fn load_all() -> HashMap<String, Profile> {
    let mut profiles = defaults();
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    if let Ok(content) = std::fs::read_to_string(&custom_path) {
        if let Ok(custom) = toml::from_str::<HashMap<String, Profile>>(&content) {
            profiles.extend(custom);
        }
    }
    profiles
}

pub fn get(slug: &str) -> Result<Profile> {
    load_all()
        .remove(slug)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found. Run `ctx profile list`.", slug))
}

/// Server identifiers for Claude Code `allowedMcpServers`. Empty means allow all MCP servers.
/// Uses underscore-preserving IDs (e.g. `Data_Shippo`) because `serverName` in
/// `allowedMcpServers` only allows `[a-zA-Z0-9_-]`.
pub fn allowed_server_names_for_profile(p: &Profile) -> Vec<String> {
    if p.keep.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = p
        .keep
        .iter()
        .map(|k| {
            if k.starts_with("mcp__") {
                mcp_prefix_to_server_id(k)
            } else {
                k.replace(' ', "_")
            }
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// All MCP server tool prefixes from usage history and the active profile keep-list.
pub fn all_known_server_prefixes() -> Vec<String> {
    let mut seen: HashSet<String> = collect_observed_prefixes().into_iter().collect();
    let cfg = Config::load();
    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    if let Ok(profile) = get(slug) {
        for prefix in &profile.keep {
            if prefix.starts_with("mcp__") {
                seen.insert(prefix.clone());
            }
        }
    }
    let mut out: Vec<String> = seen.into_iter().collect();
    out.sort();
    out
}

/// Claude Code `permissions.deny` wildcard for a tool prefix, e.g. `mcp__claude_ai_Figma__*`.
pub fn deny_wildcard_for_prefix(prefix: &str) -> String {
    if prefix.ends_with("__*") {
        prefix.to_string()
    } else if prefix.ends_with("__") {
        format!("{prefix}*")
    } else {
        format!("{prefix}__*")
    }
}

/// True for ctx-managed remote-connector deny entries (server wildcards or per-tool names).
pub fn is_ctx_managed_deny_pattern(rule: &str) -> bool {
    if !rule.starts_with("mcp__claude_ai_") {
        return false;
    }
    if rule.ends_with("__*") {
        return true;
    }
    rule.strip_prefix("mcp__claude_ai_")
        .map(|rest| rest.contains("__") && !rule.ends_with("__"))
        .unwrap_or(false)
}

fn tool_kept_by_profile(tool_name: &str, profile: &Profile) -> bool {
    profile.keeps_tool(tool_name)
}

fn tool_matches_expansion(tool_name: &str, expansion: &[String]) -> bool {
    if expansion.iter().any(|e| tool_name.eq_ignore_ascii_case(e)) {
        return true;
    }
    if let Some(prefix) = crate::filter::server_prefix_from_tool(tool_name) {
        return prefix_matches_expansion(&prefix, expansion);
    }
    false
}

fn deny_universe_tools(profile: &Profile) -> Vec<String> {
    let lookback = Config::load().profile_thresholds.lookback_days;
    let observed: Vec<String> = observed_tools_with_lookback(lookback)
        .into_iter()
        .map(|r| r.tool_name)
        .collect();
    if !observed.is_empty() {
        return observed;
    }
    if profile.filtering_enabled() {
        return SERVER_COUNTS
            .iter()
            .flat_map(|(prefix, count)| (0..*count).map(move |i| format!("{prefix}tool_{i}")))
            .collect();
    }
    Vec::new()
}

fn prefix_kept_by_profile(prefix: &str, profile: &Profile) -> bool {
    if profile.uses_tool_level() {
        return profile
            .keep_tools
            .iter()
            .any(|t| t.starts_with(prefix) || prefix.starts_with(t.as_str()));
    }
    if profile.keep.is_empty() {
        return true;
    }
    profile
        .keep
        .iter()
        .any(|k| prefix.starts_with(k.as_str()) || k.starts_with(prefix))
}

fn prefix_matches_expansion(prefix: &str, expansion: &[String]) -> bool {
    let id = mcp_prefix_to_server_id(prefix);
    let display = mcp_prefix_to_server_display(prefix);
    expansion.iter().any(|e| {
        let el = e.to_lowercase();
        prefix.eq_ignore_ascii_case(e)
            || id.eq_ignore_ascii_case(e)
            || display.to_lowercase() == el
            || el.contains(&display.to_lowercase())
    })
}

/// Prefixes that can receive deny rules for a profile (observed history, or catalog when filtering before history exists).
fn deny_universe_prefixes(profile: &Profile) -> Vec<String> {
    let observed = collect_observed_prefixes();
    if !observed.is_empty() {
        return observed;
    }
    if !profile.filtering_enabled() {
        return Vec::new();
    }
    // Built-in / manual profile before any MCP usage indexed: use catalog as deny universe.
    SERVER_COUNTS
        .iter()
        .map(|(k, _)| (*k).to_string())
        .collect()
}

/// MCP tool deny rules for tools outside the profile keep-list (soft filter mode).
/// `local_names` — MCP server keys from settings (always includes `ctx`); never denied.
pub fn deny_patterns_for_profile(
    profile: &Profile,
    expansion: &[String],
    local_names: &[String],
) -> Vec<String> {
    if !profile.filtering_enabled() {
        return Vec::new();
    }

    if profile.uses_tool_level() {
        let mut patterns: Vec<String> = deny_universe_tools(profile)
            .into_iter()
            .filter(|tool| {
                !tool_kept_by_profile(tool, profile)
                    && !tool_matches_expansion(tool, expansion)
                    && !prefix_is_local_mcp(
                        crate::filter::server_prefix_from_tool(tool)
                            .as_deref()
                            .unwrap_or(tool),
                        local_names,
                    )
            })
            .collect();
        patterns.sort();
        patterns.dedup();
        return patterns;
    }

    let mut patterns: Vec<String> = deny_universe_prefixes(profile)
        .into_iter()
        .filter(|prefix| {
            !prefix_kept_by_profile(prefix, profile)
                && !prefix_matches_expansion(prefix, expansion)
                && !prefix_is_local_mcp(prefix, local_names)
        })
        .map(|prefix| deny_wildcard_for_prefix(&prefix))
        .collect();
    patterns.sort();
    patterns.dedup();
    patterns
}

fn prefix_is_local_mcp(prefix: &str, local_names: &[String]) -> bool {
    let id = mcp_prefix_to_server_id(prefix);
    let display = mcp_prefix_to_server_display(prefix);
    local_names.iter().any(|n| {
        n.eq_ignore_ascii_case(&id)
            || n.eq_ignore_ascii_case(&display)
            || prefix.contains(&format!("__{n}__"))
    })
}

/// Slugs defined in `~/.ctx/profiles.toml` (generated or user-edited).
pub fn slugs_from_profiles_toml() -> HashSet<String> {
    let path = crate::config::ctx_dir().join("profiles.toml");
    if !path.is_file() {
        return HashSet::new();
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashSet::new();
    };
    toml::from_str::<HashMap<String, Profile>>(&content)
        .map(|m| m.into_keys().collect())
        .unwrap_or_default()
}

/// Display names for servers kept vs tools hidden, using observed + default prefixes.
pub fn profile_server_display_lists(profile: &Profile) -> (Vec<String>, Vec<String>) {
    if !profile.filtering_enabled() {
        return (Vec::new(), Vec::new());
    }
    if profile.uses_tool_level() {
        let lookback = Config::load().profile_thresholds.lookback_days;
        let rows = observed_tools_with_lookback(lookback);
        let mut included: HashSet<String> = HashSet::new();
        let mut excluded: HashSet<String> = HashSet::new();
        for row in rows {
            let name = mcp_prefix_to_server_display(&row.server_prefix);
            if profile.keeps_tool(&row.tool_name) {
                included.insert(name);
            } else {
                excluded.insert(name);
            }
        }
        for t in &profile.keep_tools {
            if let Some(p) = crate::filter::server_prefix_from_tool(t) {
                included.insert(mcp_prefix_to_server_display(&p));
            }
        }
        for s in &included {
            excluded.remove(s);
        }
        let mut included: Vec<String> = included.into_iter().collect();
        let mut excluded: Vec<String> = excluded.into_iter().collect();
        included.sort();
        excluded.sort();
        return (included, excluded);
    }
    let prefixes = all_known_server_prefixes();
    if prefixes.is_empty() {
        let included: Vec<String> = profile
            .keep
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        return (included, Vec::new());
    }
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    for prefix in prefixes {
        let name = mcp_prefix_to_server_display(&prefix);
        if prefix_kept_by_profile(&prefix, profile) {
            included.push(name);
        } else {
            excluded.push(name);
        }
    }
    included.sort();
    included.dedup();
    excluded.sort();
    excluded.dedup();
    (included, excluded)
}

/// Preferred sort order for generated category profiles.
pub fn generated_profile_sort_key(slug: &str) -> usize {
    match slug {
        "data" => 0,
        "design" => 1,
        "work" => 2,
        "finance" => 3,
        "files" => 4,
        "infra" => 5,
        "comms" => 6,
        "other" => 7,
        "all" => 90,
        "carrier" => 95,
        "minimal" => 96,
        _ => 50,
    }
}

/// Local MCP server names from settings `mcpServers` — never add deny rules for these.
pub fn local_mcp_server_names(settings: &serde_json::Value) -> Vec<String> {
    let mut names = vec!["ctx".to_string()];
    if let Some(obj) = settings.get("mcpServers").and_then(|v| v.as_object()) {
        for key in obj.keys() {
            names.push(key.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Detect denied tool/server mentions in prompt/cwd for session expansion hints.
pub fn detect_expansion_candidates(prompt: &str, cwd: &str, profile: &Profile) -> Vec<String> {
    if !profile.filtering_enabled() {
        return Vec::new();
    }
    let hay = format!("{prompt}\n{cwd}").to_lowercase();
    let mut out = Vec::new();

    if profile.uses_tool_level() {
        for row in observed_tools_with_lookback(Config::load().profile_thresholds.lookback_days) {
            if profile.keeps_tool(&row.tool_name) {
                continue;
            }
            let display = mcp_prefix_to_server_display(&row.server_prefix).to_lowercase();
            let tool_tail = row
                .tool_name
                .rsplit("__")
                .next()
                .unwrap_or("")
                .replace('_', " ");
            let tool_lower = row.tool_name.to_lowercase();
            if hay.contains(&tool_lower)
                || hay.contains(&display)
                || (!tool_tail.is_empty()
                    && tool_tail.len() >= 4
                    && hay.contains(&tool_tail.to_lowercase()))
            {
                out.push(row.tool_name.clone());
            }
        }
        out.sort();
        out.dedup();
        return out;
    }

    for prefix in all_known_server_prefixes() {
        if prefix_kept_by_profile(&prefix, profile) {
            continue;
        }
        let display = mcp_prefix_to_server_display(&prefix).to_lowercase();
        let id = mcp_prefix_to_server_id(&prefix)
            .to_lowercase()
            .replace('_', " ");
        if hay.contains(&display) || (!id.is_empty() && hay.contains(&id)) {
            out.push(mcp_prefix_to_server_id(&prefix));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn effective_keep_prefixes(slug: &str) -> Result<HashSet<String>> {
    let p = get(slug)?;
    Ok(if p.keep.is_empty() {
        SERVER_COUNTS
            .iter()
            .map(|(k, _)| (*k).to_string())
            .collect()
    } else {
        p.keep.into_iter().collect()
    })
}

pub fn apply_profile(slug: &str, force: bool, quiet: bool) -> Result<()> {
    let mut config = Config::load();
    let from_slug = config
        .active_profile
        .clone()
        .unwrap_or_else(|| "all".into());
    let profile = get(slug)?;

    if from_slug != slug {
        let report = crate::quality_guard::safety_report_for_profile(&profile);
        if !force && !report.critical_blockers.is_empty() {
            for b in &report.critical_blockers {
                eprintln!("{} {}", "[ctx]".yellow(), b);
            }
            bail!(
                "Blocked switch to '{}': active MCP usage on servers this profile would strip. Retry with --force if you accept the risk.",
                slug
            );
        }
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);
            let from_set = effective_keep_prefixes(&from_slug)?;
            let to_set = effective_keep_prefixes(slug)?;
            let added: Vec<String> = to_set.difference(&from_set).cloned().collect();
            let removed: Vec<String> = from_set.difference(&to_set).cloned().collect();
            let _ = crate::db::insert_profile_change(
                &conn,
                &from_slug,
                slug,
                &serde_json::to_string(&added).unwrap_or_else(|_| "[]".into()),
                &serde_json::to_string(&removed).unwrap_or_else(|_| "[]".into()),
            );
        }
    }

    config.active_profile = Some(slug.to_string());
    config.save()?;

    crate::filter_hook::write_filter_config_for_slug(slug)?;

    let dash = Config::load().dashboard_port.unwrap_or(8789);
    let mode = Config::load().filter_mode;
    if mode == crate::config::FilterMode::Strict && !quiet && !force {
        eprintln!(
            "{} strict filter mode: non-allowlisted MCP servers will disconnect in Claude Code.",
            "[ctx]".yellow()
        );
    }
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    let _ = crate::behavior_guard::write_behavior_hints_file();

    if !quiet {
        let pct = (profile.savings_pct() * 100.0) as u32;
        let mode = Config::load().filter_mode;
        println!(
            "{} Profile: {} ({})",
            "✓".green().bold(),
            profile.display.bold(),
            profile.description
        );
        println!(
            "  ~{} tools  |  ~{} tokens/turn  |  saving ~{} tokens ({pct}%) vs unfiltered",
            profile.tool_count(),
            fmt_k(profile.token_cost()),
            fmt_k(profile.savings_vs_all()),
        );
        let deny_n =
            deny_patterns_for_profile(&profile, &Config::load().session_expansion, &[]).len();
        match mode {
            crate::config::FilterMode::Soft => {
                println!(
                    "  Filter:     soft ({} deny rules; MCP servers stay connected)",
                    deny_n
                );
            }
            crate::config::FilterMode::Strict => {
                println!("  Filter:     strict (allowedMcpServers; other connectors disconnect)");
            }
            crate::config::FilterMode::Off => {
                println!("  Filter:     off (no ctx filter rules)");
            }
        }
        println!(
            "{} ~/.claude/settings.json updated (hooks + filter mode {})",
            "i".dimmed(),
            mode.as_str()
        );
        println!(
            "{} filter-config.json still written for legacy NODE_OPTIONS setups only",
            "i".dimmed()
        );
    }
    Ok(())
}

pub fn switch(slug: &str, force: bool) -> Result<()> {
    apply_profile(slug, force, false)
}

/// Build `personal` profile from MCP tool_use history when usage thresholds are met.
pub fn auto_generate(refresh: bool) -> Result<()> {
    if upsert_personal_from_usage(refresh)? {
        let custom_path = crate::config::ctx_dir().join("profiles.toml");
        let p = get("personal")?;
        let n = if p.uses_tool_level() {
            p.keep_tools.len()
        } else {
            p.keep.len()
        };
        println!(
            "{} Wrote profile `personal` with {} MCP tool(s) to {}",
            "✓".green().bold(),
            n,
            custom_path.display()
        );
        return Ok(());
    }
    let stats = usage_stats();
    let t = Config::load().profile_thresholds;
    if !personal_ready(&stats) {
        bail!(
            "Personal profile not ready yet ({}/{} tool calls, {}/{} servers, {}/{} sessions with MCP). \
             Use Claude Code with MCP tools, then run `ctx ingest`.",
            stats.tool_invocations,
            t.min_tool_invocations,
            stats.distinct_servers,
            t.min_distinct_servers,
            stats.sessions_with_mcp,
            t.min_sessions_with_mcp,
        );
    }
    bail!("No MCP tool history in the lookback window. Run `ctx ingest` after using Claude Code, then retry.");
}

pub fn status() -> Result<()> {
    let config = Config::load();
    let slug = config.active_profile.as_deref().unwrap_or("all");
    let profile = get(slug).unwrap_or_else(|_| Profile {
        display: "All Tools".into(),
        description: "No filtering".into(),
        ..Default::default()
    });

    println!("Profile:    {} ({})", slug.bold(), profile.display);
    println!("Filter:     {} mode", config.filter_mode.as_str());
    if tool_metrics_ready() {
        let total = dynamic_total_tools();
        println!("Tools:      ~{} / {} active", profile.tool_count(), total);
        println!(
            "Tokens/turn: ~{} (tool schemas only)",
            fmt_k(profile.token_cost())
        );
        println!(
            "Savings:    ~{} ({:.0}%) vs unfiltered",
            fmt_k(profile.savings_vs_all()),
            profile.savings_pct() * 100.0
        );
    } else {
        println!("Tools:      unknown (no MCP usage indexed yet)");
        println!("Tokens/turn: — (use Claude Code with MCP tools, then ctx ingest)");
        println!("Savings:    —");
    }

    let port = config.proxy_port.unwrap_or(8788);
    let upstream = config
        .proxy_upstream
        .as_deref()
        .unwrap_or("https://api.anthropic.com");
    println!("\nProxy:      :{port} -> {upstream}");

    if let Ok(alerts) = crate::quality_guard::quality_alerts() {
        if let Some(a) = alerts.first() {
            println!("{} {}", "!".yellow(), a.recommendation);
        }
    }

    let stats = usage_stats();
    let t = Config::load().profile_thresholds;
    if personal_ready(&stats) {
        if slugs_from_profiles_toml().iter().any(|s| s == "personal") {
            println!("\nPersonal:   ready (usage-based profile in profiles.toml)");
        }
    } else {
        println!(
            "\nPersonal:   building ({}/{} tool calls, {}/{} servers, {}/{} MCP sessions)",
            stats.tool_invocations,
            t.min_tool_invocations,
            stats.distinct_servers,
            t.min_distinct_servers,
            stats.sessions_with_mcp,
            t.min_sessions_with_mcp,
        );
    }

    Ok(())
}

pub fn list_profiles_json() -> serde_json::Value {
    let config = Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let profiles = load_all();
    let slugs = visible_profile_slugs(active);
    let metrics_ready = tool_metrics_ready();
    let items: Vec<serde_json::Value> = slugs.iter().map(|slug| {
        let p = &profiles[slug];
        serde_json::json!({
            "slug": slug,
            "display": p.display,
            "description": p.description,
            "active": slug.as_str() == active,
            "metrics_pending": !metrics_ready,
            "tools": if metrics_ready { serde_json::json!(p.tool_count()) } else { serde_json::Value::Null },
            "tokens_per_turn": if metrics_ready { serde_json::json!(p.token_cost()) } else { serde_json::Value::Null },
            "savings_vs_all": if metrics_ready { serde_json::json!(p.savings_vs_all()) } else { serde_json::Value::Null },
            "savings_pct": if metrics_ready { serde_json::json!((p.savings_pct() * 100.0).round()) } else { serde_json::Value::Null },
        })
    }).collect();
    serde_json::json!(items)
}

pub fn list() -> Result<()> {
    let config = Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let profiles = load_all();
    let slugs = visible_profile_slugs(active);

    println!(
        "{:<12} {:<6} {:<11} {}",
        "PROFILE", "TOOLS", "TOKENS/TURN", "DESCRIPTION"
    );
    println!("{}", "─".repeat(58));
    let metrics_ready = tool_metrics_ready();
    for slug in &slugs {
        let p = &profiles[slug];
        let marker = if slug.as_str() == active {
            "*".green().bold().to_string()
        } else {
            " ".to_string()
        };
        let (tools_col, tokens_col) = if metrics_ready {
            (p.tool_count().to_string(), fmt_k(p.token_cost()))
        } else {
            ("—".to_string(), "—".to_string())
        };
        println!(
            "{} {:<11} {:<6} {:<11} {}",
            marker, slug, tools_col, tokens_col, p.description
        );
    }
    println!("\n* = active");
    let stats = usage_stats();
    let t = Config::load().profile_thresholds;
    if !personal_ready(&stats) {
        println!(
            "\n{} Personal profile: {}/{} tool calls, {}/{} servers, {}/{} MCP sessions — staying on `all` until ready.",
            "i".cyan(),
            stats.tool_invocations,
            t.min_tool_invocations,
            stats.distinct_servers,
            t.min_distinct_servers,
            stats.sessions_with_mcp,
            t.min_sessions_with_mcp,
        );
        if tool_metrics_ready() {
            println!(
                "{} Tool-level metrics are active ({} distinct tools indexed); filtering starts when personal is auto-created.",
                "i".cyan(),
                dynamic_total_tools(),
            );
        }
    } else if slugs.len() == 1 && active == "all" {
        println!(
            "\n{} Usage-based profiles appear here after Claude Code MCP tool calls are indexed (ingest runs every 5 min).",
            "i".cyan()
        );
    }
    Ok(())
}

pub fn show(slug: &str) -> Result<()> {
    let p = get(slug)?;
    println!("Profile:  {} ({})", slug.bold(), p.display);
    println!("Desc:     {}", p.description);
    if tool_metrics_ready() {
        println!("Tools:    ~{}", p.tool_count());
        println!("Cost:     ~{} tokens/turn", fmt_k(p.token_cost()));
        println!(
            "Savings:  ~{} ({:.0}%) vs unfiltered",
            fmt_k(p.savings_vs_all()),
            p.savings_pct() * 100.0
        );
    } else {
        println!("Tools:    unknown (no MCP usage indexed yet)");
        println!("Cost:     —");
        println!("Savings:  —");
    }
    if !p.filtering_enabled() {
        println!("Keep:     all servers (no filtering)");
    } else if p.uses_tool_level() {
        println!(
            "Keep tools: {} tool(s) across {} server(s)",
            p.tool_count(),
            p.server_count()
        );
        for t in &p.keep_tools {
            println!("  {t}");
        }
    } else {
        println!("MCP servers (allowedMcpServers):");
        for k in allowed_server_names_for_profile(&p) {
            println!("  {k}");
        }
    }
    Ok(())
}

/// Append a tool to the active profile's keep_tools list in profiles.toml.
pub fn append_keep_tool_to_active_profile(tool: &str) -> Result<()> {
    let cfg = Config::load();
    let slug = cfg.active_profile.clone().unwrap_or_else(|| "all".into());
    let mut profile = get(&slug)?;
    if profile
        .keep_tools
        .iter()
        .any(|t| t.eq_ignore_ascii_case(tool))
    {
        return Ok(());
    }
    profile.keep_tools.push(tool.to_string());
    profile.keep_tools.sort();
    profile.keep_tools.dedup();

    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?).unwrap_or_default()
    } else {
        HashMap::new()
    };
    existing.insert(slug.clone(), profile);
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

pub fn add(slug: &str, keep: Vec<String>, keep_tools: Vec<String>) -> Result<()> {
    if keep.is_empty() && keep_tools.is_empty() {
        bail!("Provide --keep (server prefixes) or --keep-tool (tool names)");
    }
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?)?
    } else {
        HashMap::new()
    };
    let description = if !keep_tools.is_empty() {
        format!("Custom: {} tool(s)", keep_tools.len())
    } else {
        format!("Custom: {}", keep.join(", "))
    };
    existing.insert(
        slug.to_string(),
        Profile {
            display: slug.to_string(),
            description,
            keep,
            keep_tools,
            ..Default::default()
        },
    );
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    println!("{} Added profile '{slug}'", "✓".green());
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

/// Expand server-prefix `keep` lists into `keep_tools` using observed invocation counts.
/// Does not create `[personal]` before `personal_ready` — that profile is written automatically
/// by ingest/bootstrap once tool, server, and session thresholds are met.
pub fn migrate_tools(slug: Option<&str>, force: bool) -> Result<()> {
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let t = Config::load().profile_thresholds;
    let lookback = t.lookback_days;
    let min_n = t.min_tool_invocations_per_tool;

    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?)?
    } else {
        HashMap::new()
    };

    if existing.is_empty() && slug.is_none() {
        let stats = usage_stats();
        bail!(
            "No custom profiles in {} yet. `[personal]` is created automatically once usage thresholds are met \
             ({}/{} tool calls, {}/{} servers, {}/{} MCP sessions in the last {} days). \
             Run `ctx profile list` to check progress.",
            custom_path.display(),
            stats.tool_invocations,
            t.min_tool_invocations,
            stats.distinct_servers,
            t.min_distinct_servers,
            stats.sessions_with_mcp,
            t.min_sessions_with_mcp,
            lookback,
        );
    }

    let slugs: Vec<String> = match slug.as_deref() {
        Some(s) => {
            if s == "personal" && !existing.contains_key("personal") {
                let stats = usage_stats();
                bail!(
                    "Profile `personal` does not exist yet. It will be created automatically when \
                     {}/{} tool calls, {}/{} servers, and {}/{} MCP sessions are indexed.",
                    stats.tool_invocations,
                    t.min_tool_invocations,
                    stats.distinct_servers,
                    t.min_distinct_servers,
                    stats.sessions_with_mcp,
                    t.min_sessions_with_mcp,
                );
            }
            if existing.contains_key(s) {
                vec![s.to_string()]
            } else if load_all().contains_key(s) {
                existing.insert(s.to_string(), get(s)?);
                vec![s.to_string()]
            } else {
                bail!(
                    "Profile '{s}' not found in {} or built-in templates",
                    custom_path.display()
                );
            }
        }
        None => existing.keys().cloned().collect(),
    };

    let mut migrated = 0usize;
    for s in slugs {
        if s == "personal" {
            let p = existing.get("personal").cloned().unwrap_or_default();
            if p.uses_tool_level() && !force {
                eprintln!(
                    "{} Skipping `personal` — already uses keep_tools",
                    "i".dimmed()
                );
                continue;
            }
            if p.keep.is_empty() {
                if !p.uses_tool_level() {
                    eprintln!(
                        "{} `personal`: already tool-level or empty — nothing to migrate",
                        "i".dimmed()
                    );
                }
                continue;
            }
            let tools = tools_for_prefixes(&p.keep, lookback, min_n);
            if tools.is_empty() {
                eprintln!(
                    "{} `personal`: no tools met ≥{min_n} invocations in last {lookback} days",
                    "i".dimmed()
                );
                continue;
            }
            let before = p.tool_count();
            existing.insert(
                "personal".into(),
                Profile {
                    keep: vec![],
                    keep_tools: tools.clone(),
                    ..p
                },
            );
            migrated += 1;
            print_migration_line(
                "personal",
                before,
                tools.len(),
                min_n,
                before.saturating_sub(tools.len()),
            );
            continue;
        }

        let p = existing.get(&s).cloned().unwrap_or_default();
        if p.uses_tool_level() && !force {
            eprintln!("{} Skipping `{s}` — already uses keep_tools", "i".dimmed());
            continue;
        }
        if p.keep.is_empty() {
            eprintln!(
                "{} `{s}`: no server-prefix keep list to migrate",
                "i".dimmed()
            );
            continue;
        }
        let tools = tools_for_prefixes(&p.keep, lookback, min_n);
        if tools.is_empty() {
            eprintln!(
                "{} `{s}`: no tools met ≥{min_n} invocations in last {lookback} days",
                "i".dimmed()
            );
            continue;
        }
        let before = p.tool_count();
        existing.insert(
            s.clone(),
            Profile {
                keep: vec![],
                keep_tools: tools.clone(),
                ..p
            },
        );
        let after = tools.len();
        migrated += 1;
        print_migration_line(&s, before, after, min_n, before.saturating_sub(after));
    }

    if migrated == 0 {
        bail!(
            "No profiles migrated. `migrate-tools` converts server-prefix `keep` lists to `keep_tools`; \
             `[personal]` is created automatically once personal usage thresholds are met."
        );
    }
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

fn print_migration_line(slug: &str, before: usize, after: usize, min_n: u32, tools_dropped: usize) {
    println!(
        "{} `{slug}`: ~{before} → {after} tools (min {min_n} invocations; dropped {tools_dropped} schemas)",
        "✓".green().bold(),
    );
    if tools_dropped > 0 {
        println!(
            "  ~{} tokens/turn saved vs server-prefix estimate",
            fmt_k(tools_dropped * TOKENS_PER_TOOL)
        );
    }
}

/// Observed MCP tools for dashboard checklists (tool_name, server display, invocation count).
pub fn observed_tool_catalog() -> Vec<(String, String, u64)> {
    let lookback = Config::load().profile_thresholds.lookback_days;
    observed_tools_with_lookback(lookback)
        .into_iter()
        .map(|r| {
            (
                r.tool_name,
                mcp_prefix_to_server_display(&r.server_prefix),
                r.count,
            )
        })
        .collect()
}

pub fn remove(slug: &str) -> Result<()> {
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    if !custom_path.exists() {
        bail!("No custom profiles found");
    }
    let mut existing: HashMap<String, Profile> =
        toml::from_str(&std::fs::read_to_string(&custom_path)?)?;
    if existing.remove(slug).is_none() {
        bail!("Profile '{slug}' not found (built-in profiles cannot be removed)");
    }
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    println!("{} Removed profile '{slug}'", "✓".green());
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

// ---------------------------------------------------------------------------
// Profile generator
// ---------------------------------------------------------------------------

/// Maps server display names to coarse task categories.
/// Display names are the human-readable form: "Data Shippo", "Atlassian", etc.
const SERVER_CATEGORY_MAP: &[(&str, &str)] = &[
    ("Data Shippo", "data"),
    ("Fullstory", "data"),
    ("AWS Marketplace", "data"),
    ("Figma", "design"),
    ("Canva", "design"),
    ("Miro", "design"),
    ("Adobe Marketing Agent", "design"),
    ("Webflow", "design"),
    ("Slack", "comms"),
    ("Gmail", "comms"),
    ("Microsoft 365", "comms"),
    ("Zoom for Claude", "comms"),
    ("Fireflies", "comms"),
    ("Atlassian", "work"),
    ("Linear", "work"),
    ("Notion", "work"),
    ("Incident io", "work"),
    ("Postman", "work"),
    ("Zapier", "work"),
    ("Stripe", "finance"),
    ("Ramp", "finance"),
    ("NetSuite", "finance"),
    ("NetSuite Sandbox", "finance"),
    ("Moody s", "finance"),
    ("Intuit Mailchimp", "finance"),
    ("ZoomInfo", "finance"),
    ("Clay", "finance"),
    ("Tropic", "finance"),
    ("Google Drive", "files"),
    ("Google Calendar", "files"),
    ("Docusign", "files"),
    ("Cloudflare Developer Platform", "infra"),
    ("Shippo MCP Dev", "work"),
    ("Shippo MCP DEV QA", "work"),
];

/// Extract the server identifier from an MCP tool prefix, keeping underscores.
/// `mcp__claude_ai_Data_Shippo__` -> `Data_Shippo`
/// Valid for `allowedMcpServers.serverName` which requires `[a-zA-Z0-9_-]`.
pub fn mcp_prefix_to_server_id(prefix: &str) -> String {
    prefix
        .strip_prefix("mcp__claude_ai_")
        .and_then(|s| s.strip_suffix("__"))
        .unwrap_or(prefix)
        .to_string()
}

/// Convert a server prefix back to its human-readable display name.
/// `mcp__claude_ai_Data_Shippo__` -> `Data Shippo`
pub fn mcp_prefix_to_server_display(prefix: &str) -> String {
    mcp_prefix_to_server_id(prefix).replace('_', " ")
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Collect MCP server prefixes from usage history in `ctx.db` only.
/// Returns empty when there is no history — callers must run ingest first.
pub fn collect_observed_prefixes() -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();

    if crate::db::db_exists() {
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);

            if let Ok(mut stmt) =
                conn.prepare("SELECT DISTINCT server_prefix FROM tool_invocations")
            {
                let _ = stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| {
                    rows.flatten().for_each(|p| {
                        seen.insert(p);
                    });
                });
            }

            if let Ok(mut stmt) = conn.prepare(
                "SELECT kept_servers, removed_servers, mcp_tools_invoked, tools_sent_by_server \
                 FROM requests \
                 WHERE kept_servers IS NOT NULL OR removed_servers IS NOT NULL \
                    OR mcp_tools_invoked IS NOT NULL OR tools_sent_by_server IS NOT NULL",
            ) {
                let _ = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                        ))
                    })
                    .map(|rows| {
                        rows.flatten()
                            .for_each(|(kept, removed, invoked, by_server)| {
                                for json in [kept, removed].into_iter().flatten() {
                                    if let Ok(names) = serde_json::from_str::<Vec<String>>(&json) {
                                        for name in names {
                                            seen.insert(display_name_to_prefix(&name));
                                        }
                                    }
                                }
                                if let Some(json) = invoked {
                                    if let Ok(tools) = serde_json::from_str::<Vec<String>>(&json) {
                                        for tool in tools {
                                            if let Some(prefix) =
                                                crate::filter::server_prefix_from_tool(&tool)
                                            {
                                                seen.insert(prefix);
                                            }
                                        }
                                    }
                                }
                                if let Some(json) = by_server {
                                    if let Ok(map) =
                                        serde_json::from_str::<HashMap<String, usize>>(&json)
                                    {
                                        for display in map.keys() {
                                            seen.insert(display_name_to_prefix(display));
                                        }
                                    }
                                }
                            });
                    });
            }
        }
    }

    let mut result: Vec<String> = seen.into_iter().collect();
    result.sort();
    result
}

fn display_name_to_prefix(display: &str) -> String {
    format!("mcp__claude_ai_{}__", display.replace(' ', "_"))
}

fn default_path_patterns(cat: &str) -> Vec<String> {
    let patterns: &[&str] = match cat {
        "data" => &[
            "databricks",
            "dbt",
            "analytics",
            "warehouse",
            "notebook",
            "jupyter",
            "sql",
        ],
        "design" => &["figma", "design", "ui", "ux", "sketch"],
        "work" => &["jira", "linear", "confluence", "notion", "asana", "trello"],
        "comms" => &["slack", "gmail", "email", "mail"],
        "finance" => &["netsuite", "ramp", "stripe", "billing", "finance"],
        "files" => &["gdrive", "drive", "dropbox", "sharepoint"],
        "infra" => &[
            "terraform",
            "kubernetes",
            "k8s",
            "docker",
            "aws",
            "cloudflare",
            "devops",
        ],
        _ => &[],
    };
    patterns.iter().map(|s| s.to_string()).collect()
}

const PREFERRED_GENERATED_SLUGS: &[&str] = &[
    "data", "design", "work", "finance", "files", "infra", "comms", "other",
];

/// First matching slug from a freshly generated `profiles.toml`.
pub fn preferred_generated_slug() -> Option<String> {
    let custom = slugs_from_profiles_toml();
    PREFERRED_GENERATED_SLUGS
        .iter()
        .find(|s| custom.contains(**s))
        .map(|s| (*s).to_string())
}

/// Remove auto-generated profile entries when they no longer match usage history.
pub fn prune_stale_auto_generated_profiles() -> Result<usize> {
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    if !custom_path.exists() {
        return Ok(0);
    }
    let mut existing: HashMap<String, Profile> =
        toml::from_str(&std::fs::read_to_string(&custom_path)?).unwrap_or_default();
    let before = existing.len();
    for slug in AUTO_GENERATED_PROFILE_SLUGS {
        existing.remove(*slug);
    }
    if existing.len() == before {
        return Ok(0);
    }
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    Ok(before - existing.len())
}

/// Build category profiles from observed MCP history when thresholds are met and none exist yet.
/// Returns true when profiles were written.
pub fn maybe_auto_generate_profiles_from_history() -> Result<bool> {
    let _ = upsert_personal_from_usage(false)?;
    if !categories_ready(&usage_stats()) {
        return Ok(false);
    }
    if has_category_profiles_in_toml() {
        return Ok(false);
    }
    generate_from_config(false)?;
    Ok(true)
}

/// Outcome of setup/ingest profile bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileBootstrap {
    /// Category profiles generated and activated.
    Generated { active: String },
    /// Legacy single `personal` profile from tool-use history.
    Personal,
    /// No MCP usage history yet — stay on `all`.
    NoHistory,
    /// Profiles already present; kept current selection.
    Existing,
}

/// Index JSONL/requests, generate profiles from MCP usage history, pick a default profile.
/// Call after `ingest_claude_jsonl()` (setup and periodic ingest do this automatically).
pub fn bootstrap_from_history(quiet: bool) -> Result<ProfileBootstrap> {
    crate::config::ensure_dir()?;

    if let Some(pin) = crate::experiment_plan::experiment_active_profile_pin() {
        let _ = upsert_personal_from_usage(false);
        let cfg = Config::load();
        let active = cfg.active_profile.as_deref().unwrap_or("all");
        if active != pin {
            switch(&pin, true)?;
        }
        return Ok(ProfileBootstrap::Existing);
    }

    if upsert_personal_from_usage(false)? {
        let cfg = Config::load();
        let active = cfg.active_profile.as_deref().unwrap_or("all");
        if active == "all" {
            switch("personal", true)?;
            if !quiet {
                println!(
                    "  {} Personal profile ready from MCP usage; active: {}",
                    "✓".green(),
                    "personal".bold()
                );
            }
            return Ok(ProfileBootstrap::Personal);
        }
        return Ok(ProfileBootstrap::Existing);
    }

    if maybe_auto_generate_profiles_from_history()? {
        if let Some(slug) = preferred_generated_slug() {
            switch(&slug, true)?;
            if !quiet {
                println!(
                    "  {} Generated profiles from MCP usage history; active: {}",
                    "✓".green(),
                    slug.bold()
                );
            }
            return Ok(ProfileBootstrap::Generated { active: slug });
        }
    }

    let custom = slugs_from_profiles_toml();
    if !custom.is_empty() {
        let cfg = Config::load();
        let active = cfg.active_profile.as_deref().unwrap_or("all");
        if active == "all" {
            if custom.iter().any(|s| s == "personal") {
                switch("personal", true)?;
                if !quiet {
                    println!(
                        "  {} Activated personal from your generated profiles",
                        "✓".green()
                    );
                }
                return Ok(ProfileBootstrap::Personal);
            }
            if let Some(slug) = preferred_generated_slug() {
                switch(&slug, true)?;
                if !quiet {
                    println!(
                        "  {} Activated {} from your generated profiles",
                        "✓".green(),
                        slug.bold()
                    );
                }
                return Ok(ProfileBootstrap::Generated { active: slug });
            }
        }
        return Ok(ProfileBootstrap::Existing);
    }

    let pruned = prune_stale_auto_generated_profiles()?;
    if pruned > 0 && !quiet {
        println!(
            "  {} Removed {} stale auto-generated profile(s) (no MCP usage history yet)",
            "✓".green(),
            pruned
        );
    }

    switch("all", true)?;
    if !quiet {
        let stats = usage_stats();
        let t = Config::load().profile_thresholds;
        if personal_ready(&stats) {
            println!(
                "  {} MCP history found but personal profile write failed — staying on 'all'",
                "!".yellow()
            );
        } else {
            println!(
                "  {} Staying on 'all' — personal profile at {}/{} tool calls ({}/{} servers, {}/{} sessions)",
                "i".cyan(),
                stats.tool_invocations,
                t.min_tool_invocations,
                stats.distinct_servers,
                t.min_distinct_servers,
                stats.sessions_with_mcp,
                t.min_sessions_with_mcp,
            );
        }
    }
    Ok(ProfileBootstrap::NoHistory)
}

/// After ingest: upsert personal profile and optionally activate when still on `all`.
pub fn after_ingest_profile_sync() -> Result<()> {
    let cfg = Config::load();
    let active = cfg.active_profile.as_deref().unwrap_or("all");

    if let Some(pin) = crate::experiment_plan::experiment_active_profile_pin() {
        let _ = upsert_personal_from_usage(false);
        if active != pin {
            switch(&pin, true)?;
        }
        return Ok(());
    }

    if upsert_personal_from_usage(false)? && active == "all" {
        switch("personal", true)?;
        eprintln!(
            "[ctx] Personal profile ready from MCP usage; active: personal (switch with `ctx use`)"
        );
        return Ok(());
    }

    if active != "all" {
        return Ok(());
    }

    if categories_ready(&usage_stats()) && !has_category_profiles_in_toml() {
        if generate_from_config(false).is_ok() {
            if let Some(slug) = preferred_generated_slug() {
                switch(&slug, true)?;
                eprintln!(
                    "[ctx] Category profiles generated from MCP usage; active: {slug} (switch with `ctx use`)"
                );
            }
        }
    }
    Ok(())
}

/// Generate profiles from MCP servers observed in usage history (`ctx.db`).
///
/// Each non-comms category that has at least one discovered server gets a profile.
/// Comms servers (Slack, Gmail, …) are included in every profile as communication glue.
/// A standalone `comms` profile is generated when comms servers were seen.
/// Unknown/uncategorized servers are bundled into an `other` profile.
pub fn generate_from_config(force: bool) -> Result<()> {
    let stats = usage_stats();
    if !force && !categories_ready(&stats) {
        let t = Config::load().profile_thresholds;
        bail!(
            "Category profiles need at least {} tool calls in the last {} days (you have {}). \
             Keep using MCP tools or run `ctx profile generate` after more history accumulates.",
            t.min_tool_invocations_categories,
            t.lookback_days,
            stats.tool_invocations,
        );
    }
    let prefixes = collect_observed_prefixes();
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    if prefixes.is_empty() {
        let _ = prune_stale_auto_generated_profiles()?;
        bail!(
            "No MCP usage history yet. Use Claude Code with your connectors, run `ctx ingest`, then retry."
        );
    }

    // Build category lookup from display name
    let cat_map: HashMap<&str, &str> = SERVER_CATEGORY_MAP.iter().copied().collect();

    // Partition prefixes into categories
    let mut by_category: HashMap<String, Vec<String>> = HashMap::new();
    let mut uncategorized: Vec<String> = Vec::new();
    for prefix in &prefixes {
        let display = mcp_prefix_to_server_display(prefix);
        if let Some(cat) = cat_map.get(display.as_str()) {
            by_category
                .entry(cat.to_string())
                .or_default()
                .push(prefix.clone());
        } else {
            uncategorized.push(prefix.clone());
        }
    }

    // Comms servers go into every profile
    let mut comms_servers: Vec<String> = by_category.get("comms").cloned().unwrap_or_default();
    comms_servers.sort();

    // Print discovery summary
    let total = prefixes.len();
    println!(
        "\nDiscovered {} MCP server{} from your usage history:\n",
        total,
        if total == 1 { "" } else { "s" }
    );
    let mut all_cats: Vec<&str> = by_category.keys().map(|s| s.as_str()).collect();
    all_cats.sort();
    for cat in &all_cats {
        let servers = &by_category[*cat];
        let names: Vec<String> = servers
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        println!("  {:<10}  {}", cat, names.join(", "));
    }
    if !uncategorized.is_empty() {
        let names: Vec<String> = uncategorized
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        println!("  {:<10}  {}", "other", names.join(", "));
    }

    // Load existing custom profiles so we don't wipe user edits of other slugs
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?).unwrap_or_default()
    } else {
        HashMap::new()
    };

    let mut generated: Vec<(String, usize, f32)> = Vec::new();
    let mut new_slugs: HashSet<String> = HashSet::new();
    let total_tools = dynamic_total_tools() as f32;
    let t = Config::load().profile_thresholds;
    let lookback = t.lookback_days;
    let min_per_tool = t.min_tool_invocations_per_tool;

    let mut category_slugs: Vec<String> = by_category
        .keys()
        .filter(|c| *c != "comms")
        .cloned()
        .collect();
    category_slugs.sort();

    for cat in &category_slugs {
        let Some(cat_servers) = by_category.get(cat.as_str()) else {
            continue;
        };
        if cat_servers.is_empty() {
            continue;
        }

        let mut prefix_list: Vec<String> = cat_servers.clone();
        for s in &comms_servers {
            if !prefix_list.contains(s) {
                prefix_list.push(s.clone());
            }
        }
        prefix_list.sort();

        let keep_tools = tools_for_prefixes(&prefix_list, lookback, min_per_tool);
        if keep_tools.is_empty() {
            continue;
        }

        let tool_count = keep_tools.len();
        let savings_pct = if total_tools > 0.0 {
            (1.0 - tool_count as f32 / total_tools) * 100.0
        } else {
            0.0
        };

        let cat_names: Vec<String> = cat_servers
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        let comms_names: Vec<String> = comms_servers
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        let description = if comms_names.is_empty() {
            cat_names.join(", ")
        } else {
            format!("{} + {}", cat_names.join(", "), comms_names.join(", "))
        };

        existing.insert(
            cat.to_string(),
            Profile {
                display: capitalize(cat),
                description,
                keep: vec![],
                keep_tools,
                path_patterns: default_path_patterns(cat),
                ..Default::default()
            },
        );
        new_slugs.insert(cat.clone());
        generated.push((cat.clone(), tool_count, savings_pct));
    }

    // Comms-only profile
    if !comms_servers.is_empty() {
        let names: Vec<String> = comms_servers
            .iter()
            .map(|p| mcp_prefix_to_server_display(p))
            .collect();
        let keep_tools = tools_for_prefixes(&comms_servers, lookback, min_per_tool);
        if !keep_tools.is_empty() {
            existing.insert(
                "comms".to_string(),
                Profile {
                    display: "Comms".to_string(),
                    description: names.join(", "),
                    keep: vec![],
                    keep_tools,
                    ..Default::default()
                },
            );
            new_slugs.insert("comms".into());
        }
    }

    // Catch-all for uncategorized servers
    if !uncategorized.is_empty() {
        let mut prefix_list = uncategorized.clone();
        for s in &comms_servers {
            if !prefix_list.contains(s) {
                prefix_list.push(s.clone());
            }
        }
        prefix_list.sort();
        let keep_tools = tools_for_prefixes(&prefix_list, lookback, min_per_tool);
        if !keep_tools.is_empty() {
            let names: Vec<String> = uncategorized
                .iter()
                .map(|p| mcp_prefix_to_server_display(p))
                .collect();
            existing.insert(
                "other".to_string(),
                Profile {
                    display: "Other".to_string(),
                    description: if comms_servers.is_empty() {
                        names.join(", ")
                    } else {
                        format!("{} + comms", names.join(", "))
                    },
                    keep: vec![],
                    keep_tools,
                    ..Default::default()
                },
            );
            new_slugs.insert("other".into());
        }
    }

    // Drop stale auto-generated profiles (e.g. legacy `shippo` or categories no longer observed).
    for slug in AUTO_GENERATED_PROFILE_SLUGS {
        if !new_slugs.contains(*slug) {
            existing.remove(*slug);
        }
    }

    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;

    println!(
        "\nGenerated {} profile{}:\n",
        generated.len(),
        if generated.len() == 1 { "" } else { "s" }
    );
    for (slug, tools, pct) in &generated {
        println!(
            "  {:<10}  ~{} tools   {:.0}% savings vs unfiltered",
            slug, tools, pct
        );
    }
    println!(
        "\n{} Wrote to {}",
        "✓".green().bold(),
        custom_path.display()
    );
    println!(
        "  Review with {}   activate with {}",
        "`ctx profile list`".bold(),
        "`ctx use <profile>`".bold()
    );

    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

pub fn fmt_k(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::CTX_ENV_LOCK;

    fn with_ctx_home<F: FnOnce(&tempfile::TempDir)>(f: F) {
        let _guard = CTX_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        // Isolate HOME too: rule-signal scanning reads ~/.cursor/rules and ~/.claude via
        // dirs::home_dir(), so without this a developer's real global rules leak into unit
        // tests (e.g. a rule that mentions "Notion" would flip rule-mention assertions).
        let prev_home = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());
        f(&tmp);
        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        std::env::remove_var("CTX_HOME");
    }

    // extract_cwd is pure -- no I/O, safe to test directly

    #[test]
    fn extract_cwd_from_primary_working_directory_prefix() {
        let system =
            "Primary working directory: /Users/alice/Documents/carrier-integrations-platform";
        assert_eq!(
            extract_working_directory_from_system(system),
            Some("/users/alice/documents/carrier-integrations-platform".to_string())
        );
    }

    #[test]
    fn extract_cwd_from_working_directory_prefix() {
        let system = "Working directory: /home/user/carrier_adapter_ms";
        assert_eq!(
            extract_working_directory_from_system(system),
            Some("/home/user/carrier_adapter_ms".to_string())
        );
    }

    #[test]
    fn extract_cwd_from_cwd_prefix() {
        let system = "cwd: /tmp/shippo-databricks-mcp";
        assert_eq!(
            extract_working_directory_from_system(system),
            Some("/tmp/shippo-databricks-mcp".to_string())
        );
    }

    #[test]
    fn extract_cwd_returns_none_when_no_match() {
        let system = "You are a helpful assistant working on code.";
        assert_eq!(extract_working_directory_from_system(system), None);
    }

    #[test]
    fn extract_cwd_ignores_empty_path() {
        let system = "Primary working directory:   ";
        assert_eq!(extract_working_directory_from_system(system), None);
    }

    fn write_test_profiles(tmp: &tempfile::TempDir, slug: &str, p: Profile) {
        let mut map = HashMap::new();
        map.insert(slug.to_string(), p);
        std::fs::write(
            tmp.path().join("profiles.toml"),
            toml::to_string_pretty(&map).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn auto_select_matches_visible_profile_via_cwd() {
        with_ctx_home(|tmp| {
            write_test_profiles(
                tmp,
                "work-carrier",
                Profile {
                    display: "Carrier".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Slack__".into()],
                    path_patterns: vec!["carrier-integrations".into()],
                    ..Default::default()
                },
            );

            let cwd = "/Users/alice/Documents/carrier-integrations-platform";
            let result = auto_select(cwd, "", "all");
            assert_eq!(
                result.map(|(slug, trigger)| (slug, trigger.starts_with("cwd:"))),
                Some(("work-carrier".to_string(), true))
            );
        });
    }

    #[test]
    fn auto_select_returns_none_when_already_on_correct_profile() {
        with_ctx_home(|tmp| {
            write_test_profiles(
                tmp,
                "work-carrier",
                Profile {
                    display: "Carrier".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Slack__".into()],
                    path_patterns: vec!["carrier-integrations".into()],
                    ..Default::default()
                },
            );

            let cwd = "/Users/alice/Documents/carrier-integrations-platform";
            let result = auto_select(cwd, "", "work-carrier");
            assert_eq!(
                result,
                Some((
                    "work-carrier".to_string(),
                    "cwd:carrier-integrations-platform:confirmed".to_string()
                ))
            );
        });
    }

    #[test]
    fn auto_select_path_fallback_skips_hidden_builtins() {
        with_ctx_home(|_tmp| {
            let cwd = "/Users/alice/Documents/shippo-databricks-mcp";
            let result = auto_select(cwd, "", "all");
            assert!(
                result.is_none(),
                "built-in data profile must not match when hidden"
            );
        });
    }

    #[test]
    fn auto_select_returns_none_for_unrecognised_cwd() {
        with_ctx_home(|_tmp| {
            let result = auto_select("/Users/alice/Documents/some-random-project", "", "all");
            assert!(result.is_none());
        });
    }

    #[test]
    fn auto_select_falls_back_to_keyword_on_visible_profile() {
        with_ctx_home(|tmp| {
            write_test_profiles(
                tmp,
                "work-carrier",
                Profile {
                    display: "Carrier".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Slack__".into()],
                    triggers: vec!["carrier integration".into()],
                    ..Default::default()
                },
            );

            let result = auto_select("", "carrier integration project at Shippo", "all");
            assert_eq!(
                result.map(|(slug, _)| slug),
                Some("work-carrier".to_string())
            );
        });
    }

    #[test]
    fn personal_ready_false_below_threshold() {
        let stats = UsageStats {
            tool_invocations: 5,
            distinct_servers: 2,
            sessions_with_mcp: 1,
        };
        let t = ProfileThresholds::default();
        assert!(!personal_ready_with_thresholds(&stats, &t));
    }

    #[test]
    fn personal_ready_true_at_threshold() {
        let t = ProfileThresholds::default();
        let stats = UsageStats {
            tool_invocations: t.min_tool_invocations,
            distinct_servers: t.min_distinct_servers,
            sessions_with_mcp: t.min_sessions_with_mcp,
        };
        assert!(personal_ready_with_thresholds(&stats, &t));
    }

    #[test]
    fn upsert_personal_does_not_write_when_below_threshold() {
        with_ctx_home(|tmp| {
            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            conn.execute(
                "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
                 VALUES ('s1', 'p', datetime('now'), 'all', '/tmp', 1)",
                [],
            )
            .unwrap();
            let sid: i64 = conn
                .query_row("SELECT id FROM sessions WHERE external_key='s1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            for _ in 0..5 {
                conn.execute(
                    "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                     VALUES (?1, 't', 'mcp__claude_ai_Slack__', datetime('now'))",
                    [sid],
                )
                .unwrap();
            }
            assert!(!upsert_personal_from_usage(false).unwrap());
            assert!(!tmp.path().join("profiles.toml").exists());
        });
    }

    #[test]
    fn personal_profile_keeps_rule_mentioned_tool_below_usage_threshold() {
        with_ctx_home(|tmp| {
            std::fs::write(
                tmp.path().join("config.toml"),
                "[profile_thresholds]\nmin_tool_invocations = 5\nmin_distinct_servers = 2\nmin_sessions_with_mcp = 1\nmin_tool_invocations_per_tool = 3\n",
            )
            .unwrap();

            let proj = tempfile::tempdir().unwrap();
            std::fs::write(
                proj.path().join("CLAUDE.md"),
                "Use mcp__claude_ai_Linear__create_issue for every bug fix.",
            )
            .unwrap();

            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            for (key, wd) in [("s1", proj.path()), ("s2", proj.path())] {
                conn.execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
                     VALUES (?1, 'p', datetime('now'), 'all', ?2, 1)",
                    rusqlite::params![key, wd.to_string_lossy().as_ref()],
                )
                .unwrap();
            }
            let sid1: i64 = conn
                .query_row("SELECT id FROM sessions WHERE external_key='s1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let sid2: i64 = conn
                .query_row("SELECT id FROM sessions WHERE external_key='s2'", [], |r| {
                    r.get(0)
                })
                .unwrap();

            for _ in 0..5 {
                conn.execute(
                    "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                     VALUES (?1, 'mcp__claude_ai_Slack__send', 'mcp__claude_ai_Slack__', datetime('now'))",
                    [sid1],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                 VALUES (?1, 'mcp__claude_ai_Notion__search', 'mcp__claude_ai_Notion__', datetime('now'))",
                [sid1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                 VALUES (?1, 'mcp__claude_ai_Linear__create_issue', 'mcp__claude_ai_Linear__', datetime('now'))",
                [sid2],
            )
            .unwrap();

            assert!(upsert_personal_from_usage(false).unwrap());
            let p = get("personal").unwrap();
            assert!(p
                .keep_tools
                .iter()
                .any(|t| t.contains("Linear__create_issue") || t.contains("Linear__")));
            assert!(
                !p.keep_tools.iter().any(|t| t.contains("Notion__search")),
                "Notion only invoked once and not mentioned in rules"
            );
        });
    }

    #[test]
    fn migrate_tools_errors_without_profiles_toml_before_ready() {
        with_ctx_home(|tmp| {
            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            conn.execute(
                "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
                 VALUES ('s1', 'p', datetime('now'), 'all', '/tmp', 1)",
                [],
            )
            .unwrap();
            let sid: i64 = conn
                .query_row("SELECT id FROM sessions WHERE external_key='s1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            for _ in 0..3 {
                conn.execute(
                    "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                     VALUES (?1, 'mcp__claude_ai_Slack__send', 'mcp__claude_ai_Slack__', datetime('now'))",
                    [sid],
                )
                .unwrap();
            }
            assert!(!tmp.path().join("profiles.toml").exists());
            assert!(migrate_tools(None, false).is_err());
        });
    }

    #[test]
    fn migrate_tools_converts_prefix_profile_in_toml() {
        with_ctx_home(|tmp| {
            write_test_profiles(
                tmp,
                "legacy",
                Profile {
                    display: "Legacy".into(),
                    description: "prefix keep".into(),
                    keep: vec!["mcp__claude_ai_Slack__".into()],
                    ..Default::default()
                },
            );
            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            conn.execute(
                "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
                 VALUES ('s1', 'p', datetime('now'), 'all', '/tmp', 1)",
                [],
            )
            .unwrap();
            let sid: i64 = conn
                .query_row("SELECT id FROM sessions WHERE external_key='s1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            for _ in 0..3 {
                conn.execute(
                    "INSERT INTO tool_invocations (session_id, tool_name, server_prefix, ts)
                     VALUES (?1, 'mcp__claude_ai_Slack__send', 'mcp__claude_ai_Slack__', datetime('now'))",
                    [sid],
                )
                .unwrap();
            }
            migrate_tools(Some("legacy"), false).unwrap();
            let p = get("legacy").unwrap();
            assert!(p.uses_tool_level());
            assert_eq!(p.keep_tools.len(), 1);
        });
    }

    #[test]
    fn select_by_similarity_picks_personal_from_hook_traces() {
        with_ctx_home(|tmp| {
            std::fs::write(
                tmp.path().join("config.toml"),
                "similarity_min_confidence = 0.1\n",
            )
            .unwrap();
            write_test_profiles(
                tmp,
                "personal",
                Profile {
                    display: "Personal".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Slack__".into()],
                    ..Default::default()
                },
            );

            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            let shared_embed = "data pipeline databricks fix the job";

            for ext in ["sim-a", "sim-b"] {
                conn.execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count, embed_text, tokens_saved)
                     VALUES (?1, 'p', datetime('now'), 'all', '/tmp/data-project', 1, ?2, 1000)",
                    rusqlite::params![ext, shared_embed],
                )
                .unwrap();
                let sid: i64 = conn
                    .query_row(
                        "SELECT id FROM sessions WHERE external_key=?1",
                        [ext],
                        |r| r.get(0),
                    )
                    .unwrap();
                let emb = crate::embedder::embed_text(shared_embed).unwrap();
                crate::db::set_session_embedding_blob(
                    &conn,
                    sid,
                    &crate::embedder::vec_to_blob(&emb),
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO hook_traces (ts, session_id, working_directory, profile, tokens_saved, enriched)
                     VALUES (datetime('now'), ?1, '/tmp/data-project', 'personal', 50000, 1)",
                    [ext],
                )
                .unwrap();
            }

            let m = select_by_similarity(
                "/tmp/data-project",
                "fix the databricks data pipeline job",
                "all",
            )
            .expect("similarity should match");
            assert_eq!(m.slug, "personal");
            assert!(m.based_on >= 2);
            assert!(m.avg_match > 0.0 && m.avg_match <= 1.0);
            assert!(similarity_auto_trigger(&m, false).starts_with("similarity:"));
            assert!(similarity_auto_trigger(&m, false).contains('·'));
        });
    }

    #[test]
    fn similarity_auto_trigger_includes_avg_match_and_session_count() {
        let m = ProfileMatch {
            slug: "design".into(),
            confidence: 1.0,
            based_on: 4,
            avg_match: 0.87,
        };
        assert_eq!(similarity_auto_trigger(&m, false), "similarity:0.87·4");
        assert_eq!(
            similarity_auto_trigger(&m, true),
            "similarity:0.87·4:confirmed"
        );
    }

    #[test]
    fn select_by_similarity_rejects_below_min_avg_match() {
        with_ctx_home(|tmp| {
            std::fs::write(
                tmp.path().join("config.toml"),
                "similarity_min_confidence = 0.0\nsimilarity_min_avg_match = 0.95\n",
            )
            .unwrap();
            write_test_profiles(
                tmp,
                "design",
                Profile {
                    display: "Design".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Figma__".into()],
                    ..Default::default()
                },
            );

            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();

            for (ext, etext) in [
                ("sim-a", "the gaffer figma screen layout"),
                ("sim-b", "unrelated carrier integration jira tickets"),
            ] {
                conn.execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count, embed_text, tokens_saved)
                     VALUES (?1, 'p', datetime('now'), 'all', '/tmp/the-gaffer', 1, ?2, 1000)",
                    rusqlite::params![ext, etext],
                )
                .unwrap();
                let sid: i64 = conn
                    .query_row(
                        "SELECT id FROM sessions WHERE external_key=?1",
                        [ext],
                        |r| r.get(0),
                    )
                    .unwrap();
                let emb = crate::embedder::embed_text(etext).unwrap();
                crate::db::set_session_embedding_blob(
                    &conn,
                    sid,
                    &crate::embedder::vec_to_blob(&emb),
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO hook_traces (ts, session_id, working_directory, profile, tokens_saved, enriched)
                     VALUES (datetime('now'), ?1, '/tmp/the-gaffer', 'design', 50000, 1)",
                    [ext],
                )
                .unwrap();
            }

            assert!(
                select_by_similarity("/tmp/the-gaffer", "continue the gaffer figma screen", "all",)
                    .is_none(),
                "weak mixed neighbors should fail avg_match gate"
            );
        });
    }

    #[test]
    fn select_by_similarity_votes_from_hook_traces_with_claude_jsonl_keys() {
        with_ctx_home(|tmp| {
            std::fs::write(
                tmp.path().join("config.toml"),
                "similarity_min_confidence = 0.0\n",
            )
            .unwrap();
            write_test_profiles(
                tmp,
                "design",
                Profile {
                    display: "Design".into(),
                    description: "test".into(),
                    keep: vec!["mcp__claude_ai_Figma__".into()],
                    ..Default::default()
                },
            );

            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            let shared_embed = "the gaffer figma screen layout";

            for uuid in [
                "9b23efef-5d3f-4166-96ea-d71bc966332d",
                "d7a91ab4-77b3-44db-88eb-58f8ff7b4393",
            ] {
                let ext = format!(
                    "/Users/alice/.claude/projects/-Users-alice-Documents-the-gaffer/{uuid}.jsonl"
                );
                conn.execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count, embed_text, tokens_saved)
                     VALUES (?1, 'the gaffer', datetime('now'), 'all', '/Users/alice/Documents/the-gaffer', 1, ?2, 1000)",
                    rusqlite::params![ext, shared_embed],
                )
                .unwrap();
                let sid: i64 = conn
                    .query_row(
                        "SELECT id FROM sessions WHERE external_key=?1",
                        [&ext],
                        |r| r.get(0),
                    )
                    .unwrap();
                let emb = crate::embedder::embed_text(shared_embed).unwrap();
                crate::db::set_session_embedding_blob(
                    &conn,
                    sid,
                    &crate::embedder::vec_to_blob(&emb),
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO hook_traces (ts, session_id, working_directory, profile, tokens_saved, enriched)
                     VALUES (datetime('now'), ?1, '/Users/alice/Documents/the-gaffer', 'design', 50000, 1)",
                    [uuid],
                )
                .unwrap();
            }

            let m = select_by_similarity(
                "/Users/alice/Documents/the-gaffer",
                "continue the gaffer figma screen",
                "all",
            )
            .expect("similarity should match hook traces via jsonl external_key");
            assert_eq!(m.slug, "design");
            assert!(m.based_on >= 2);
        });
    }

    #[test]
    fn allowed_server_names_maps_mcp_prefixes_to_server_ids() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let p = get("data").unwrap();
        let names = allowed_server_names_for_profile(&p);
        assert!(
            names.iter().any(|n| n == "Data_Shippo"),
            "expected Data_Shippo, got {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n == "Slack"),
            "expected Slack, got {:?}",
            names
        );
        assert!(
            names.iter().all(|n| n
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')),
            "serverName must match [a-zA-Z0-9_-], got {:?}",
            names
        );

        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn collect_observed_prefixes_empty_without_history() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        assert!(collect_observed_prefixes().is_empty());
        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn dynamic_total_tools_zero_without_history() {
        with_ctx_home(|_tmp| {
            assert_eq!(dynamic_total_tools(), 0);
            assert!(!tool_metrics_ready());
        });
    }

    #[test]
    fn builtin_templates_hidden_until_usage_profiles_exist() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let slugs = visible_profile_slugs("all");
        assert_eq!(slugs, vec!["all".to_string()]);
        assert!(!is_profile_visible("carrier", "all"));
        assert!(!is_profile_visible("data", "all"));
        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn keeps_tool_prefers_keep_tools_over_keep_prefixes() {
        let p = Profile {
            display: "t".into(),
            description: String::new(),
            keep: vec!["mcp__claude_ai_Figma__".into()],
            keep_tools: vec!["mcp__claude_ai_Slack__send".into()],
            ..Default::default()
        };
        assert!(p.keeps_tool("mcp__claude_ai_Slack__send"));
        assert!(!p.keeps_tool("mcp__claude_ai_Figma__get_file"));
    }

    #[test]
    fn is_ctx_managed_deny_pattern_matches_tool_and_wildcard() {
        assert!(is_ctx_managed_deny_pattern(
            "mcp__claude_ai_Figma__get_file"
        ));
        assert!(is_ctx_managed_deny_pattern("mcp__claude_ai_Figma__*"));
        assert!(!is_ctx_managed_deny_pattern("Read"));
    }
}
