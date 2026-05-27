use anyhow::{bail, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::config::Config;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Profile {
    pub display: String,
    pub description: String,
    /// MCP tool name prefixes to keep. Empty = keep everything (no filtering).
    pub keep: Vec<String>,
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

impl Profile {
    pub fn tool_count(&self) -> usize {
        if self.keep.is_empty() {
            return TOTAL_TOOLS;
        }
        self.keep
            .iter()
            .map(|prefix| {
                SERVER_COUNTS
                    .iter()
                    .find(|(k, _)| k.starts_with(prefix.as_str()) || prefix.starts_with(k))
                    .map(|(_, c)| *c)
                    .unwrap_or(3)
            })
            .sum()
    }

    pub fn token_cost(&self) -> usize {
        self.tool_count() * TOKENS_PER_TOOL
    }

    pub fn savings_vs_all(&self) -> usize {
        (TOTAL_TOOLS * TOKENS_PER_TOOL).saturating_sub(self.token_cost())
    }

    pub fn savings_pct(&self) -> f32 {
        1.0 - (self.tool_count() as f32 / TOTAL_TOOLS as f32)
    }

    pub fn filters_tool(&self, tool_name: &str) -> bool {
        if self.keep.is_empty() {
            return false;
        }
        // Only filter MCP tools; never touch Cursor/Claude Code built-in tools
        if !tool_name.starts_with("mcp__") {
            return false;
        }
        !self.keep.iter().any(|prefix| tool_name.starts_with(prefix.as_str()))
    }

    pub fn matches_path(&self, cwd: &str) -> bool {
        if self.path_patterns.is_empty() {
            return false;
        }
        let lower = cwd.to_lowercase();
        self.path_patterns.iter().any(|p| lower.contains(p.as_str()))
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
        },
    );

    m.insert(
        "design".into(),
        Profile {
            display: "Design".into(),
            description: "Figma, Canva, Miro, Slack, Google Drive".into(),
            keep: vec![
                "mcp__claude_ai_Figma__".into(),
                "mcp__claude_ai_Canva__".into(),
                "mcp__claude_ai_Miro__".into(),
                "mcp__claude_ai_Slack__".into(),
                "mcp__claude_ai_Google_Drive__".into(),
            ],
            path_patterns: vec![
                "design".into(),
                "frontend".into(),
                "ui-".into(),
                "figma".into(),
                "marketing".into(),
            ],
            triggers: vec![
                "figma".into(),
                "design system".into(),
                "wireframe".into(),
            ],
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
            path_patterns: vec![],
            triggers: vec![],
        },
    );

    m.insert(
        "all".into(),
        Profile {
            display: "All Tools".into(),
            description: "No filtering (current default without ctx)".into(),
            keep: vec![],
            path_patterns: vec![],
            triggers: vec![],
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

/// Find the best profile slug for the given system prompt text.
/// Returns None if no profile matches, or if the active profile already matches.
pub fn auto_select(system: &str, active_slug: &str) -> Option<(String, String)> {
    let profiles = load_all();
    let priority = ["carrier", "data", "design", "minimal"];

    // Primary: match against the working directory path (deterministic, reliable)
    if let Some(cwd) = extract_working_directory_from_system(system) {
        for slug in &priority {
            if let Some(p) = profiles.get(*slug) {
                if p.matches_path(&cwd) {
                    // Use the last directory component as the trigger label
                    let dir_label = cwd.split('/').filter(|s| !s.is_empty()).last()
                        .unwrap_or(&cwd)
                        .to_string();
                    if *slug != active_slug {
                        return Some((slug.to_string(), dir_label));
                    }
                    return None; // already on the right profile
                }
            }
        }
    }

    // Fallback: keyword scan on full system prompt text
    for slug in &priority {
        if let Some(p) = profiles.get(*slug) {
            if p.matches_system_prompt(system) {
                let matched_trigger = p.triggers.iter()
                    .find(|t| system.to_lowercase().contains(t.as_str()))
                    .cloned()
                    .unwrap_or_default();
                if *slug != active_slug {
                    return Some((slug.to_string(), matched_trigger));
                }
                return None;
            }
        }
    }
    None
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

pub fn switch(slug: &str, force: bool) -> Result<()> {
    let mut config = Config::load();
    let from_slug = config.active_profile.clone().unwrap_or_else(|| "all".into());
    let profile = get(slug)?;

    if from_slug != slug {
        let report = crate::quality_guard::safety_report(&profile.keep);
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

    let _ = crate::behavior_guard::write_behavior_hints_file();

    let pct = (profile.savings_pct() * 100.0) as u32;
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
    println!(
        "{} filter-config.json updated (picked up on the next API request)",
        "i".dimmed()
    );
    Ok(())
}

/// Build `personal` profile from MCP tool_use history (last 30 days in DB, or empty).
pub fn auto_generate(_refresh: bool) -> Result<()> {
    let mut prefixes: Vec<String> = Vec::new();
    if crate::db::db_exists() {
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);
            let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
            let mut stmt = conn.prepare(
                "SELECT DISTINCT server_prefix FROM tool_invocations WHERE ts >= ?1 ORDER BY server_prefix",
            )?;
            let rows = stmt.query_map(rusqlite::params![cutoff], |r| r.get::<_, String>(0))?;
            for row in rows {
                prefixes.push(row?);
            }
        }
    }
    if prefixes.is_empty() {
        bail!("No MCP tool history in the database yet. Run `ctx ingest` after using Claude Code, then retry.");
    }
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?)?
    } else {
        HashMap::new()
    };
    let n = prefixes.len();
    existing.insert(
        "personal".into(),
        Profile {
            display: "Personal (auto)".into(),
            description: "Auto-built from your MCP tool_use history (last 30 days)".into(),
            keep: prefixes.clone(),
            path_patterns: vec![],
            triggers: vec![],
        },
    );
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    println!(
        "{} Wrote profile `personal` with {} MCP server prefix(es) to {}",
        "✓".green().bold(),
        n,
        custom_path.display()
    );
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
}

pub fn status() -> Result<()> {
    let config = Config::load();
    let slug = config.active_profile.as_deref().unwrap_or("all");
    let profile = get(slug).unwrap_or_else(|_| Profile {
        display: "All Tools".into(),
        description: "No filtering".into(),
        keep: vec![],
        path_patterns: vec![],
        triggers: vec![],
    });

    println!("Profile:    {} ({})", slug.bold(), profile.display);
    println!("Tools:      ~{} / {} active", profile.tool_count(), TOTAL_TOOLS);
    println!(
        "Tokens/turn: ~{} (tool schemas only)",
        fmt_k(profile.token_cost())
    );
    println!(
        "Savings:    ~{} ({:.0}%) vs unfiltered",
        fmt_k(profile.savings_vs_all()),
        profile.savings_pct() * 100.0
    );

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

    Ok(())
}

pub fn list_profiles_json() -> serde_json::Value {
    let config = Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let profiles = load_all();
    let mut slugs: Vec<String> = profiles.keys().cloned().collect();
    slugs.sort();
    let items: Vec<serde_json::Value> = slugs.iter().map(|slug| {
        let p = &profiles[slug];
        serde_json::json!({
            "slug": slug,
            "display": p.display,
            "description": p.description,
            "active": slug.as_str() == active,
            "tools": p.tool_count(),
            "tokens_per_turn": p.token_cost(),
            "savings_vs_all": p.savings_vs_all(),
            "savings_pct": (p.savings_pct() * 100.0).round(),
        })
    }).collect();
    serde_json::json!(items)
}

pub fn list() -> Result<()> {
    let config = Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let profiles = load_all();

    let mut slugs: Vec<String> = profiles.keys().cloned().collect();
    slugs.sort();

    println!(
        "{:<12} {:<6} {:<11} {}",
        "PROFILE", "TOOLS", "TOKENS/TURN", "DESCRIPTION"
    );
    println!("{}", "─".repeat(58));
    for slug in &slugs {
        let p = &profiles[slug];
        let marker = if slug.as_str() == active {
            "*".green().bold().to_string()
        } else {
            " ".to_string()
        };
        println!(
            "{} {:<11} {:<6} {:<11} {}",
            marker,
            slug,
            p.tool_count(),
            fmt_k(p.token_cost()),
            p.description
        );
    }
    println!("\n* = active");
    Ok(())
}

pub fn show(slug: &str) -> Result<()> {
    let p = get(slug)?;
    println!("Profile:  {} ({})", slug.bold(), p.display);
    println!("Desc:     {}", p.description);
    println!("Tools:    ~{}", p.tool_count());
    println!("Cost:     ~{} tokens/turn", fmt_k(p.token_cost()));
    println!(
        "Savings:  ~{} ({:.0}%) vs unfiltered",
        fmt_k(p.savings_vs_all()),
        p.savings_pct() * 100.0
    );
    if p.keep.is_empty() {
        println!("Keep:     all servers (no filtering)");
    } else {
        println!("Keep:");
        for k in &p.keep {
            println!("  {k}");
        }
    }
    Ok(())
}

pub fn add(slug: &str, keep: Vec<String>) -> Result<()> {
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?)?
    } else {
        HashMap::new()
    };
    existing.insert(
        slug.to_string(),
        Profile {
            display: slug.to_string(),
            description: format!("Custom: {}", keep.join(", ")),
            keep,
            path_patterns: vec![],
            triggers: vec![],
        },
    );
    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;
    println!("{} Added profile '{slug}'", "✓".green());
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();
    Ok(())
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
    ("Shippo MCP Dev", "shippo"),
    ("Shippo MCP DEV QA", "shippo"),
];

