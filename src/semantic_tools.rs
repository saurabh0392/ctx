//! Semantic tool mix from similar sessions and access-friction recovery.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::config::Config;
use crate::profiles::{self, Profile};

pub const META_TOOL_MIX_LAST: &str = "semantic_tool_mix_last";
pub const META_ACCESS_FRICTION: &str = "access_friction_counts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMixSummary {
    pub enabled: bool,
    pub source: String,
    pub neighbor_count: usize,
    pub tools: Vec<String>,
}

impl Default for ToolMixSummary {
    fn default() -> Self {
        Self {
            enabled: false,
            source: "profile-only".into(),
            neighbor_count: 0,
            tools: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessFrictionRow {
    pub tool: String,
    pub tool_display: String,
    pub count: u32,
}

/// Recommend MCP tools to un-deny based on similar past sessions.
pub fn recommend_tools_from_similar_sessions(
    conn: &Connection,
    cwd: &str,
    prompt: &str,
    profile: &Profile,
) -> Result<Vec<String>> {
    let cfg = Config::load();
    if !cfg.embeddings_enabled() || !cfg.semantic_tool_mix_enabled || !profile.filtering_enabled() {
        return Ok(vec![]);
    }

    let embedding_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    if embedding_rows < 2 {
        return Ok(vec![]);
    }

    let query_text = format!("[dir: {}] {}", cwd.trim(), prompt.trim());
    let embedding = crate::embedder::embed_text(&query_text)?;
    let top_k = cfg.semantic_tool_mix_top_k.max(1).min(20);
    let sims = crate::embedder::similar_sessions_by_query(conn, &embedding, top_k, None)?;

    let min_sim = cfg.semantic_tool_mix_min_similarity as f64;
    let min_frac = cfg.semantic_tool_mix_min_neighbor_fraction as f64;

    let qualifying: Vec<(i64, f32)> = sims
        .into_iter()
        .filter(|(_, sim)| *sim as f64 >= min_sim)
        .collect();
    if qualifying.len() < 2 {
        return Ok(vec![]);
    }

    let mut tool_weights: HashMap<String, f64> = HashMap::new();
    let mut tool_sessions: HashMap<String, HashSet<i64>> = HashMap::new();

    for (session_pk, sim) in &qualifying {
        let tools = tools_for_session(conn, *session_pk)?;
        let weight = *sim as f64;
        for tool in tools {
            *tool_weights.entry(tool.clone()).or_insert(0.0) += weight;
            tool_sessions.entry(tool).or_default().insert(*session_pk);
        }
    }

    let neighbor_n = qualifying.len() as f64;
    let min_sessions = (neighbor_n * min_frac).ceil() as usize;

    let profile_kept: HashSet<String> = if profile.uses_tool_level() {
        profile.keep_tools.iter().cloned().collect()
    } else {
        HashSet::new()
    };

    let mut recommended: Vec<String> = tool_weights
        .into_iter()
        .filter_map(|(tool, _weight)| {
            let sessions_with = tool_sessions.get(&tool).map(|s| s.len()).unwrap_or(0);
            if sessions_with < min_sessions {
                return None;
            }
            if profile.uses_tool_level() && profile_kept.contains(&tool) {
                return None;
            }
            if profile.uses_tool_level() && !profile.filters_tool(&tool) {
                return None;
            }
            Some(tool)
        })
        .collect();

    recommended.sort();
    recommended.dedup();
    Ok(recommended)
}

fn tools_for_session(conn: &Connection, session_pk: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tool_name FROM tool_invocations WHERE session_id = ?1 ORDER BY tool_name",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_pk], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn persist_tool_mix_summary(conn: &Connection, summary: &ToolMixSummary) -> Result<()> {
    let json = serde_json::to_string(summary)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
        rusqlite::params![META_TOOL_MIX_LAST, json],
    )?;
    Ok(())
}

pub fn load_tool_mix_summary(conn: &Connection) -> ToolMixSummary {
    let Some(json) = crate::db::get_meta(conn, META_TOOL_MIX_LAST) else {
        return ToolMixSummary::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Hook entry: compute semantic overlay and persist to config + meta.
pub fn apply_hook_semantic_tool_mix(new_slug: &str, prompt: &str, cwd: &str) -> Result<()> {
    let cfg = Config::load();
    if !cfg.semantic_tool_mix_enabled || cfg.filter_mode != crate::config::FilterMode::Soft {
        return Ok(());
    }

    let profile = profiles::get(new_slug)?;
    let Ok(conn) = crate::db::open_db() else {
        return Ok(());
    };
    let _ = crate::db::ensure_schema(&conn);

    let tools = recommend_tools_from_similar_sessions(&conn, cwd, prompt, &profile).unwrap_or_default();

    let mut cfg = Config::load();
    cfg.session_semantic_tools = tools.clone();
    cfg.save()?;

    let summary = if tools.is_empty() {
        ToolMixSummary {
            enabled: true,
            source: "profile-only".into(),
            neighbor_count: 0,
            tools: vec![],
        }
    } else {
        let query_text = format!("[dir: {}] {}", cwd.trim(), prompt.trim());
        let neighbor_count = if cfg.embeddings_enabled() {
            crate::embedder::embed_text(&query_text)
                .ok()
                .and_then(|emb| {
                    crate::embedder::similar_sessions_by_query(&conn, &emb, cfg.semantic_tool_mix_top_k.max(1).min(20), None).ok()
                })
                .map(|s| {
                    s.iter()
                        .filter(|(_, sim)| *sim as f64 >= cfg.semantic_tool_mix_min_similarity as f64)
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };
        ToolMixSummary {
            enabled: true,
            source: "semantic".into(),
            neighbor_count,
            tools: tools.clone(),
        }
    };

    let _ = persist_tool_mix_summary(&conn, &summary);
    Ok(())
}

const ACCESS_PATTERNS: &[&str] = &[
    "don't have access to",
    "do not have access to",
    "don't have access",
    "cannot access",
    "can't access",
    "can't use the",
    "cannot use the",
    "isn't available",
    "is not available",
    "not available",
    "don't have permission",
    "no access to",
];

/// Scan assistant text for access-friction signals against denied tools.
pub fn detect_access_friction_tools(text: &str, profile: &Profile) -> Vec<String> {
    if text.trim().is_empty() || !profile.filtering_enabled() {
        return vec![];
    }
    let lower = text.to_lowercase();
    if !ACCESS_PATTERNS.iter().any(|p| lower.contains(p)) {
        return vec![];
    }
    profiles::detect_expansion_candidates(text, "", profile)
}

fn load_friction_counts(conn: &Connection) -> HashMap<String, u32> {
    let Some(json) = crate::db::get_meta(conn, META_ACCESS_FRICTION) else {
        return HashMap::new();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

fn save_friction_counts(conn: &Connection, counts: &HashMap<String, u32>) -> Result<()> {
    let json = serde_json::to_string(counts)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
        rusqlite::params![META_ACCESS_FRICTION, json],
    )?;
    Ok(())
}

/// Record access friction and expand session targets immediately.
pub fn record_access_friction(tools: &[String]) -> Result<()> {
    if tools.is_empty() {
        return Ok(());
    }
    let Ok(conn) = crate::db::open_db() else {
        return Ok(());
    };
    let _ = crate::db::ensure_schema(&conn);

    let mut counts = load_friction_counts(&conn);
    let mut cfg = Config::load();
    let mut changed = false;

    for tool in tools {
        *counts.entry(tool.clone()).or_insert(0) += 1;
        if !cfg
            .session_expansion
            .iter()
            .any(|s| s.eq_ignore_ascii_case(tool))
        {
            cfg.session_expansion.push(tool.clone());
            changed = true;
        }
    }

    let _ = save_friction_counts(&conn, &counts);
    if changed {
        cfg.save()?;
        let slug = cfg.active_profile.as_deref().unwrap_or("all");
        let dash = cfg.dashboard_port.unwrap_or(8789);
        crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;
    }
    Ok(())
}

pub fn list_access_friction(conn: &Connection, promote_threshold: u32) -> Vec<AccessFrictionRow> {
    let counts = load_friction_counts(conn);
    let mut rows: Vec<AccessFrictionRow> = counts
        .into_iter()
        .filter(|(_, c)| *c >= 1)
        .map(|(tool, count)| {
            let tool_display = if tool.starts_with("mcp__") {
                tool.rsplit("__").next().unwrap_or(tool.as_str()).replace('_', " ")
            } else {
                tool.clone()
            };
            AccessFrictionRow {
                tool,
                tool_display,
                count,
            }
        })
        .collect();
    rows.retain(|r| r.count >= promote_threshold.min(1));
    rows.sort_by(|a, b| b.count.cmp(&a.count));
    rows
}

/// Promote a tool to profile keep_tools permanently.
pub fn promote_tool_to_profile(tool: &str) -> Result<()> {
    profiles::append_keep_tool_to_active_profile(tool)?;

    let mut cfg = Config::load();
    if !cfg.session_expansion.iter().any(|s| s.eq_ignore_ascii_case(tool)) {
        cfg.session_expansion.push(tool.to_string());
        cfg.save()?;
    }

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    if let Ok(conn) = crate::db::open_db() {
        let mut counts = load_friction_counts(&conn);
        counts.remove(tool);
        let _ = save_friction_counts(&conn, &counts);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::Profile;
    use crate::test_lock::CTX_ENV_LOCK;

    fn with_ctx_home<F: FnOnce(&tempfile::TempDir)>(f: F) {
        let _guard = CTX_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        f(&tmp);
        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn detect_access_friction_finds_notion_in_denial_text() {
        with_ctx_home(|_tmp| {
            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            let session_id = conn
                .execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile) VALUES ('t', 'p', '2026-01-01', 'design')",
                    [],
                )
                .ok();
            let sid = conn.last_insert_rowid();
            let ts = chrono::Utc::now().to_rfc3339();
            crate::db::insert_tool_invocation(
                &conn,
                sid,
                None,
                "mcp__claude_ai_Notion__notion-search",
                "mcp__claude_ai_Notion__",
                &ts,
            )
            .unwrap();

            let profile = Profile {
                display: "Design".into(),
                description: "test".into(),
                keep_tools: vec!["mcp__claude_ai_Figma__use_figma".into()],
                ..Default::default()
            };

            let tools = detect_access_friction_tools(
                "I don't have access to Notion for this task.",
                &profile,
            );
            assert!(
                tools.iter().any(|t| t.contains("Notion")),
                "expected Notion tool, got {tools:?}"
            );
            let _ = session_id;
        });
    }
}
