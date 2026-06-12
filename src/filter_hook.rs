//! Writes `~/.ctx/filter.js` and `~/.ctx/filter-config.json` for legacy NODE_OPTIONS setups.
//! **Deprecated:** Claude Code ships a Bun binary where `NODE_OPTIONS --require` is ignored.
//! Prefer native `allowedMcpServers` + hooks (see `claude_settings` and `ctx setup` / `ctx proxy install`).

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct ProfileExport {
    keep: Vec<String>,
    path_patterns: Vec<String>,
    triggers: Vec<String>,
}

pub const FILTER_JS_NAME: &str = "filter.js";
pub const FILTER_CONFIG_NAME: &str = "filter-config.json";

fn embedded_filter_js() -> &'static str {
    include_str!("../assets/filter.js")
}

pub fn filter_js_path() -> PathBuf {
    crate::config::ctx_dir().join(FILTER_JS_NAME)
}

pub fn filter_config_path() -> PathBuf {
    crate::config::ctx_dir().join(FILTER_CONFIG_NAME)
}

/// Writes the canonical filter script into `~/.ctx/filter.js`.
pub fn write_filter_js() -> Result<()> {
    crate::config::ensure_dir()?;
    let path = filter_js_path();
    let tmp = path.with_extension("js.tmp");
    std::fs::write(&tmp, embedded_filter_js())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Writes `filter-config.json` for the given profile slug (keep-list matches Rust `Profile::filters_tool`).
pub fn write_filter_config_for_slug(slug: &str) -> Result<()> {
    crate::config::ensure_dir()?;
    let config = crate::config::Config::load();
    let profiles = crate::profiles::load_all();
    let keep = profiles
        .get(slug)
        .map(|p| p.keep.clone())
        .unwrap_or_default();

    let mut prof_json = serde_json::Map::new();
    for (k, p) in &profiles {
        prof_json.insert(
            k.clone(),
            serde_json::to_value(ProfileExport {
                keep: p.keep.clone(),
                path_patterns: p.path_patterns.clone(),
                triggers: p.triggers.clone(),
            })?,
        );
    }

    let v = serde_json::json!({
        "profile": slug,
        "keep": keep,
        "auto_profile_enabled": config.auto_profile_enabled,
        "inject_enabled": config.inject_enabled,
        "session_budget_threshold_usd": crate::budget_guard::session_threshold_usd(),
        "dashboard_port": config.dashboard_port.unwrap_or(8789),
        "profiles": prof_json,
    });
    let path = filter_config_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn sync_filter_config_from_active_config() -> Result<()> {
    let config = crate::config::Config::load();
    let slug = config.active_profile.as_deref().unwrap_or("all");
    write_filter_config_for_slug(slug)
}

/// Absolute path to `filter.js` for `NODE_OPTIONS=--require <path>`.
pub fn filter_js_abs_path_string() -> Result<String> {
    write_filter_js()?;
    let p = filter_js_path();
    let c = std::fs::canonicalize(&p).with_context(|| format!("canonicalize {}", p.display()))?;
    Ok(c.to_string_lossy().into_owned())
}

fn path_matches_require_arg(req_path: &str, abs_filter: &Path) -> bool {
    let Ok(can_req) = Path::new(req_path).canonicalize() else {
        return req_path == abs_filter.to_string_lossy();
    };
    can_req == abs_filter
}

/// Prepends `--require <abs_filter>` unless already present.
pub fn merge_node_options_require(existing: Option<&str>, abs_filter: &str) -> String {
    let abs_path = Path::new(abs_filter);
    let Ok(can_filter) = abs_path.canonicalize() else {
        let flag = format!("--require {}", abs_filter);
        let cur = existing.unwrap_or("").trim();
        if cur.contains(&flag) {
            return cur.to_string();
        }
        if cur.is_empty() {
            return flag;
        }
        return format!("{} {}", flag, cur);
    };

    let flag = format!("--require {}", can_filter.display());
    let cur = existing.unwrap_or("").trim();
    let tokens: Vec<&str> = cur.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "--require" && i + 1 < tokens.len() {
            if path_matches_require_arg(tokens[i + 1], &can_filter) {
                return cur.to_string();
            }
        } else if let Some(rest) = tokens[i].strip_prefix("--require=") {
            if path_matches_require_arg(rest, &can_filter) {
                return cur.to_string();
            }
        }
        i += 1;
    }
    if cur.is_empty() {
        flag
    } else {
        format!("{} {}", flag, cur)
    }
}

/// Removes ctx's `--require` for `filter.js` from NODE_OPTIONS.
pub fn strip_ctx_require_from_node_options(existing: Option<&str>) -> Option<String> {
    let abs = filter_js_path();
    let can_filter = abs.canonicalize().ok();
    let cur = existing?.trim();
    if cur.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = cur.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "--require" && i + 1 < tokens.len() {
            let drop = if let Some(ref c) = can_filter {
                path_matches_require_arg(tokens[i + 1], c)
            } else {
                tokens[i + 1] == abs.to_string_lossy()
            };
            if drop {
                i += 2;
                continue;
            }
            out.push(tokens[i].to_string());
            out.push(tokens[i + 1].to_string());
            i += 2;
            continue;
        }
        if let Some(rest) = tokens[i].strip_prefix("--require=") {
            let drop = if let Some(ref c) = can_filter {
                path_matches_require_arg(rest, c)
            } else {
                rest == abs.to_string_lossy()
            };
            if drop {
                i += 1;
                continue;
            }
        }
        out.push(tokens[i].to_string());
        i += 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prepends_require() {
        let m = merge_node_options_require(None, "/tmp/filter.js");
        assert_eq!(m, "--require /tmp/filter.js");
    }

    #[test]
    fn merge_idempotent_when_flag_present() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("filter.js");
        std::fs::write(&p, "x").unwrap();
        let abs = p.canonicalize().unwrap();
        let flag = format!("--require {}", abs.display());
        let s = format!("{flag} --max-old-space-size=4096");
        let m = merge_node_options_require(Some(&s), abs.to_str().unwrap());
        assert_eq!(m, s);
    }

    #[test]
    fn strip_removes_only_ctx_require() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var("CTX_HOME").ok();
        std::env::set_var("CTX_HOME", tmp.path());
        let c = crate::filter_hook::filter_js_path();
        std::fs::write(&c, "x").unwrap();
        let abs = c.canonicalize().unwrap();
        let s = format!("--require {} --foo bar", abs.display());
        let stripped = strip_ctx_require_from_node_options(Some(&s));
        assert_eq!(stripped.as_deref(), Some("--foo bar"));
        match prev {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
    }
}
