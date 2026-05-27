//! SQLite index for analytics requests, Claude sessions, embeddings, and quality guard tables.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::analytics::Record;

const SCHEMA_VERSION: i32 = 6;

pub fn open_db() -> Result<Connection> {
    let path = crate::config::db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA synchronous=NORMAL;",
    )?;
    Ok(conn)
}

pub fn db_exists() -> bool {
    crate::config::db_path().exists()
}

fn migrate_hook_traces_adaptive_fired(conn: &Connection) {
    let table_exists: bool = conn
        .prepare("SELECT 1 FROM hook_traces LIMIT 0")
        .is_ok();
    if !table_exists {
        return;
    }
    let has: bool = conn
        .prepare("SELECT adaptive_fired FROM hook_traces LIMIT 0")
        .is_ok();
    if !has {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN adaptive_fired INTEGER DEFAULT 0",
            [],
        );
    }
}

fn migrate_hook_traces_power_columns(conn: &Connection) {
    let table_exists: bool = conn
        .prepare("SELECT 1 FROM hook_traces LIMIT 0")
        .is_ok();
    if !table_exists {
        return;
    }
    if conn.prepare("SELECT mode FROM hook_traces LIMIT 0").is_err() {
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN mode TEXT", []);
    }
    if conn
        .prepare("SELECT parent_session_id FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN parent_session_id TEXT",
            [],
        );
    }
}

fn migrate_hook_traces_ab_columns(conn: &Connection) {
    let table_exists: bool = conn
        .prepare("SELECT 1 FROM hook_traces LIMIT 0")
        .is_ok();
    if !table_exists {
        return;
    }
    if conn.prepare("SELECT ab_group FROM hook_traces LIMIT 0").is_err() {
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN ab_group TEXT", []);
    }
    if conn
        .prepare("SELECT human_text_prefix FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN human_text_prefix TEXT",
            [],
        );
    }
}

fn migrate_hook_traces_savings_columns(conn: &Connection) {
    let table_exists: bool = conn
        .prepare("SELECT 1 FROM hook_traces LIMIT 0")
        .is_ok();
    if !table_exists {
        return;
    }
    let has_tools_kept: bool = conn
        .prepare("SELECT tools_kept FROM hook_traces LIMIT 0")
        .is_ok();
    if !has_tools_kept {
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN tools_kept INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN tools_removed INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN tokens_saved INTEGER DEFAULT 0", []);
    }
}