/// Convert a server prefix back to its display name.
/// `mcp__claude_ai_Data_Shippo__` -> `Data Shippo`
fn prefix_to_display(prefix: &str) -> String {
    prefix
        .strip_prefix("mcp__claude_ai_")
        .and_then(|s| s.strip_suffix("__"))
        .unwrap_or(prefix)
        .replace('_', " ")
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Collect every server prefix seen in the analytics DB.
/// Primary source: `tool_invocations.server_prefix` (actually invoked tools).
/// Secondary source: `requests.kept_servers` + `removed_servers` JSON arrays
///   (tools that were sent, even if never invoked).
/// Falls back to the full SERVER_COUNTS list when the DB is empty.
fn collect_observed_prefixes() -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();

    if crate::db::db_exists() {
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);

            // Servers actually invoked
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DISTINCT server_prefix FROM tool_invocations",
            ) {
                let _ = stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| {
                    rows.flatten().for_each(|p| { seen.insert(p); });
                });
            }

            // Servers present in requests (kept or removed) even if never invoked
            if let Ok(mut stmt) = conn.prepare(
                "SELECT kept_servers, removed_servers FROM requests \
                 WHERE kept_servers IS NOT NULL OR removed_servers IS NOT NULL",
            ) {
                let _ = stmt.query_map([], |r| {
                    Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
                }).map(|rows| {
                    rows.flatten().for_each(|(kept, removed)| {
                        for json in [kept, removed].into_iter().flatten() {
                            if let Ok(names) = serde_json::from_str::<Vec<String>>(&json) {
                                for name in names {
                                    let prefix = format!("mcp__claude_ai_{}__", name.replace(' ', "_"));
                                    seen.insert(prefix);
                                }
                            }
                        }
                    });
                });
            }
        }
    }

    if seen.is_empty() {
        // First-time user with no history: seed from the known server table.
        return SERVER_COUNTS.iter().map(|(k, _)| k.to_string()).collect();
    }

    let mut result: Vec<String> = seen.into_iter().collect();
    result.sort();
    result
}