pub fn ensure_schema(conn: &Connection) -> Result<()> {
    // Run column migrations unconditionally (idempotent ALTER TABLE checks)
    migrate_hook_traces_savings_columns(conn);
    migrate_hook_traces_adaptive_fired(conn);
    migrate_hook_traces_ab_columns(conn);
    migrate_hook_traces_power_columns(conn);

    let v: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if v >= SCHEMA_VERSION {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            k TEXT PRIMARY KEY,
            v TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            profile TEXT,
            tools_removed INTEGER DEFAULT 0,
            tokens_saved INTEGER DEFAULT 0,
            compress_chars_saved INTEGER DEFAULT 0,
            auto_selected INTEGER DEFAULT 0,
            auto_trigger TEXT,
            inject_fired INTEGER DEFAULT 0,
            coach_kind TEXT,
            budget_fired INTEGER DEFAULT 0,
            behavior_kind TEXT,
            working_directory TEXT,
            tools_sent_count INTEGER DEFAULT 0,
            removed_servers TEXT,
            kept_servers TEXT,
            mcp_tools_invoked TEXT,
            tools_sent_by_server TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_requests_ts ON requests(ts);

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            external_key TEXT NOT NULL UNIQUE,
            project TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            duration_mins INTEGER,
            request_count INTEGER DEFAULT 0,
            tools_removed INTEGER DEFAULT 0,
            tokens_saved INTEGER DEFAULT 0,
            cost_saved REAL DEFAULT 0,
            profile TEXT,
            working_directory TEXT,
            turn_count INTEGER DEFAULT 0,
            total_usd REAL DEFAULT 0,
            input_tokens INTEGER DEFAULT 0,
            cache_creation_tokens INTEGER DEFAULT 0,
            cache_read_tokens INTEGER DEFAULT 0,
            output_tokens INTEGER DEFAULT 0,
            models_used TEXT,
            hit_compact INTEGER DEFAULT 0,
            clarifying_turns INTEGER DEFAULT 0,
            correction_turns INTEGER DEFAULT 0,
            first_user_message TEXT,
            embed_text TEXT,
            top_turns_json TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);
        CREATE INDEX IF NOT EXISTS idx_sessions_wd ON sessions(working_directory);
        CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project);

        CREATE TABLE IF NOT EXISTS turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            turn_index INTEGER NOT NULL,
            role TEXT NOT NULL,
            cost_usd REAL DEFAULT 0,
            input_tokens INTEGER DEFAULT 0,
            output_tokens INTEGER DEFAULT 0,
            cache_read_tokens INTEGER DEFAULT 0,
            cache_creation_tokens INTEGER DEFAULT 0,
            model TEXT,
            flags TEXT,
            human_text_prefix TEXT,
            ts TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);

        CREATE TABLE IF NOT EXISTS session_embeddings (
            session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
            embedding BLOB NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tool_invocations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER REFERENCES sessions(id) ON DELETE CASCADE,
            turn_id INTEGER REFERENCES turns(id) ON DELETE CASCADE,
            tool_name TEXT NOT NULL,
            server_prefix TEXT NOT NULL,
            ts TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_tool_inv_server ON tool_invocations(server_prefix);
        CREATE INDEX IF NOT EXISTS idx_tool_inv_ts ON tool_invocations(ts);
        CREATE INDEX IF NOT EXISTS idx_tool_inv_session_server ON tool_invocations(session_id, server_prefix);

        CREATE TABLE IF NOT EXISTS profile_changes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            from_profile TEXT,
            to_profile TEXT,
            servers_added TEXT,
            servers_removed TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_profile_changes_ts ON profile_changes(ts);

        CREATE TABLE IF NOT EXISTS hook_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            hook_type TEXT NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_hook_events_ts ON hook_events(ts);
        CREATE INDEX IF NOT EXISTS idx_hook_events_type ON hook_events(hook_type);

        CREATE TABLE IF NOT EXISTS hook_traces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            working_directory TEXT,
            profile TEXT,
            auto_selected INTEGER DEFAULT 0,
            auto_trigger TEXT,
            inject_fired INTEGER DEFAULT 0,
            coach_kind TEXT,
            budget_fired INTEGER DEFAULT 0,
            tools_kept INTEGER DEFAULT 0,
            tools_removed INTEGER DEFAULT 0,
            tokens_saved INTEGER DEFAULT 0,
            adaptive_fired INTEGER DEFAULT 0,
            -- enriched by ingest (NULL until matched)
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_creation_tokens INTEGER,
            cost_usd REAL,
            model TEXT,
            enriched INTEGER DEFAULT 0,
            ab_group TEXT,
            human_text_prefix TEXT,
            mode TEXT,
            parent_session_id TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_hook_traces_ts ON hook_traces(ts);
        CREATE INDEX IF NOT EXISTS idx_hook_traces_session ON hook_traces(session_id);
        "#,
    )?;

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

pub fn insert_hook_event(conn: &Connection, hook_type: &str, payload_json: &str) -> Result<()> {
    ensure_schema(conn)?;
    let ts = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO hook_events (ts, hook_type, payload) VALUES (?1, ?2, ?3)",
        params![ts, hook_type, payload_json],
    )?;
    stamp_ctx_active_since(conn);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct HookEventRow {
    pub id: i64,
    pub ts: String,
    pub hook_type: String,
    pub payload: String,
}

pub fn load_hook_events(
    conn: &Connection,
    limit: usize,
    offset: usize,
    ts_since: Option<&str>,
) -> Result<Vec<HookEventRow>> {
    ensure_schema(conn)?;
    fn map_ev(r: &rusqlite::Row<'_>) -> rusqlite::Result<HookEventRow> {
        Ok(HookEventRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            hook_type: r.get(2)?,
            payload: r.get(3)?,
        })
    }
    let mut out = Vec::new();
    if let Some(s) = ts_since {
        let mut stmt = conn.prepare(
            "SELECT id, ts, hook_type, payload FROM hook_events WHERE ts >= ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![s, limit as i64, offset as i64], map_ev)?;
        out.extend(rows.filter_map(|x| x.ok()));
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, ts, hook_type, payload FROM hook_events ORDER BY id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], map_ev)?;
        out.extend(rows.filter_map(|x| x.ok()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Hook traces — lightweight rows from UserPromptSubmit, enriched by ingest
// ---------------------------------------------------------------------------

pub fn insert_hook_trace(
    conn: &Connection,
    session_id: Option<&str>,
    parent_session_id: Option<&str>,
    working_directory: &str,
    profile: &str,
    mode: Option<&str>,
    auto_selected: bool,
    auto_trigger: Option<&str>,
    inject_fired: bool,
    coach_kind: Option<&str>,
    budget_fired: bool,
    tools_kept: usize,
    tools_removed: usize,
    tokens_saved: usize,
    adaptive_fired: bool,
    ab_group: Option<&str>,
) -> Result<i64> {
    ensure_schema(conn)?;
    let ts = chrono::Utc::now().to_rfc3339();
    conn.execute(
        r#"INSERT INTO hook_traces (
            ts, session_id, parent_session_id, working_directory, profile, mode,
            auto_selected, auto_trigger, inject_fired, coach_kind, budget_fired,
            tools_kept, tools_removed, tokens_saved, adaptive_fired, ab_group
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"#,
        params![
            ts,
            session_id,
            parent_session_id,
            working_directory,
            profile,
            mode,
            auto_selected as i64,
            auto_trigger,
            inject_fired as i64,
            coach_kind,
            budget_fired as i64,
            tools_kept as i64,
            tools_removed as i64,
            tokens_saved as i64,
            adaptive_fired as i64,
            ab_group,
        ],
    )?;
    stamp_ctx_active_since(conn);
    Ok(conn.last_insert_rowid())
}

#[derive(serde::Serialize)]
pub struct HookTraceRow {
    pub id: i64,
    pub ts: String,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub mode: Option<String>,
    pub working_directory: String,
    pub profile: String,
    pub auto_selected: bool,
    pub auto_trigger: Option<String>,
    pub inject_fired: bool,
    pub coach_kind: Option<String>,
    pub budget_fired: bool,
    pub tools_kept: usize,
    pub tools_removed: usize,
    pub tokens_saved: usize,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
    pub enriched: bool,
    pub adaptive_fired: bool,
    pub ab_group: Option<String>,
    pub human_text_prefix: Option<String>,
}

pub fn load_hook_traces(
    conn: &Connection,
    limit: usize,
    offset: usize,
    ts_since: Option<&str>,
) -> Result<Vec<HookTraceRow>> {
    ensure_schema(conn)?;
    let base = r#"SELECT
        id, ts, session_id, parent_session_id, mode,
        COALESCE(working_directory, '') AS working_directory,
        COALESCE(profile, '') AS profile,
        COALESCE(auto_selected, 0) AS auto_selected,
        auto_trigger,
        COALESCE(inject_fired, 0) AS inject_fired,
        coach_kind,
        COALESCE(budget_fired, 0) AS budget_fired,
        COALESCE(tools_kept, 0) AS tools_kept,
        COALESCE(tools_removed, 0) AS tools_removed,
        COALESCE(tokens_saved, 0) AS tokens_saved,
        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
        cost_usd, model,
        COALESCE(enriched, 0) AS enriched,
        COALESCE(adaptive_fired, 0) AS adaptive_fired,
        ab_group,
        human_text_prefix
    FROM hook_traces"#;
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(HookTraceRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            session_id: r.get(2)?,
            parent_session_id: r.get(3)?,
            mode: r.get(4)?,
            working_directory: r.get(5)?,
            profile: r.get(6)?,
            auto_selected: r.get::<_, i64>(7)? != 0,
            auto_trigger: r.get(8)?,
            inject_fired: r.get::<_, i64>(9)? != 0,
            coach_kind: r.get(10)?,
            budget_fired: r.get::<_, i64>(11)? != 0,
            tools_kept: r.get::<_, i64>(12)? as usize,
            tools_removed: r.get::<_, i64>(13)? as usize,
            tokens_saved: r.get::<_, i64>(14)? as usize,
            input_tokens: r.get(15)?,
            output_tokens: r.get(16)?,
            cache_read_tokens: r.get(17)?,
            cache_creation_tokens: r.get(18)?,
            cost_usd: r.get(19)?,
            model: r.get(20)?,
            enriched: r.get::<_, i64>(21)? != 0,
            adaptive_fired: r.get::<_, i64>(22)? != 0,
            ab_group: r.get(23)?,
            human_text_prefix: r.get(24)?,
        })
    };
    let mut out = Vec::new();
    if let Some(s) = ts_since {
        let sql = format!("{base} WHERE ts >= ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![s, limit as i64, offset as i64], map_row)?;
        for row in rows {
            out.push(row?);
        }
    } else {
        let sql = format!("{base} ORDER BY id DESC LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], map_row)?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// Match unenriched hook_trace rows to the nearest JSONL turn by session + timestamp.
/// Called during ingest after sessions/turns are populated.
pub fn enrich_hook_traces(conn: &Connection) -> Result<usize> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, ts, session_id FROM hook_traces WHERE enriched = 0",
    )?;
    let pending: Vec<(i64, String, Option<String>)> = stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?.filter_map(|x| x.ok()).collect();

    let mut count = 0usize;
    for (trace_id, trace_ts, session_id) in &pending {
        // Try to find the turn closest in time to this hook trace.
        // Join through sessions table using external_key LIKE %session_id%
        // or fall back to pure timestamp proximity across all turns.
        let matched: Option<(i64, i64, i64, i64, f64, String, Option<String>)> =
            if let Some(sid) = session_id {
                conn.query_row(
                    r#"SELECT t.input_tokens, t.output_tokens, t.cache_read_tokens,
                              t.cache_creation_tokens, t.cost_usd, t.model, t.human_text_prefix
                       FROM turns t
                       JOIN sessions s ON t.session_id = s.id
                       WHERE s.external_key LIKE '%' || ?1 || '%'
                         AND t.ts IS NOT NULL
                       ORDER BY ABS(julianday(t.ts) - julianday(?2))
                       LIMIT 1"#,
                    params![sid, trace_ts],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ))
                    },
                )
                .optional()?
            } else {
                None
            };

        // Fall back to timestamp-only match if session_id didn't resolve
        let matched = matched.or_else(|| {
            conn
                .query_row(
                    r#"SELECT input_tokens, output_tokens, cache_read_tokens,
                              cache_creation_tokens, cost_usd, model, human_text_prefix
                       FROM turns
                       WHERE ts IS NOT NULL
                       ORDER BY ABS(julianday(ts) - julianday(?1))
                       LIMIT 1"#,
                    params![trace_ts],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ))
                    },
                )
                .optional()
                .ok()
                .flatten()
        });

        if let Some((inp, outp, cr, cc, cost, model, human_prefix)) = matched {
            conn.execute(
                r#"UPDATE hook_traces SET
                    input_tokens = ?1, output_tokens = ?2, cache_read_tokens = ?3,
                    cache_creation_tokens = ?4, cost_usd = ?5, model = ?6, enriched = 1,
                    human_text_prefix = ?7
                   WHERE id = ?8"#,
                params![inp, outp, cr, cc, cost, model, human_prefix, trace_id],
            )?;
            count += 1;
        }
    }
    let _ = backfill_parent_session_ids(conn);
    Ok(count)
}