/// Generate profiles from the user's actual observed (or configured) MCP servers.
///
/// Each non-comms category that has at least one discovered server gets a profile.
/// Comms servers (Slack, Gmail, …) are included in every profile as communication glue.
/// A standalone `comms` profile is always generated if any comms servers were found.
/// Unknown/uncategorized servers are bundled into an `other` profile.
pub fn generate_from_config() -> Result<()> {
    let prefixes = collect_observed_prefixes();
    if prefixes.is_empty() {
        bail!(
            "No MCP servers discovered. Use Claude Code for a session, run `ctx ingest`, then retry."
        );
    }

    // Build category lookup from display name
    let cat_map: HashMap<&str, &str> = SERVER_CATEGORY_MAP.iter().copied().collect();

    // Partition prefixes into categories
    let mut by_category: HashMap<String, Vec<String>> = HashMap::new();
    let mut uncategorized: Vec<String> = Vec::new();
    for prefix in &prefixes {
        let display = prefix_to_display(prefix);
        if let Some(cat) = cat_map.get(display.as_str()) {
            by_category.entry(cat.to_string()).or_default().push(prefix.clone());
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
        "\nDiscovered {} MCP server{} from your sessions:\n",
        total,
        if total == 1 { "" } else { "s" }
    );
    let mut all_cats: Vec<&str> = by_category.keys().map(|s| s.as_str()).collect();
    all_cats.sort();
    for cat in &all_cats {
        let servers = &by_category[*cat];
        let names: Vec<String> = servers.iter().map(|p| prefix_to_display(p)).collect();
        println!("  {:<10}  {}", cat, names.join(", "));
    }
    if !uncategorized.is_empty() {
        let names: Vec<String> = uncategorized.iter().map(|p| prefix_to_display(p)).collect();
        println!("  {:<10}  {}", "other", names.join(", "));
    }

    // Load existing custom profiles so we don't wipe user edits of other slugs
    let custom_path = crate::config::ctx_dir().join("profiles.toml");
    crate::config::ensure_dir()?;
    let mut existing: HashMap<String, Profile> = if custom_path.exists() {
        toml::from_str(&std::fs::read_to_string(&custom_path)?).unwrap_or_default()
    } else {
        HashMap::new()
    };

    let mut generated: Vec<(String, usize, f32)> = Vec::new();

    for cat in &["data", "design", "work", "finance", "files", "infra", "shippo"] {
        let Some(cat_servers) = by_category.get(*cat) else { continue };

        let mut keep: Vec<String> = cat_servers.clone();
        for s in &comms_servers {
            if !keep.contains(s) {
                keep.push(s.clone());
            }
        }
        keep.sort();

        let tool_count: usize = keep
            .iter()
            .map(|p| {
                SERVER_COUNTS
                    .iter()
                    .find(|(k, _)| k.starts_with(p.as_str()) || p.starts_with(*k))
                    .map(|(_, c)| *c)
                    .unwrap_or(3)
            })
            .sum();
        let savings_pct = (1.0 - tool_count as f32 / TOTAL_TOOLS as f32) * 100.0;

        let cat_names: Vec<String> = cat_servers.iter().map(|p| prefix_to_display(p)).collect();
        let comms_names: Vec<String> = comms_servers.iter().map(|p| prefix_to_display(p)).collect();
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
                keep,
                path_patterns: vec![],
                triggers: vec![],
            },
        );
        generated.push((cat.to_string(), tool_count, savings_pct));
    }

    // Comms-only profile
    if !comms_servers.is_empty() {
        let names: Vec<String> = comms_servers.iter().map(|p| prefix_to_display(p)).collect();
        existing.insert(
            "comms".to_string(),
            Profile {
                display: "Comms".to_string(),
                description: names.join(", "),
                keep: comms_servers.clone(),
                path_patterns: vec![],
                triggers: vec![],
            },
        );
    }

    // Catch-all for uncategorized servers
    if !uncategorized.is_empty() {
        let mut keep = uncategorized.clone();
        for s in &comms_servers {
            if !keep.contains(s) {
                keep.push(s.clone());
            }
        }
        keep.sort();
        let names: Vec<String> = uncategorized.iter().map(|p| prefix_to_display(p)).collect();
        existing.insert(
            "other".to_string(),
            Profile {
                display: "Other".to_string(),
                description: format!("{} + comms", names.join(", ")),
                keep,
                path_patterns: vec![],
                triggers: vec![],
            },
        );
    }

    std::fs::write(&custom_path, toml::to_string_pretty(&existing)?)?;

    println!("\nGenerated {} profile{}:\n", generated.len(), if generated.len() == 1 { "" } else { "s" });
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

    // extract_cwd is pure -- no I/O, safe to test directly

    #[test]
    fn extract_cwd_from_primary_working_directory_prefix() {
        let system = "Primary working directory: /Users/alice/Documents/carrier-integrations-platform";
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
        assert_eq!(extract_working_directory_from_system(system), Some("/tmp/shippo-databricks-mcp".to_string()));
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

    // auto_select uses load_all() which reads defaults + optional ~/.ctx/profiles.toml.
    // Tests use CTX_HOME pointing to a temp dir so no custom profiles can interfere.

    #[test]
    fn auto_select_matches_carrier_profile_via_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let system = "Primary working directory: /Users/alice/Documents/carrier-integrations-platform\nYou are Claude Code.";
        let result = auto_select(system, "all");
        assert_eq!(result.map(|(slug, _)| slug).as_deref(), Some("carrier"));

        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn auto_select_returns_none_when_already_on_correct_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let system = "Primary working directory: /Users/alice/Documents/carrier-integrations-platform";
        let result = auto_select(system, "carrier");
        assert!(result.is_none(), "should return None when already on carrier profile");

        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn auto_select_matches_data_profile_via_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let system = "Primary working directory: /Users/alice/Documents/shippo-databricks-mcp";
        let result = auto_select(system, "all");
        assert_eq!(result.map(|(slug, _)| slug).as_deref(), Some("data"));

        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn auto_select_returns_none_for_unrecognised_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        let system = "Primary working directory: /Users/alice/Documents/some-random-project";
        let result = auto_select(system, "all");
        assert!(result.is_none());

        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn auto_select_falls_back_to_keyword_trigger() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());

        // No CWD line, but system prompt contains a carrier trigger keyword
        let system = "You are helping with a carrier integration project at Shippo.";
        let result = auto_select(system, "all");
        assert_eq!(result.map(|(slug, _)| slug).as_deref(), Some("carrier"));

        std::env::remove_var("CTX_HOME");
    }
}