/// Infer parent_session_id for rows still NULL when another trace lists them as parent.
fn backfill_parent_session_ids(conn: &Connection) -> Result<()> {
    conn.execute(
        r#"UPDATE hook_traces
           SET parent_session_id = (
             SELECT h2.parent_session_id FROM hook_traces h2
             WHERE h2.session_id = hook_traces.session_id
               AND h2.parent_session_id IS NOT NULL
             LIMIT 1
           )
           WHERE parent_session_id IS NULL
             AND session_id IS NOT NULL
             AND EXISTS (
               SELECT 1 FROM hook_traces h3
               WHERE h3.parent_session_id = hook_traces.session_id
             )"#,
        [],
    )?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskCostChild {
    pub session_id: String,
    pub cost_usd: f64,
    pub requests: u64,
    pub tokens_saved: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskCostGroup {
    pub parent_session: String,
    pub working_directory: String,
    pub children: Vec<TaskCostChild>,
    pub total_cost_usd: f64,
    pub total_requests: u64,
}

/// Group hook trace costs by parent session (subagent tasks).
pub fn load_task_costs(conn: &Connection) -> Result<Vec<TaskCostGroup>> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare(
        r#"SELECT
            COALESCE(parent_session_id, session_id, 'unknown') AS group_key,
            session_id,
            COALESCE(working_directory, '') AS working_directory,
            COALESCE(cost_usd, 0.0) AS cost_usd,
            COALESCE(tokens_saved, 0) AS tokens_saved
           FROM hook_traces
           WHERE enriched = 1"#,
    )?;
    let rows: Vec<(String, Option<String>, String, f64, i64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?
        .filter_map(|x| x.ok())
        .collect();

    use std::collections::BTreeMap;
    struct Agg {
        working_directory: String,
        children: BTreeMap<String, (f64, u64, u64)>,
        total_cost: f64,
        total_requests: u64,
    }
    let mut groups: BTreeMap<String, Agg> = BTreeMap::new();
    for (group_key, session_id, cwd, cost, tokens_saved) in rows {
        let sid = session_id.unwrap_or_else(|| group_key.clone());
        let agg = groups.entry(group_key).or_insert_with(|| Agg {
            working_directory: cwd.clone(),
            children: BTreeMap::new(),
            total_cost: 0.0,
            total_requests: 0,
        });
        if agg.working_directory.is_empty() && !cwd.is_empty() {
            agg.working_directory = cwd;
        }
        agg.total_cost += cost;
        agg.total_requests += 1;
        let entry = agg.children.entry(sid).or_insert((0.0, 0, 0));
        entry.0 += cost;
        entry.1 += 1;
        entry.2 += tokens_saved.max(0) as u64;
    }

    let mut out: Vec<TaskCostGroup> = groups
        .into_iter()
        .map(|(parent_session, agg)| {
            let children: Vec<TaskCostChild> = agg
                .children
                .into_iter()
                .map(|(session_id, (cost_usd, requests, tokens_saved))| TaskCostChild {
                    session_id,
                    cost_usd,
                    requests,
                    tokens_saved,
                })
                .collect();
            TaskCostGroup {
                parent_session,
                working_directory: agg.working_directory,
                children,
                total_cost_usd: agg.total_cost,
                total_requests: agg.total_requests,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.total_cost_usd
            .partial_cmp(&a.total_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Record when ctx first became active. Called on first request insert or hook event.
/// Writes only once; subsequent calls are no-ops.
pub fn stamp_ctx_active_since(conn: &Connection) {
    let existing: Option<String> = conn
        .query_row("SELECT v FROM meta WHERE k = 'ctx_active_since'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    if existing.is_some() {
        return;
    }
    let ts = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO meta (k, v) VALUES ('ctx_active_since', ?1)",
        params![ts],
    );
}

pub fn get_ctx_active_since(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT v FROM meta WHERE k = 'ctx_active_since'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

/// Clear the install watermark so the dashboard shows all historical rows again until the next hook or request stamps it.
pub fn reset_ctx_active_since(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE k = 'ctx_active_since'", [])?;
    Ok(())
}

pub fn insert_request(conn: &Connection, rec: &Record) -> Result<i64> {
    ensure_schema(conn)?;
    let removed = serde_json::to_string(&rec.removed_servers)?;
    let kept = serde_json::to_string(&rec.kept_servers)?;
    let mcp = serde_json::to_string(&rec.mcp_tools_invoked)?;
    let by_srv = serde_json::to_string(&rec.tools_sent_by_server)?;
    conn.execute(
        r#"INSERT INTO requests (
            ts, profile, tools_removed, tokens_saved, compress_chars_saved,
            auto_selected, auto_trigger, inject_fired, coach_kind, budget_fired, behavior_kind,
            working_directory, tools_sent_count, removed_servers, kept_servers, mcp_tools_invoked,
            tools_sent_by_server
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"#,
        params![
            rec.ts,
            rec.profile,
            rec.tools_removed as i64,
            rec.tokens_saved as i64,
            rec.compress_chars_saved as i64,
            rec.auto_selected as i64,
            rec.auto_trigger,
            rec.inject_fired as i64,
            rec.coach_kind,
            rec.budget_fired as i64,
            rec.behavior_kind,
            rec.working_directory,
            rec.tools_sent_count as i64,
            removed,
            kept,
            mcp,
            by_srv,
        ],
    )?;
    stamp_ctx_active_since(conn);
    Ok(conn.last_insert_rowid())
}

pub fn load_requests_ordered(conn: &Connection) -> Result<Vec<Record>> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT ts, profile, tools_removed, tokens_saved, compress_chars_saved,
                auto_selected, auto_trigger, inject_fired, coach_kind, budget_fired, behavior_kind,
                working_directory, tools_sent_count, removed_servers, kept_servers, mcp_tools_invoked,
                tools_sent_by_server
         FROM requests ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        let removed_s: String = r.get(13)?;
        let kept_s: String = r.get(14)?;
        let mcp_s: String = r.get(15)?;
        let by_srv_s: String = r.get(16)?;
        Ok(Record {
            ts: r.get(0)?,
            profile: r.get(1)?,
            tools_removed: r.get::<_, i64>(2)? as usize,
            tokens_saved: r.get::<_, i64>(3)? as usize,
            compress_chars_saved: r.get::<_, i64>(4)? as usize,
            auto_selected: r.get::<_, i64>(5)? != 0,
            auto_trigger: r.get(6)?,
            inject_fired: r.get::<_, i64>(7)? != 0,
            coach_kind: r.get(8)?,
            budget_fired: r.get::<_, i64>(9)? != 0,
            behavior_kind: r.get(10)?,
            working_directory: r.get(11)?,
            tools_sent_count: r.get::<_, i64>(12)? as usize,
            removed_servers: serde_json::from_str(&removed_s).unwrap_or_default(),
            kept_servers: serde_json::from_str(&kept_s).unwrap_or_default(),
            mcp_tools_invoked: serde_json::from_str(&mcp_s).unwrap_or_default(),
            tools_sent_by_server: serde_json::from_str(&by_srv_s).unwrap_or_default(),
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn request_count(conn: &Connection) -> Result<i64> {
    ensure_schema(conn)?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0))?;
    Ok(n)
}

pub fn session_count(conn: &Connection) -> Result<i64> {
    ensure_schema(conn)?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
    Ok(n)
}

pub fn maybe_backfill_requests_from_jsonl(conn: &Connection) -> Result<()> {
    ensure_schema(conn)?;
    let done: Option<String> = conn
        .query_row(
            "SELECT v FROM meta WHERE k = 'backfill_requests_v1'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if done.as_deref() == Some("1") {
        return Ok(());
    }
    let n = request_count(conn)?;
    if n > 0 {
        conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('backfill_requests_v1', '1')",
            [],
        )?;
        return Ok(());
    }
    let path = crate::config::analytics_path();
    if !path.exists() {
        conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('backfill_requests_v1', '1')",
            [],
        )?;
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<Record>(line) {
            let _ = insert_request(conn, &rec);
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('backfill_requests_v1', '1')",
        [],
    )?;
    Ok(())
}

pub fn insert_profile_change(
    conn: &Connection,
    from_profile: &str,
    to_profile: &str,
    servers_added: &str,
    servers_removed: &str,
) -> Result<()> {
    ensure_schema(conn)?;
    let ts = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO profile_changes (ts, from_profile, to_profile, servers_added, servers_removed)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![ts, from_profile, to_profile, servers_added, servers_removed],
    )?;
    Ok(())
}

fn duration_mins_between(started_at: &str, ended_at: Option<&str>) -> i64 {
    let Ok(s) = chrono::DateTime::parse_from_rfc3339(started_at) else {
        return 0;
    };
    let Some(e_str) = ended_at else {
        return 0;
    };
    let Ok(e) = chrono::DateTime::parse_from_rfc3339(e_str) else {
        return 0;
    };
    (e.with_timezone(&chrono::Utc) - s.with_timezone(&chrono::Utc))
        .num_minutes()
        .max(0)
}

/// Upsert a Claude session row. Returns the stable `sessions.id`.
pub fn upsert_claude_session(
    conn: &Connection,
    external_key: &str,
    project: &str,
    started_at: &str,
    ended_at: Option<&str>,
    turn_count: i64,
    total_usd: f64,
    input_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    output_tokens: i64,
    models_used_json: &str,
    hit_compact: i64,
    clarifying_turns: i64,
    correction_turns: i64,
    first_user_message: &str,
    embed_text: &str,
    working_directory: &str,
    top_turns_json: &str,
) -> Result<i64> {
    ensure_schema(conn)?;
    let ended = ended_at.unwrap_or(started_at);
    let dur = duration_mins_between(started_at, Some(ended));

    conn.execute(
        r#"INSERT INTO sessions (
            external_key, project, started_at, ended_at, duration_mins,
            turn_count, total_usd, input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens,
            models_used, hit_compact, clarifying_turns, correction_turns,
            first_user_message, embed_text, working_directory, top_turns_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        ON CONFLICT(external_key) DO UPDATE SET
            project = excluded.project,
            started_at = excluded.started_at,
            ended_at = excluded.ended_at,
            duration_mins = excluded.duration_mins,
            turn_count = excluded.turn_count,
            total_usd = excluded.total_usd,
            input_tokens = excluded.input_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            output_tokens = excluded.output_tokens,
            models_used = excluded.models_used,
            hit_compact = excluded.hit_compact,
            clarifying_turns = excluded.clarifying_turns,
            correction_turns = excluded.correction_turns,
            first_user_message = excluded.first_user_message,
            embed_text = excluded.embed_text,
            working_directory = excluded.working_directory,
            top_turns_json = excluded.top_turns_json"#,
        params![
            external_key,
            project,
            started_at,
            ended,
            dur,
            turn_count,
            total_usd,
            input_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            output_tokens,
            models_used_json,
            hit_compact,
            clarifying_turns,
            correction_turns,
            first_user_message,
            embed_text,
            working_directory,
            top_turns_json,
        ],
    )?;

    let id: i64 = conn.query_row(
        "SELECT id FROM sessions WHERE external_key = ?1",
        params![external_key],
        |r| r.get(0),
    )?;
    Ok(id)
}

pub fn replace_session_turns(conn: &Connection, session_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM session_embeddings WHERE session_id = ?1",
        params![session_id],
    )?;
    conn.execute(
        "DELETE FROM tool_invocations WHERE session_id = ?1",
        params![session_id],
    )?;
    conn.execute("DELETE FROM turns WHERE session_id = ?1", params![session_id])?;
    Ok(())
}

pub fn insert_turn(
    conn: &Connection,
    session_id: i64,
    turn_index: i64,
    role: &str,
    cost_usd: f64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    model: &str,
    flags_json: &str,
    human_text_prefix: &str,
    ts: Option<&str>,
) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO turns (
            session_id, turn_index, role, cost_usd, input_tokens, output_tokens,
            cache_read_tokens, cache_creation_tokens, model, flags, human_text_prefix, ts
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
        params![
            session_id,
            turn_index,
            role,
            cost_usd,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            model,
            flags_json,
            human_text_prefix,
            ts,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_tool_invocation(
    conn: &Connection,
    session_id: i64,
    turn_id: Option<i64>,
    tool_name: &str,
    server_prefix: &str,
    ts: &str,
) -> Result<()> {
    conn.execute(
        r#"INSERT INTO tool_invocations (session_id, turn_id, tool_name, server_prefix, ts)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![session_id, turn_id, tool_name, server_prefix, ts],
    )?;
    Ok(())
}

pub fn set_session_embedding_blob(conn: &Connection, session_id: i64, blob: &[u8]) -> Result<()> {
    ensure_schema(conn)?;
    conn.execute(
        "INSERT INTO session_embeddings (session_id, embedding) VALUES (?1, ?2)
         ON CONFLICT(session_id) DO UPDATE SET embedding = excluded.embedding",
        params![session_id, blob],
    )?;
    Ok(())
}

pub fn list_embedding_rows(
    conn: &Connection,
) -> Result<Vec<(i64, Vec<u8>)>> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare("SELECT session_id, embedding FROM session_embeddings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    let mut v = Vec::new();
    for row in rows {
        v.push(row?);
    }
    Ok(v)
}

/// Clear prompt-derived columns and all embedding vectors (privacy).
pub fn purge_prompt_text_columns(conn: &Connection) -> Result<()> {
    ensure_schema(conn)?;
    conn.execute(
        "UPDATE sessions SET first_user_message = '', embed_text = '', top_turns_json = '[]'",
        [],
    )?;
    conn.execute("UPDATE turns SET human_text_prefix = ''", [])?;
    conn.execute("DELETE FROM session_embeddings", [])?;
    Ok(())
}

/// Remove all indexed sessions, turns, tool rows, embeddings, requests, and profile change history.
pub fn delete_all_indexed_data(conn: &Connection) -> Result<()> {
    ensure_schema(conn)?;
    conn.execute("DELETE FROM session_embeddings", [])?;
    conn.execute("DELETE FROM tool_invocations", [])?;
    conn.execute("DELETE FROM turns", [])?;
    conn.execute("DELETE FROM sessions", [])?;
    conn.execute("DELETE FROM requests", [])?;
    conn.execute("DELETE FROM profile_changes", [])?;
    Ok(())
}

/// Returns display names of MCP servers that were loaded (present in kept_servers) in the
/// last `lookback_days` days of requests but never actually invoked in that same window.
/// These are pure token waste candidates the user should add to a profile's strip list.
///
/// kept_servers stores JSON arrays of display names like ["Databricks", "Slack"].
/// tool_invocations.server_prefix stores prefixes like "mcp__claude_ai_Data_Shippo__".
/// We convert kept display names to prefix form for comparison.
pub fn zero_usage_servers(conn: &Connection, lookback_days: u32) -> Result<Vec<String>> {
    let cutoff = format!("now, '-{lookback_days} days'");
    // Collect all kept display names from recent requests
    let mut stmt = conn.prepare(&format!(
        "SELECT kept_servers FROM requests WHERE ts > datetime({cutoff}) AND kept_servers IS NOT NULL"
    ))?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;

    let mut kept: std::collections::HashSet<String> = std::collections::HashSet::new();
    for json in &rows {
        if let Ok(names) = serde_json::from_str::<Vec<String>>(json) {
            for n in names {
                kept.insert(n);
            }
        }
    }

    if kept.is_empty() {
        return Ok(vec![]);
    }

    // Collect all invoked server_prefixes from the same window
    let mut stmt2 = conn.prepare(&format!(
        "SELECT DISTINCT server_prefix FROM tool_invocations WHERE ts > datetime({cutoff})"
    ))?;
    let invoked: std::collections::HashSet<String> = stmt2
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;

    // A kept display name is "unused" when no invoked prefix starts with the
    // canonical mcp__claude_ai_<name>__ form (spaces -> underscores).
    let mut unused: Vec<String> = kept
        .into_iter()
        .filter(|name| {
            let prefix = format!(
                "mcp__claude_ai_{}__",
                name.replace(' ', "_").replace('-', "_")
            );
            !invoked.iter().any(|p| p.starts_with(&prefix) || prefix.starts_with(p.as_str()))
        })
        .collect();
    unused.sort();
    Ok(unused)
}
