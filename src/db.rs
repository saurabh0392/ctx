//! SQLite index for analytics requests, Claude sessions, embeddings, and quality guard tables.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::analytics::Record;

// Bumped to 8 for the tool_misses table (CTX-66 / M-D); the CREATE TABLE batch is version-gated, so
// a new table only lands on existing installs when this rises.
const SCHEMA_VERSION: i32 = 8;

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
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
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
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT mode FROM hook_traces LIMIT 0")
        .is_err()
    {
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
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT ab_group FROM hook_traces LIMIT 0")
        .is_err()
    {
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
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    let has_tools_kept: bool = conn
        .prepare("SELECT tools_kept FROM hook_traces LIMIT 0")
        .is_ok();
    if !has_tools_kept {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN tools_kept INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN tools_removed INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN tokens_saved INTEGER DEFAULT 0",
            [],
        );
    }
}

fn migrate_hook_traces_prefix_and_budget_columns(conn: &Connection) {
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT inject_chars FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN inject_chars INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN adaptive_chars INTEGER DEFAULT 0",
            [],
        );
    }
    if conn
        .prepare("SELECT budget_blocked FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN budget_blocked INTEGER DEFAULT 0",
            [],
        );
    }
}

fn migrate_hook_traces_pinned_profile(conn: &Connection) {
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT pinned_profile FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute("ALTER TABLE hook_traces ADD COLUMN pinned_profile TEXT", []);
    }
    if conn
        .prepare("SELECT effective_profile FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN effective_profile TEXT",
            [],
        );
    }
}

fn migrate_hook_traces_expansion_column(conn: &Connection) {
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT tools_expanded_json FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN tools_expanded_json TEXT DEFAULT '[]'",
            [],
        );
    }
}

fn migrate_hook_traces_compress_columns(conn: &Connection) {
    let table_exists: bool = conn.prepare("SELECT 1 FROM hook_traces LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT compress_chars_saved FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN compress_chars_saved INTEGER DEFAULT 0",
            [],
        );
    }
    if conn
        .prepare("SELECT compress_event_count FROM hook_traces LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE hook_traces ADD COLUMN compress_event_count INTEGER DEFAULT 0",
            [],
        );
    }
}

fn migrate_requests_prefix_and_budget_columns(conn: &Connection) {
    let table_exists: bool = conn.prepare("SELECT 1 FROM requests LIMIT 0").is_ok();
    if !table_exists {
        return;
    }
    if conn
        .prepare("SELECT inject_chars FROM requests LIMIT 0")
        .is_err()
    {
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN inject_chars INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN adaptive_chars INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN budget_blocked INTEGER DEFAULT 0",
            [],
        );
    }
}

fn migrate_allowance_snapshots_table(conn: &Connection) {
    let _ = conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS allowance_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            model TEXT,
            window TEXT NOT NULL,
            used_pct REAL NOT NULL,
            remaining_pct REAL,
            resets_at INTEGER,
            session_cost_usd REAL
        );
        CREATE INDEX IF NOT EXISTS idx_allowance_ts ON allowance_snapshots(ts);
        CREATE INDEX IF NOT EXISTS idx_allowance_window_ts ON allowance_snapshots(window, ts);
        "#,
    );
}

fn migrate_compress_events_table(conn: &Connection) {
    let _ = conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS compress_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            tool_name TEXT NOT NULL,
            strategy TEXT NOT NULL,
            chars_in INTEGER NOT NULL,
            chars_out INTEGER NOT NULL,
            command_or_path TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_compress_events_ts ON compress_events(ts);
        CREATE INDEX IF NOT EXISTS idx_compress_events_strategy ON compress_events(strategy);

        CREATE TABLE IF NOT EXISTS compress_line_fingerprints (
            session_id TEXT NOT NULL,
            line_hash INTEGER NOT NULL,
            first_ts TEXT NOT NULL,
            PRIMARY KEY (session_id, line_hash)
        );

        CREATE TABLE IF NOT EXISTS compress_output_fingerprints (
            session_id TEXT NOT NULL,
            fingerprint INTEGER NOT NULL,
            first_ts TEXT NOT NULL,
            PRIMARY KEY (session_id, fingerprint)
        );
        "#,
    );
}

/// Self-labeling shadow store: every tool result becomes one decision row that an
/// ingest pass later joins to its outcome (correction / re-read). This is the Act 0
/// training corpus. `applied=0` rows are shadow (decision recorded, output untouched).
fn migrate_compress_decisions_table(conn: &Connection) {
    let _ = conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS compress_decisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            tool_name TEXT NOT NULL,
            server_prefix TEXT,
            kind TEXT NOT NULL,
            task_mode TEXT NOT NULL,
            lines_total INTEGER NOT NULL,
            lines_keep INTEGER NOT NULL,
            lines_drop INTEGER NOT NULL,
            chars_in INTEGER NOT NULL,
            would_chars_out INTEGER NOT NULL,
            features_json TEXT NOT NULL,
            command_or_path TEXT,
            applied INTEGER NOT NULL DEFAULT 0,
            outcome_correction INTEGER,
            outcome_reread INTEGER,
            outcome_joined INTEGER NOT NULL DEFAULT 0,
            surface TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_compress_decisions_ts ON compress_decisions(ts);
        CREATE INDEX IF NOT EXISTS idx_compress_decisions_session ON compress_decisions(session_id);
        CREATE INDEX IF NOT EXISTS idx_compress_decisions_tool ON compress_decisions(tool_name);
        CREATE INDEX IF NOT EXISTS idx_compress_decisions_joined ON compress_decisions(outcome_joined);
        "#,
    );
    // Idempotent: existing DBs predate the surface-provenance column. The label join
    // stamps which agent surface produced a decision so training can exclude the
    // lower-confidence (transcript-derived) labels until their precision is proven.
    let _ = conn.execute("ALTER TABLE compress_decisions ADD COLUMN surface TEXT", []);
    // CTX-51 L2: link a trim to its rewind entry and record when the agent recovered it via
    // ctx_expand, so the gate can treat a recovery as a benign outcome, not a harmful re-read.
    let _ = conn.execute("ALTER TABLE compress_decisions ADD COLUMN rewind_id TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE compress_decisions ADD COLUMN outcome_recovered INTEGER",
        [],
    );
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS rewind_store (
            id TEXT PRIMARY KEY,
            ts TEXT NOT NULL,
            session_id TEXT,
            tool_name TEXT NOT NULL,
            command_or_path TEXT,
            original TEXT NOT NULL,
            chars INTEGER NOT NULL,
            expanded_at TEXT,
            trimmed TEXT
        )",
        [],
    );
    let _ = conn.execute("ALTER TABLE rewind_store ADD COLUMN expanded_at TEXT", []);
    let _ = conn.execute("ALTER TABLE rewind_store ADD COLUMN trimmed TEXT", []);
    // Phase 2 randomized exploration arm (ADR 0009): "treatment" (trimmed) or "control"
    // (deliberately kept) for rows that entered the experiment; NULL for every prior and
    // non-experiment decision. Kept separate from `applied` because a randomized control and an
    // ordinary shadow row are both applied=0 but mean very different things.
    let _ = conn.execute(
        "ALTER TABLE compress_decisions ADD COLUMN explore_arm TEXT",
        [],
    );
    // Observation-only richer outcome signals (ADR 0019 / CTX-32): a JSON array of signal
    // names that fired for this decision within the outcome window (e.g. ["reread","reedit",
    // "correction_explicit"]). Recorded so each signal's precision can be spot-checked before
    // any of them is allowed to influence the gate. The gate and the learned model do NOT read
    // this column; it is purely for the audit.
    let _ = conn.execute(
        "ALTER TABLE compress_decisions ADD COLUMN outcome_signals TEXT",
        [],
    );
    // Same-file edit-follow label (CTX-46 / ADR 0031): 1 when the same file this read touched was
    // edited (an edit/write tool) within the outcome window, NULL/0 otherwise. Distinct from
    // `outcome_reread` (which counts any later same-path touch, read or edit) and from
    // `outcome_correction` (the causal harm label). This is the observational "the agent needed
    // this read whole" signal the file-aware retention model will train on (CTX-46 increment 3);
    // recording it here is safe and never feeds the causal gate.
    let _ = conn.execute(
        "ALTER TABLE compress_decisions ADD COLUMN outcome_edit_follow INTEGER",
        [],
    );
}

/// Cursor compaction events captured live from the `preCompact` hook (CTX-31, ADR 0023).
/// Cursor's transcript carries no compaction marker, so unlike Claude Code (whose compactions
/// are read from the `turns` table) Cursor's are recorded here as they happen. `message_count`
/// is the conversation position Cursor reports, kept for the later correction-followup join.
fn migrate_cursor_compactions_table(conn: &Connection) {
    let _ = conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cursor_compactions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            trigger TEXT,
            context_usage_percent REAL,
            context_tokens INTEGER,
            context_window_size INTEGER,
            message_count INTEGER,
            messages_to_compact INTEGER,
            is_first_compaction INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_cursor_compactions_session ON cursor_compactions(session_id);
        CREATE INDEX IF NOT EXISTS idx_cursor_compactions_ts ON cursor_compactions(ts);
        "#,
    );
}

/// A Cursor compaction event from the `preCompact` hook, ready to persist.
#[derive(Debug, Clone, Default)]
pub struct CursorCompaction {
    pub ts: String,
    pub session_id: Option<String>,
    pub trigger: Option<String>,
    pub context_usage_percent: Option<f64>,
    pub context_tokens: Option<i64>,
    pub context_window_size: Option<i64>,
    pub message_count: Option<i64>,
    pub messages_to_compact: Option<i64>,
    pub is_first_compaction: Option<bool>,
}

/// Record one live Cursor compaction. Best-effort: the hook never fails the Cursor session.
pub fn insert_cursor_compaction(conn: &Connection, c: &CursorCompaction) -> Result<()> {
    conn.execute(
        r#"INSERT INTO cursor_compactions
            (ts, session_id, trigger, context_usage_percent, context_tokens,
             context_window_size, message_count, messages_to_compact, is_first_compaction)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
        params![
            c.ts,
            c.session_id,
            c.trigger,
            c.context_usage_percent,
            c.context_tokens,
            c.context_window_size,
            c.message_count,
            c.messages_to_compact,
            c.is_first_compaction.map(|b| if b { 1i64 } else { 0i64 }),
        ],
    )?;
    Ok(())
}

/// One shadow/active retention decision recorded for forward label collection.
#[derive(Debug, Clone)]
pub struct CompressDecision<'a> {
    pub ts: &'a str,
    pub session_id: Option<&'a str>,
    pub tool_name: &'a str,
    pub server_prefix: Option<&'a str>,
    pub kind: &'a str,
    pub task_mode: &'a str,
    pub lines_total: usize,
    pub lines_keep: usize,
    pub lines_drop: usize,
    pub chars_in: usize,
    pub would_chars_out: usize,
    pub features_json: &'a str,
    pub command_or_path: &'a str,
    pub applied: bool,
    /// Phase 2 exploration arm (ADR 0009): Some("treatment"|"control") when the decision was part
    /// of the randomized experiment, None otherwise.
    pub explore_arm: Option<&'a str>,
    /// Originating agent surface, stamped at insert for surfaces ctx observes live (e.g. "cursor"
    /// from the Cursor postToolUse hook, ADR 0018). `None` for Claude Code, whose surface is
    /// stamped at outcome-join time so legacy rows keep their existing behaviour.
    pub surface: Option<&'a str>,
}

pub fn insert_compress_decision(conn: &Connection, d: &CompressDecision<'_>) -> Result<()> {
    conn.execute(
        r#"INSERT INTO compress_decisions
            (ts, session_id, tool_name, server_prefix, kind, task_mode,
             lines_total, lines_keep, lines_drop, chars_in, would_chars_out,
             features_json, command_or_path, applied, explore_arm, surface)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"#,
        params![
            d.ts,
            d.session_id,
            d.tool_name,
            d.server_prefix,
            d.kind,
            d.task_mode,
            d.lines_total as i64,
            d.lines_keep as i64,
            d.lines_drop as i64,
            d.chars_in as i64,
            d.would_chars_out as i64,
            d.features_json,
            d.command_or_path,
            if d.applied { 1 } else { 0 },
            d.explore_arm,
            d.surface,
        ],
    )?;
    Ok(())
}

/// Progress of the Act 0 collection window: how many decision rows exist, how many are
/// joined to an outcome, and the per-tool breakdown the Learning home shows.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CompressDecisionStats {
    pub total: i64,
    pub joined: i64,
    pub corrections_caused: i64,
    pub shadow: i64,
    pub active: i64,
    pub today: i64,
}

/// One command or path within a tool that spent context, for the expanded bill detail.
#[derive(Debug, Default, serde::Serialize)]
pub struct ContextBillSource {
    pub label: String,
    pub calls: i64,
    pub sink_chars: i64,
}

/// A stored trim of a tool the user can re-expand from the bill (CTX-57 drill-down).
#[derive(Debug, Default, serde::Serialize)]
pub struct ContextBillRewind {
    pub id: String,
    pub source: String,
    pub chars: i64,
    pub expanded: bool,
}

/// One tool's line on the context bill.
#[derive(Debug, Default, serde::Serialize)]
pub struct ContextBillTool {
    pub tool: String,
    pub decisions: i64,
    /// Total characters this tool poured into the agent's context.
    pub sink_chars: i64,
    /// Characters a trim would drop (what is on the table).
    pub reclaimable_chars: i64,
    /// Characters ctx actually removed on applied trims.
    pub reclaimed_chars: i64,
    /// Top commands or paths within this tool, biggest first.
    pub sources: Vec<ContextBillSource>,
    /// Recent verbatim trims of this tool the user can re-expand (CTX-57 drill-down).
    pub rewinds: Vec<ContextBillRewind>,
}

/// One day of context volume for the leaner-or-heavier trend (CTX-57).
#[derive(Debug, Default, serde::Serialize)]
pub struct BillDay {
    pub day: String,
    pub sink_chars: i64,
    pub reclaimable_chars: i64,
}

/// Where context goes, itemized from `compress_decisions`. Needs no labels, so it renders on day
/// one: ranked output sinks with what ctx could reclaim and what it already has.
#[derive(Debug, Default, serde::Serialize)]
pub struct ContextBill {
    pub tools: Vec<ContextBillTool>,
    pub total_sink_chars: i64,
    pub total_reclaimable_chars: i64,
    pub total_reclaimed_chars: i64,
    pub decisions: i64,
    pub since: Option<String>,
    pub trend: Vec<BillDay>,
}

/// One MCP server's line on the Tool Menu Bill: the full catalog it ships every request against
/// what the developer actually invokes. Counts and ratios are exact; token figures are the count
/// times a flat per-schema estimate (`ToolMenuBill::tokens_per_tool`), so the UI can label them.
#[derive(Debug, Default, serde::Serialize)]
pub struct ToolMenuBillServer {
    pub server: String,
    pub prefix: String,
    /// Full menu carried on every request (the fixed input tax).
    pub catalog_tools: i64,
    /// catalog_tools * tokens_per_tool: tokens paid per request whether used or not.
    pub carried_tokens: i64,
    /// Distinct tools actually invoked in the window.
    pub invoked_tools: i64,
    /// Total invocations in the window.
    pub calls: i64,
    /// catalog_tools - invoked_tools: tools carried but never called.
    pub dead_tools: i64,
    /// dead_tools * tokens_per_tool: reclaimable input tax per request.
    pub dead_tokens: i64,
    /// dead_tools / catalog_tools, 0..1.
    pub dead_ratio: f64,
    pub last_used: Option<String>,
    /// True when this server is currently pruned from the tool menu (CTX-64), so the UI shows an
    /// "undo" instead of a "prune". Set by the API handler from config, not computed here.
    pub pruned: bool,
    /// Reaches for a hidden tool of this server in the window (CTX-66): the harm a prune must not
    /// raise. Set by the API handler from `tool_miss_stats`, not computed here.
    pub misses: i64,
    /// Earn-it gate stage for auto-prune (CTX-67): active | watching | candidate | earned | blocked.
    /// Set by the API handler from `server_prune_outcomes`, not computed here.
    pub prune_stage: String,
    /// Sessions this server has run hidden so far, and how many clean ones the gate needs to
    /// confirm the prune is safe. Lets the UI teach what "proving it's safe" actually means.
    pub hidden_sessions: i64,
    pub hidden_needed: i64,
}

/// The input-side Context Bill: the fixed per-request tool-menu tax, itemized per server and ranked
/// by dead weight. The mirror of `context_bill` (the output/result tax). Built from real
/// `tool_invocations` against measured server catalogs, no new tracking. CTX-63 / M-A.
#[derive(Debug, Default, serde::Serialize)]
pub struct ToolMenuBill {
    pub servers: Vec<ToolMenuBillServer>,
    pub total_catalog_tools: i64,
    /// Fixed input tax paid on every request, all servers summed.
    pub total_carried_tokens: i64,
    pub total_invoked_tools: i64,
    /// Reclaimable input tax per request (dead weight across all servers).
    pub total_dead_tokens: i64,
    /// The flat per-schema token estimate used, surfaced so the UI can label the figures.
    pub tokens_per_tool: i64,
    pub biggest_dead_server: Option<String>,
    pub biggest_dead_tokens: i64,
    pub lookback_days: i64,
    pub since: Option<String>,
    /// Tool-miss harm read (CTX-66 / M-D): reaches for hidden tools in the window, and the coarse
    /// per-session rate the earn-it gate holds prunes against. Set by the API handler.
    pub total_misses: i64,
    pub miss_rate: f64,
    pub miss_sessions: i64,
}

/// One repo ctx has recorded decisions for, for the shareable report's repo picker (CTX-56).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoSummary {
    pub repo_key: String,
    pub decisions: i64,
    pub sink_chars: i64,
}

/// Repos ctx has data for, biggest sink first. `repo_key` comes from `features_json` (the repo the
/// decision was recorded in); rows without one fold into "(unknown)".
pub fn list_repos(conn: &Connection) -> Vec<RepoSummary> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(json_extract(features_json,'$.repo_key'),'(unknown)') AS repo,
                COUNT(*), COALESCE(SUM(chars_in),0)
         FROM compress_decisions
         GROUP BY repo
         ORDER BY 3 DESC",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok(RepoSummary {
                repo_key: r.get(0)?,
                decisions: r.get(1)?,
                sink_chars: r.get(2)?,
            })
        }) {
            out.extend(rows.flatten());
        }
    }
    out
}

/// Per-repo Context Bill for the shareable report (CTX-56). Same shape as `context_bill`, scoped to
/// one repo via the `repo_key` recorded in `features_json`. Rewinds and trend are left empty: the
/// export is a static, sendable snapshot of where context went, not the live drill-down.
pub fn repo_bill(conn: &Connection, repo_key: &str) -> ContextBill {
    let mut bill = ContextBill::default();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT tool_name,
                COUNT(*),
                COALESCE(SUM(chars_in), 0),
                COALESCE(SUM(chars_in - would_chars_out), 0),
                COALESCE(SUM(CASE WHEN applied = 1 THEN chars_in - would_chars_out ELSE 0 END), 0)
         FROM compress_decisions
         WHERE COALESCE(json_extract(features_json,'$.repo_key'),'(unknown)') = ?1
         GROUP BY tool_name
         ORDER BY 3 DESC",
    ) {
        if let Ok(rows) = stmt.query_map(params![repo_key], |r| {
            Ok(ContextBillTool {
                tool: r.get(0)?,
                decisions: r.get(1)?,
                sink_chars: r.get(2)?,
                reclaimable_chars: r.get(3)?,
                reclaimed_chars: r.get(4)?,
                sources: Vec::new(),
                rewinds: Vec::new(),
            })
        }) {
            for t in rows.flatten() {
                bill.total_sink_chars += t.sink_chars;
                bill.total_reclaimable_chars += t.reclaimable_chars;
                bill.total_reclaimed_chars += t.reclaimed_chars;
                bill.decisions += t.decisions;
                bill.tools.push(t);
            }
        }
    }
    let mut idx = std::collections::HashMap::new();
    for (i, t) in bill.tools.iter().enumerate() {
        idx.insert(t.tool.clone(), i);
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT tool_name,
                COALESCE(NULLIF(command_or_path, ''), '(unlabeled)'),
                COUNT(*),
                COALESCE(SUM(chars_in), 0)
         FROM compress_decisions
         WHERE COALESCE(json_extract(features_json,'$.repo_key'),'(unknown)') = ?1
         GROUP BY tool_name, command_or_path
         ORDER BY SUM(chars_in) DESC",
    ) {
        if let Ok(rows) = stmt.query_map(params![repo_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ContextBillSource {
                    label: r.get(1)?,
                    calls: r.get(2)?,
                    sink_chars: r.get(3)?,
                },
            ))
        }) {
            for (tool, src) in rows.flatten() {
                if let Some(&i) = idx.get(&tool) {
                    if bill.tools[i].sources.len() < 8 {
                        bill.tools[i].sources.push(src);
                    }
                }
            }
        }
    }
    bill
}

/// A verbatim tool output ctx trimmed, kept so the agent can re-expand it on demand (CTX-51).
#[derive(Debug, Default, serde::Serialize)]
pub struct RewindEntry {
    pub id: String,
    pub ts: String,
    pub tool_name: String,
    pub command_or_path: String,
    pub original: String,
    pub trimmed: String,
    pub chars: i64,
}

/// Store a trimmed original, keyed by a content id, so a later `ctx_expand` returns it verbatim.
/// Self-creates the table and keeps the store bounded so the DB does not grow without limit.
pub fn insert_rewind(
    conn: &Connection,
    id: &str,
    ts: &str,
    session_id: Option<&str>,
    tool_name: &str,
    command_or_path: &str,
    original: &str,
    trimmed: &str,
) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS rewind_store (
            id TEXT PRIMARY KEY,
            ts TEXT NOT NULL,
            session_id TEXT,
            tool_name TEXT NOT NULL,
            command_or_path TEXT,
            original TEXT NOT NULL,
            chars INTEGER NOT NULL,
            trimmed TEXT
        )",
        [],
    );
    let _ = conn.execute("ALTER TABLE rewind_store ADD COLUMN trimmed TEXT", []);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO rewind_store
         (id, ts, session_id, tool_name, command_or_path, original, chars, trimmed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            ts,
            session_id,
            tool_name,
            command_or_path,
            original,
            original.chars().count() as i64,
            trimmed
        ],
    );
    // Keep only the most recent entries so verbatim blobs do not accumulate forever.
    let _ = conn.execute(
        "DELETE FROM rewind_store WHERE id NOT IN
         (SELECT id FROM rewind_store ORDER BY ts DESC LIMIT 500)",
        [],
    );
}

/// Mark a stored trim as recovered when the agent re-expands it (CTX-51 L2). A recovery is a
/// benign outcome, so the gate later discounts it from the tool's harmful re-read count.
pub fn mark_rewind_expanded(conn: &Connection, id: &str) {
    let _ = conn.execute("ALTER TABLE rewind_store ADD COLUMN expanded_at TEXT", []);
    let _ = conn.execute(
        "UPDATE rewind_store SET expanded_at = ?2 WHERE id = ?1 AND expanded_at IS NULL",
        params![id, chrono::Utc::now().to_rfc3339()],
    );
}

/// Link the just-recorded applied decision to the rewind entry it produced, so a later recovery
/// joins back to it. Runs right after the decision insert, so the latest applied row for this
/// session and tool is the one to stamp.
pub fn link_decision_rewind(
    conn: &Connection,
    session_id: Option<&str>,
    tool_name: &str,
    rewind_id: &str,
) {
    let _ = conn.execute(
        "UPDATE compress_decisions SET rewind_id = ?3
         WHERE id = (
             SELECT MAX(id) FROM compress_decisions
             WHERE applied = 1 AND tool_name = ?2 AND session_id IS ?1
         )",
        params![session_id, tool_name, rewind_id],
    );
}

/// Fetch a stored original by id. None if the id is unknown or the store is empty.
pub fn get_rewind(conn: &Connection, id: &str) -> Option<RewindEntry> {
    conn.query_row(
        "SELECT id, ts, tool_name, COALESCE(command_or_path, ''), original, chars, COALESCE(trimmed, '')
         FROM rewind_store WHERE id = ?1",
        params![id],
        |r| {
            Ok(RewindEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                tool_name: r.get(2)?,
                command_or_path: r.get(3)?,
                original: r.get(4)?,
                chars: r.get(5)?,
                trimmed: r.get(6)?,
            })
        },
    )
    .ok()
}

/// Aggregate per-tool "suspected trim cost" (CTX-54). Single-case causation is unprovable: the
/// branch where the same session ran with the full output is in no log. So this never claims a
/// trim caused anything. It reads the offline signals ingest already computed and counts, per
/// tool, how often an *applied* trim was followed by the agent behaving as if it needed the
/// dropped content: it worked around the trim, re-read the source, or asked for the verbatim
/// original back via `ctx_expand`. Aggregated over many trims that is an honest per-tool risk
/// read; on any single trim it is only a suspect. This replaces the always-zero "gate
/// corrections" headline with something the data actually supports.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ToolAttribution {
    pub tool: String,
    /// Applied trims that dropped lines: the only trims we can attribute a cost to.
    pub applied_trims: i64,
    /// The agent read the source another way after the trim (compression_workaround signal).
    pub workaround: i64,
    /// The agent re-touched the trimmed path/command within the window (reread signal).
    pub reread: i64,
    /// The agent pulled the verbatim original back with ctx_expand. Closest-to-causal signal
    /// available without an A/B: it asked for exactly what the trim dropped.
    pub reexpanded: i64,
    /// Applied trims with any of the above. The union, so a trim with two signals counts once.
    pub suspect: i64,
    /// suspect / applied_trims, the aggregate rate. The "confidence" is this rate over N, not a
    /// per-case verdict.
    pub suspect_rate: f64,
}

pub fn tool_attribution(conn: &Connection) -> Vec<ToolAttribution> {
    let mut out = Vec::new();
    // Exclude ctx self-dev rows the same way the model corpus does: building and editing ctx is the
    // developer's own churn (exactly the source-heavy work trimming hurts most), so counting it would
    // overstate the per-tool suspect rate for everyone else. `features_json` is unambiguous here;
    // the LEFT-joined rewind_store has no such column.
    let sql = format!(
        "SELECT d.tool_name,
                SUM(CASE WHEN d.applied=1 AND d.lines_drop>0 THEN 1 ELSE 0 END),
                SUM(CASE WHEN d.applied=1 AND d.lines_drop>0
                         AND d.outcome_signals LIKE '%compression_workaround%' THEN 1 ELSE 0 END),
                SUM(CASE WHEN d.applied=1 AND d.lines_drop>0
                         AND d.outcome_signals LIKE '%reread%' THEN 1 ELSE 0 END),
                SUM(CASE WHEN d.applied=1 AND d.lines_drop>0
                         AND r.expanded_at IS NOT NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN d.applied=1 AND d.lines_drop>0 AND (
                         d.outcome_signals LIKE '%compression_workaround%'
                      OR d.outcome_signals LIKE '%reread%'
                      OR d.outcome_signals LIKE '%reedit%'
                      OR r.expanded_at IS NOT NULL) THEN 1 ELSE 0 END)
         FROM compress_decisions d
         LEFT JOIN rewind_store r ON d.rewind_id = r.id
         WHERE 1=1{EXCLUDE_SELF_DEV}
         GROUP BY d.tool_name
         HAVING SUM(CASE WHEN d.applied=1 AND d.lines_drop>0 THEN 1 ELSE 0 END) > 0
         ORDER BY 6 DESC, 2 DESC"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return out;
    };
    let rows = stmt.query_map([], |r| {
        let applied_trims: i64 = r.get(1)?;
        let suspect: i64 = r.get(5)?;
        Ok(ToolAttribution {
            tool: r.get(0)?,
            applied_trims,
            workaround: r.get(2)?,
            reread: r.get(3)?,
            reexpanded: r.get(4)?,
            suspect,
            suspect_rate: if applied_trims > 0 {
                suspect as f64 / applied_trims as f64
            } else {
                0.0
            },
        })
    });
    if let Ok(rows) = rows {
        out.extend(rows.flatten());
    }
    out
}

pub fn context_bill(conn: &Connection) -> ContextBill {
    let mut bill = ContextBill::default();
    let mut stmt = match conn.prepare(
        "SELECT tool_name,
                COUNT(*),
                COALESCE(SUM(chars_in), 0),
                COALESCE(SUM(chars_in - would_chars_out), 0),
                COALESCE(SUM(CASE WHEN applied = 1 THEN chars_in - would_chars_out ELSE 0 END), 0)
         FROM compress_decisions
         GROUP BY tool_name
         ORDER BY 3 DESC",
    ) {
        Ok(s) => s,
        Err(_) => return bill,
    };
    let rows = stmt.query_map([], |r| {
        Ok(ContextBillTool {
            tool: r.get(0)?,
            decisions: r.get(1)?,
            sink_chars: r.get(2)?,
            reclaimable_chars: r.get(3)?,
            reclaimed_chars: r.get(4)?,
            sources: Vec::new(),
            rewinds: Vec::new(),
        })
    });
    if let Ok(rows) = rows {
        for t in rows.flatten() {
            bill.total_sink_chars += t.sink_chars;
            bill.total_reclaimable_chars += t.reclaimable_chars;
            bill.total_reclaimed_chars += t.reclaimed_chars;
            bill.decisions += t.decisions;
            bill.tools.push(t);
        }
    }
    // Top sources (command or path) per tool for the expanded detail. Ordered by size globally,
    // so the first rows seen for each tool are its biggest.
    let mut idx = std::collections::HashMap::new();
    for (i, t) in bill.tools.iter().enumerate() {
        idx.insert(t.tool.clone(), i);
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT tool_name,
                COALESCE(NULLIF(command_or_path, ''), '(unlabeled)'),
                COUNT(*),
                COALESCE(SUM(chars_in), 0)
         FROM compress_decisions
         GROUP BY tool_name, command_or_path
         ORDER BY SUM(chars_in) DESC",
    ) {
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ContextBillSource {
                    label: r.get(1)?,
                    calls: r.get(2)?,
                    sink_chars: r.get(3)?,
                },
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                if let Some(&i) = idx.get(&row.0) {
                    if bill.tools[i].sources.len() < 6 {
                        bill.tools[i].sources.push(row.1);
                    }
                }
            }
        }
    }
    // Recent verbatim trims per tool for the drill-down (CTX-57).
    if let Ok(mut stmt) = conn.prepare(
        "SELECT tool_name, id, COALESCE(command_or_path, ''), chars,
                CASE WHEN expanded_at IS NOT NULL THEN 1 ELSE 0 END
         FROM rewind_store ORDER BY ts DESC",
    ) {
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ContextBillRewind {
                    id: r.get(1)?,
                    source: r.get(2)?,
                    chars: r.get(3)?,
                    expanded: r.get::<_, i64>(4)? == 1,
                },
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                if let Some(&i) = idx.get(&row.0) {
                    if bill.tools[i].rewinds.len() < 5 {
                        bill.tools[i].rewinds.push(row.1);
                    }
                }
            }
        }
    }

    // Per-day context volume for the leaner-or-heavier trend (CTX-57).
    if let Ok(mut stmt) = conn.prepare(
        "SELECT substr(ts, 1, 10) AS day,
                COALESCE(SUM(chars_in), 0),
                COALESCE(SUM(chars_in - would_chars_out), 0)
         FROM compress_decisions
         GROUP BY day ORDER BY day DESC LIMIT 14",
    ) {
        let rows = stmt.query_map([], |r| {
            Ok(BillDay {
                day: r.get(0)?,
                sink_chars: r.get(1)?,
                reclaimable_chars: r.get(2)?,
            })
        });
        if let Ok(rows) = rows {
            let mut days: Vec<BillDay> = rows.flatten().collect();
            days.reverse();
            bill.trend = days;
        }
    }

    bill.since = conn
        .query_row("SELECT MIN(ts) FROM compress_decisions", [], |r| {
            r.get::<_, Option<String>>(0)
        })
        .ok()
        .flatten();
    bill
}

/// The input-side Context Bill (CTX-63 / M-A): the per-server tool-menu tax over the last
/// `lookback_days`. Carried = the full catalog each connected server ships every request; invoked =
/// what was actually called (real `tool_invocations`); dead weight = the difference, ranked biggest
/// first. Only servers with invocation history in the window appear, so it renders on this
/// machine's own data with no new tracking. Token figures are counts times a flat per-schema
/// estimate; the multiplier is returned so the UI can label them.
pub fn tool_menu_bill(conn: &Connection, lookback_days: u32) -> ToolMenuBill {
    let tpt = crate::profiles::TOKENS_PER_TOOL as i64;
    let mut bill = ToolMenuBill {
        tokens_per_tool: tpt,
        lookback_days: lookback_days as i64,
        ..Default::default()
    };

    let cutoff = (chrono::Utc::now() - chrono::Duration::days(lookback_days as i64)).to_rfc3339();

    let mut stmt = match conn.prepare(
        "SELECT server_prefix,
                COUNT(DISTINCT tool_name),
                COUNT(*),
                MAX(ts)
         FROM tool_invocations
         WHERE ts >= ?1 AND server_prefix IS NOT NULL AND server_prefix != ''
         GROUP BY server_prefix
         ORDER BY COUNT(*) DESC",
    ) {
        Ok(s) => s,
        Err(_) => return bill,
    };

    let rows = stmt.query_map(params![cutoff], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    });

    if let Ok(rows) = rows {
        for (prefix, invoked_tools, calls, last_used) in rows.flatten() {
            // Carried catalog: the measured server catalog, floored at the distinct tools ever
            // observed for this server so it never reads below what we have actually seen it ship.
            let observed_all_time: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT tool_name) FROM tool_invocations WHERE server_prefix = ?1",
                    params![prefix],
                    |r| r.get(0),
                )
                .unwrap_or(invoked_tools);
            let catalog = crate::profiles::catalog_tool_count(&prefix) as i64;
            let catalog_tools = catalog.max(observed_all_time).max(invoked_tools);
            let dead_tools = (catalog_tools - invoked_tools).max(0);
            let dead_ratio = if catalog_tools > 0 {
                dead_tools as f64 / catalog_tools as f64
            } else {
                0.0
            };
            let carried_tokens = catalog_tools * tpt;
            let dead_tokens = dead_tools * tpt;

            bill.total_catalog_tools += catalog_tools;
            bill.total_carried_tokens += carried_tokens;
            bill.total_invoked_tools += invoked_tools;
            bill.total_dead_tokens += dead_tokens;

            bill.servers.push(ToolMenuBillServer {
                server: crate::profiles::mcp_prefix_to_server_display(&prefix),
                prefix,
                catalog_tools,
                carried_tokens,
                invoked_tools,
                calls,
                dead_tools,
                dead_tokens,
                dead_ratio,
                last_used,
                pruned: false,
                misses: 0,
                prune_stage: String::new(),
                hidden_sessions: 0,
                hidden_needed: 0,
            });
        }
    }

    bill.servers
        .sort_by(|a, b| b.dead_tokens.cmp(&a.dead_tokens));
    if let Some(top) = bill.servers.first() {
        bill.biggest_dead_server = Some(top.server.clone());
        bill.biggest_dead_tokens = top.dead_tokens;
    }

    bill.since = conn
        .query_row(
            "SELECT MIN(ts) FROM tool_invocations WHERE ts >= ?1",
            params![cutoff],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();

    bill
}

pub fn compress_decision_stats(conn: &Connection) -> CompressDecisionStats {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let row = conn.query_row(
        "SELECT
            COUNT(*),
            COALESCE(SUM(outcome_joined), 0),
            COALESCE(SUM(CASE WHEN applied = 1 AND outcome_correction = 1 THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN applied = 0 THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN applied = 1 THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN substr(ts, 1, 10) = ?1 THEN 1 ELSE 0 END), 0)
         FROM compress_decisions",
        params![today],
        |r| {
            Ok(CompressDecisionStats {
                total: r.get(0)?,
                joined: r.get(1)?,
                corrections_caused: r.get(2)?,
                shadow: r.get(3)?,
                active: r.get(4)?,
                today: r.get(5)?,
            })
        },
    );
    row.unwrap_or_default()
}

/// One day of decision accrual for the loop-health view (CTX-26): how many decisions ctx made
/// that day and how many have since joined to an outcome. Read-only. Lets the dashboard show
/// whether signal is actually arriving over time, and how much of it gets labeled, instead of a
/// single lifetime total that hides a stall.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionsByDay {
    /// `YYYY-MM-DD` (UTC), taken from the decision timestamp.
    pub day: String,
    pub total: i64,
    pub joined: i64,
}

/// Decisions per day for the last `days` calendar days that have any decisions, oldest first.
/// Only days with at least one decision appear (no zero-filling): the view draws gaps honestly
/// rather than inventing empty buckets.
pub fn decisions_by_day(conn: &Connection, days: usize) -> Vec<DecisionsByDay> {
    let mut stmt = match conn.prepare(
        "SELECT substr(ts, 1, 10) AS day,
                COUNT(*),
                COALESCE(SUM(outcome_joined), 0)
         FROM compress_decisions
         GROUP BY day
         ORDER BY day DESC
         LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![days as i64], |r| {
        Ok(DecisionsByDay {
            day: r.get(0)?,
            total: r.get(1)?,
            joined: r.get(2)?,
        })
    });
    let mut out: Vec<DecisionsByDay> = match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    };
    out.reverse();
    out
}

/// Per-tool collection progress, used by the Learning home rows and Act 1 activation gate.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompressToolProgress {
    pub tool_name: String,
    pub decisions: i64,
    pub joined: i64,
    pub clean_runs: i64,
    pub corrections: i64,
    pub rereads: i64,
    pub active: bool,
}

pub fn compress_tool_progress(conn: &Connection) -> Vec<CompressToolProgress> {
    let mut stmt = match conn.prepare(
        &format!("SELECT tool_name,
                COUNT(*),
                COALESCE(SUM(outcome_joined), 0),
                COALESCE(SUM(CASE WHEN outcome_joined = 1 AND COALESCE(outcome_correction,0) = 0
                                   AND COALESCE(outcome_reread,0) = 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(COALESCE(outcome_correction,0)), 0),
                COALESCE(SUM(COALESCE(outcome_reread,0)), 0),
                COALESCE(MAX(applied), 0)
         FROM compress_decisions
         WHERE 1=1{EXCLUDE_EDIT_TOOLS}
         GROUP BY tool_name
         ORDER BY COUNT(*) DESC"),
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| {
        Ok(CompressToolProgress {
            tool_name: r.get(0)?,
            decisions: r.get(1)?,
            joined: r.get(2)?,
            clean_runs: r.get(3)?,
            corrections: r.get(4)?,
            rereads: r.get(5)?,
            active: r.get::<_, i64>(6)? == 1,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Recent decisions for the live observation feed on the Learning home.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompressDecisionFeedRow {
    pub ts: String,
    pub tool_name: String,
    pub kind: String,
    pub task_mode: String,
    pub lines_total: i64,
    pub lines_keep: i64,
    pub lines_drop: i64,
    pub chars_in: i64,
    pub would_chars_out: i64,
    pub command_or_path: Option<String>,
    pub applied: bool,
    /// True when the Read edit-intent guard deliberately kept this read in full (CTX-8/CTX-11),
    /// so the feed can show a "protected" state instead of conflating it with "watching".
    pub protected: bool,
}

pub fn compress_decision_feed(conn: &Connection, limit: usize) -> Vec<CompressDecisionFeedRow> {
    let mut stmt = match conn.prepare(
        "SELECT ts, tool_name, kind, task_mode, lines_total, lines_keep, lines_drop,
                chars_in, would_chars_out, command_or_path, applied, features_json
         FROM compress_decisions ORDER BY id DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![limit as i64], |r| {
        let features_json: String = r.get::<_, Option<String>>(11)?.unwrap_or_default();
        let protected = serde_json::from_str::<serde_json::Value>(&features_json)
            .ok()
            .and_then(|v| v.get("read_protected").and_then(|p| p.as_bool()))
            .unwrap_or(false);
        Ok(CompressDecisionFeedRow {
            ts: r.get(0)?,
            tool_name: r.get(1)?,
            kind: r.get(2)?,
            task_mode: r.get(3)?,
            lines_total: r.get(4)?,
            lines_keep: r.get(5)?,
            lines_drop: r.get(6)?,
            chars_in: r.get(7)?,
            would_chars_out: r.get(8)?,
            command_or_path: r.get(9)?,
            applied: r.get::<_, i64>(10)? == 1,
            protected,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Applied compression savings per tool, drawn only from decisions ctx actually trimmed.
/// `chars_saved` is characters removed from what the model saw (chars_in - would_chars_out,
/// floored at 0). Shadow-only decisions (applied = 0) contribute nothing, so this never counts
/// trims that were not really made. Bucketing into "earned" vs "still testing" happens upstream
/// by joining tool_name to the causal verdict.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CompressToolSavings {
    pub tool_name: String,
    pub applied_count: i64,
    pub chars_saved: i64,
}

pub fn compress_savings_by_tool(conn: &Connection) -> Vec<CompressToolSavings> {
    let mut stmt = match conn.prepare(
        "SELECT tool_name,
                COALESCE(SUM(CASE WHEN applied = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN applied = 1 AND chars_in > would_chars_out
                                  THEN chars_in - would_chars_out ELSE 0 END), 0)
         FROM compress_decisions
         GROUP BY tool_name",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| {
        Ok(CompressToolSavings {
            tool_name: r.get(0)?,
            applied_count: r.get(1)?,
            chars_saved: r.get(2)?,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// How far the per-decision model's shadow logging has progressed (ADR 0007 / CTX-16). Counts
/// decisions the served model actually scored (`model_score` present in features_json), how many of
/// those have been judged by an outcome, and how many distinct repos they span. These are the
/// "data accruing" half of Phase 2 readiness; the "does the model beat the rules" half comes from
/// the benchmark arms.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ModelShadowProgress {
    pub scored_total: i64,
    pub scored_joined: i64,
    pub distinct_repos: i64,
}

pub fn model_shadow_progress(conn: &Connection) -> ModelShadowProgress {
    let mut out = ModelShadowProgress::default();
    let mut repos = std::collections::HashSet::new();
    let mut stmt =
        match conn.prepare("SELECT features_json, outcome_joined FROM compress_decisions") {
            Ok(s) => s,
            Err(_) => return out,
        };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            r.get::<_, i64>(1)?,
        ))
    });
    let rows = match rows {
        Ok(r) => r,
        Err(_) => return out,
    };
    for (features_json, joined) in rows.flatten() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&features_json) {
            if v.get("model_score").is_some() {
                out.scored_total += 1;
                if joined == 1 {
                    out.scored_joined += 1;
                }
            }
            if let Some(repo) = v.get("repo_key").and_then(|x| x.as_str()) {
                repos.insert(repo.to_string());
            }
        }
    }
    out.distinct_repos = repos.len() as i64;
    out
}

/// Back-fill outcome labels onto shadow decision rows once downstream turns land.
/// A row is only marked joined when there is later evidence in the same session, so a
/// decision is never scored "clean" merely because nothing has happened yet.
///
/// - `outcome_correction`: explicit user complaint after the decision AND ctx trimmed lines
///   (applied=1, lines_drop>0). Uniform for every tool (CTX-48 / ADR 0033).
/// - `outcome_reread`: the same `command_or_path` was hit again later in the same session.
///
/// Returns the number of decision rows newly joined.
/// How long after a tool decision a user correction or re-read still counts as caused by
/// that decision. Beyond this the user has moved on, so a later short turn is unrelated.
/// The label must mean "happened soon after", and it must be reproducible no matter when
/// ingest runs, which is why the join only scores a decision once its window has closed.
/// Grounded in observed pacing: turns land every minute or two, while unrelated
/// corrections in long sessions sit an hour or more away.
pub const CORRECTION_WINDOW_MINUTES: f64 = 15.0;

pub fn join_compress_outcomes(conn: &Connection) -> Result<usize> {
    // `?1` is the window in days (minutes / 1440). julianday() normalizes the mixed
    // timestamp shapes (offset vs `Z`, varying fractional digits) that a string compare
    // would get wrong.
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    // Edit-follow reuses the re-read attribution exactly, but the later same-path touch must be an
    // edit/write tool. Shared with the one-time backfill so both compute the label identically
    // (CTX-46 / ADR 0031).
    let edit_follow = edit_follow_value_sql();
    let reread = reread_value_sql();
    let n = conn.execute(
        &format!(
            r#"
        UPDATE compress_decisions
        SET outcome_correction = (
                -- CTX-48 / ADR 0033: gate correction only when the user explicitly complained
                -- AND ctx actually trimmed this decision. Uniform for every tool.
                SELECT CASE WHEN compress_decisions.applied = 1
                                 AND compress_decisions.lines_drop > 0
                                 AND EXISTS (
                    SELECT 1 FROM turns t
                    JOIN sessions s ON s.id = t.session_id
                    WHERE s.external_key LIKE '%' || compress_decisions.session_id || '%'
                      AND t.flags LIKE '%correction_explicit%'
                      AND t.flags NOT LIKE '%long_dump%'
                      AND t.flags NOT LIKE '%session_steer%'
                      AND t.ts IS NOT NULL
                      AND julianday(t.ts) > julianday(compress_decisions.ts)
                      AND julianday(t.ts) <= julianday(compress_decisions.ts) + ?1
                      AND NOT EXISTS (
                          SELECT 1 FROM compress_decisions d2
                          WHERE d2.session_id = compress_decisions.session_id
                            AND julianday(d2.ts) > julianday(compress_decisions.ts)
                            AND julianday(d2.ts) < julianday(t.ts)
                      )
                ) THEN 1 ELSE 0 END
            ),
            outcome_reread = {reread},
            outcome_recovered = (
                SELECT CASE WHEN compress_decisions.rewind_id IS NOT NULL AND EXISTS (
                    SELECT 1 FROM rewind_store r
                    WHERE r.id = compress_decisions.rewind_id AND r.expanded_at IS NOT NULL
                ) THEN 1 ELSE 0 END
            ),
            -- Same nearest-preceding rule as the re-read, but only a later *edit* of the same path
            -- counts: the agent acted on the file, the strongest "needed it whole" signal. A plain
            -- re-read does not set this; it sets outcome_reread (CTX-46 / ADR 0031).
            outcome_edit_follow = {edit_follow},
            outcome_joined = 1,
            surface = 'claude-code'
        WHERE outcome_joined = 0
          AND session_id IS NOT NULL
          AND (
              -- Any observational user signal or a closed window is enough to join.
              EXISTS (
                  SELECT 1 FROM turns t
                  JOIN sessions s ON s.id = t.session_id
                  WHERE s.external_key LIKE '%' || compress_decisions.session_id || '%'
                    AND (
                      t.flags LIKE '%correction%'
                      OR t.flags LIKE '%aborted%'
                      OR t.flags LIKE '%session_steer%'
                    )
                    AND t.ts IS NOT NULL
                    AND julianday(t.ts) > julianday(compress_decisions.ts)
                    AND julianday(t.ts) <= julianday(compress_decisions.ts) + ?1
              )
              -- Or the window has closed (a turn landed beyond it), confirming a clean run.
              OR EXISTS (
                  SELECT 1 FROM turns t
                  JOIN sessions s ON s.id = t.session_id
                  WHERE s.external_key LIKE '%' || compress_decisions.session_id || '%'
                    AND t.ts IS NOT NULL
                    AND julianday(t.ts) > julianday(compress_decisions.ts) + ?1
              )
          )
        "#,
            edit_follow = edit_follow,
            reread = reread
        ),
        params![window_days],
    )?;
    let _ = refresh_outcome_signals(conn);
    Ok(n)
}

/// Populate `outcome_signals` on joined Claude-code rows from turn flags in the outcome window.
/// Purely observational; the gate reads only `outcome_correction`.
pub fn refresh_outcome_signals(conn: &Connection) -> Result<usize> {
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    let mut sel = conn.prepare(
        "SELECT id, session_id, ts, applied, lines_drop, COALESCE(outcome_correction,0),
                COALESCE(outcome_reread,0), COALESCE(outcome_edit_follow,0)
         FROM compress_decisions
         WHERE outcome_joined = 1 AND session_id IS NOT NULL",
    )?;
    let rows: Vec<(i64, String, String, i64, i64, i64, i64, i64)> = sel
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })?
        .filter_map(|x| x.ok())
        .collect();

    let mut updated = 0usize;
    for (id, sid, ts, applied, lines_drop, corr, rr, ef) in rows {
        let sid_like = format!("%{sid}%");
        let mut flags_stmt = match conn.prepare(
            "SELECT t.flags FROM turns t
             JOIN sessions s ON s.id = t.session_id
             WHERE s.external_key LIKE ?1
               AND t.ts IS NOT NULL
               AND julianday(t.ts) > julianday(?2)
               AND julianday(t.ts) <= julianday(?2) + ?3",
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let flag_rows = flags_stmt
            .query_map(params![sid_like, ts, window_days], |r| r.get::<_, String>(0))
            .ok();
        let mut signals: Vec<&str> = Vec::new();
        if let Some(it) = flag_rows {
            for f in it.filter_map(|x| x.ok()) {
                if f.contains("correction_explicit") {
                    signals.push("correction_explicit");
                }
                if f.contains("correction_terse") {
                    signals.push("correction_terse");
                }
                if f.contains("aborted") {
                    signals.push("aborted");
                }
                if f.contains("session_steer") {
                    signals.push("session_steer");
                }
            }
        }
        signals.sort_unstable();
        signals.dedup();
        if corr > 0 {
            signals.push("correction_gate");
        }
        if rr > 0 {
            signals.push("reread");
        }
        if ef > 0 {
            signals.push("edit_follow");
        }
        if applied > 0 && lines_drop > 0 {
            signals.push("trimmed");
        }
        if compression_workaround_from_db(conn, &sid, &ts, applied, lines_drop)? {
            signals.push("compression_workaround");
        }
        signals.sort_unstable();
        signals.dedup();
        let json = serde_json::to_string(&signals).unwrap_or_else(|_| "[]".into());
        if conn
            .execute(
                "UPDATE compress_decisions SET outcome_signals = ?2 WHERE id = ?1",
                params![id, json],
            )
            .is_ok()
        {
            updated += 1;
        }
    }
    Ok(updated)
}

/// Structural compression workaround from the timestamp join path: trimmed decision followed
/// by a shell bypass in the same session within the outcome window (CTX-50 / ADR 0035).
fn compression_workaround_from_db(
    conn: &Connection,
    session_id: &str,
    decision_ts: &str,
    applied: i64,
    lines_drop: i64,
) -> Result<bool> {
    if applied == 0 || lines_drop <= 0 {
        return Ok(false);
    }
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    let shell_list: String = crate::outcome_signals::BYPASS_SHELL_TOOL_NAMES
        .iter()
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1 FROM compress_decisions d2
            WHERE d2.session_id = ?1
              AND julianday(d2.ts) > julianday(?2)
              AND julianday(d2.ts) <= julianday(?2) + ?3
              AND LOWER(TRIM(d2.tool_name)) IN ({shell_list})
              AND (
                LOWER(COALESCE(d2.command_or_path,'')) LIKE '%json%'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '%<<%'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '%heredoc%'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '%python3 -c%'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '%node -e%'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '% > %'
                OR LOWER(COALESCE(d2.command_or_path,'')) LIKE '%>>%'
              )
         )"
    );
    conn.query_row(&sql, params![session_id, decision_ts, window_days], |r| r.get(0))
        .map_err(Into::into)
}

/// The `outcome_reread` value subquery: nearest-preceding same fingerprint within the window.
/// State-mutation tools with legacy bare fingerprints skip routine follow-up calls (CTX-49).
fn reread_value_sql() -> String {
    let legacy_exclude = crate::outcome_signals::reread_legacy_state_mutation_exclusion_sql();
    format!(
        r#"(
                SELECT CASE WHEN EXISTS (
                    SELECT 1 FROM compress_decisions d2
                    WHERE d2.session_id = compress_decisions.session_id
                      AND d2.command_or_path = compress_decisions.command_or_path
                      AND d2.command_or_path IS NOT NULL
                      AND d2.id <> compress_decisions.id
                      AND julianday(d2.ts) > julianday(compress_decisions.ts)
                      AND julianday(d2.ts) <= julianday(compress_decisions.ts) + ?1
                      {legacy_exclude}
                      AND NOT EXISTS (
                          SELECT 1 FROM compress_decisions d3
                          WHERE d3.session_id = compress_decisions.session_id
                            AND d3.command_or_path = compress_decisions.command_or_path
                            AND d3.id <> compress_decisions.id
                            AND julianday(d3.ts) > julianday(compress_decisions.ts)
                            AND julianday(d3.ts) < julianday(d2.ts)
                      )
                ) THEN 1 ELSE 0 END
            )"#,
        legacy_exclude = legacy_exclude
    )
}

/// The `outcome_edit_follow` value subquery (CTX-46 / ADR 0031): 1 when the same file was edited
/// (an edit/write tool) within the window after a decision, using the same nearest-preceding
/// attribution as the re-read so one edit is owned by the last same-path read before it. Shared by
/// the live join and the one-time backfill so they can never compute the label differently. `?1`
/// is the window in days; the edit-tool predicate comes from the one shared edit-tool set.
fn edit_follow_value_sql() -> String {
    let edit_is_d2 = crate::outcome_signals::edit_tool_sql_in_list("d2.tool_name");
    // Same-region requirement (CTX-62 content-overlap fix). When the decision is itself an edit, a
    // later edit of the same file only counts as a re-edit if it *sought the exact text this edit
    // wrote* (`d2.edit_sought` == `decision.edit_wrote`): the agent went back and changed the very
    // lines it just wrote. Editing a different part of the same big file is normal multi-part work,
    // not a botched-edit redo, and no longer fires. An edit with no anchor (an older row, or a
    // trivial edit) never matches, so the old ~70% file-level noise drops out. When the decision is
    // a read, this whole clause is skipped and the original same-file rule stands, so the read
    // "needed whole" signal is unchanged.
    let region = edit_content_match_sql();
    format!(
        r#"(
                SELECT CASE WHEN EXISTS (
                    SELECT 1 FROM compress_decisions d2
                    WHERE d2.session_id = compress_decisions.session_id
                      AND d2.command_or_path = compress_decisions.command_or_path
                      AND d2.command_or_path IS NOT NULL
                      AND d2.id <> compress_decisions.id
                      AND {edit_is_d2}
                      AND {region}
                      AND julianday(d2.ts) > julianday(compress_decisions.ts)
                      AND julianday(d2.ts) <= julianday(compress_decisions.ts) + ?1
                      AND NOT EXISTS (
                          SELECT 1 FROM compress_decisions d3
                          WHERE d3.session_id = compress_decisions.session_id
                            AND d3.command_or_path = compress_decisions.command_or_path
                            AND d3.id <> compress_decisions.id
                            AND julianday(d3.ts) > julianday(compress_decisions.ts)
                            AND julianday(d3.ts) < julianday(d2.ts)
                      )
                ) THEN 1 ELSE 0 END
            )"#,
        edit_is_d2 = edit_is_d2,
        region = region,
    )
}

/// SQL predicate for the same-region requirement. If the decision is not an edit tool (a read),
/// pass through and keep the same-file rule. If it is an edit, require the follow-up edit to have
/// sought the exact text this edit wrote, so only a genuine same-region redo counts.
fn edit_content_match_sql() -> String {
    let decision_is_edit =
        crate::outcome_signals::edit_tool_sql_in_list("compress_decisions.tool_name");
    let wrote = "json_extract(compress_decisions.features_json,'$.edit_wrote')";
    let sought = "json_extract(d2.features_json,'$.edit_sought')";
    format!("(NOT ({decision_is_edit}) OR ({wrote} IS NOT NULL AND {sought} = {wrote}))")
}

/// A shadow decision still awaiting an outcome, with the fields a transcript-based join
/// needs to place it on a surface timeline (Phase 4, surfaces without timestamps).
#[derive(Debug, Clone)]
pub struct UnjoinedDecision {
    pub id: i64,
    pub command_or_path: String,
    pub applied: bool,
    pub lines_drop: i64,
}

/// Unjoined decisions for one surface session, keyed by the surface's own session id
/// (the UUID the hook recorded). Used by the ordinal/fingerprint outcome join for agents
/// whose transcripts carry no timestamps (Cursor).
pub fn unjoined_decisions_for_session(
    conn: &Connection,
    session_id: &str,
) -> Vec<UnjoinedDecision> {
    let mut stmt = match conn.prepare(
        "SELECT id, command_or_path, applied, lines_drop FROM compress_decisions
         WHERE session_id = ?1 AND outcome_joined = 0 AND command_or_path IS NOT NULL",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(UnjoinedDecision {
            id: r.get(0)?,
            command_or_path: r.get(1)?,
            applied: r.get::<_, i64>(2)? != 0,
            lines_drop: r.get(3)?,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Back-fill a single decision's outcome label and mark it joined. Used by the
/// transcript (ordinal) join; the timestamp join uses a bulk UPDATE instead.
///
/// `signals_json` is the observation-only richer-signal set for this decision (ADR 0019), a JSON
/// array of signal names, or `None` to leave it unset. `edit_follow` is the same-file edit-follow
/// label (CTX-46 / ADR 0031): the same path was edited within the window. None of these feed the
/// causal gate, which reads only `outcome_correction`; recording is decoupled from voting.
pub fn set_decision_outcome(
    conn: &Connection,
    id: i64,
    correction: bool,
    reread: bool,
    edit_follow: bool,
    surface: &str,
    signals_json: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE compress_decisions
         SET outcome_correction = ?2, outcome_reread = ?3, outcome_edit_follow = ?4,
             outcome_joined = 1, surface = ?5, outcome_signals = ?6
         WHERE id = ?1",
        params![
            id,
            correction as i64,
            reread as i64,
            edit_follow as i64,
            surface,
            signals_json
        ],
    )?;
    Ok(())
}

/// One joined decision that carries an observation-only richer-signal set (ADR 0019 / CTX-32),
/// for the precision spot-check. `signals` are the recorded signal names; `correction` is the
/// only one the gate currently reads, shown so co-occurrence is visible during hand-labeling.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SignalAuditRow {
    pub ts: String,
    pub tool_name: String,
    pub command_or_path: Option<String>,
    pub surface: Option<String>,
    pub kind: String,
    pub signals: Vec<String>,
    pub correction: bool,
    pub reread: bool,
}

/// Joined decisions whose observation-only signal set is non-empty, newest first. When
/// `signal` is given, only rows where that signal fired are returned (substring match on the
/// stored JSON array). `cap` bounds how many rows are read. Read-only; never touches the gate.
pub fn signal_audit_rows(conn: &Connection, signal: Option<&str>, cap: usize) -> Vec<SignalAuditRow> {
    let like = signal.map(|s| format!("%\"{s}\"%"));
    let sql = format!(
        "SELECT ts, tool_name, command_or_path, surface, kind, outcome_signals,
                      outcome_correction, outcome_reread
               FROM compress_decisions
               WHERE outcome_joined = 1 AND outcome_signals IS NOT NULL
                     AND outcome_signals != '[]'
                     AND (?1 IS NULL OR outcome_signals LIKE ?1){EXCLUDE_SELF_DEV}
               ORDER BY ts DESC
               LIMIT ?2"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![like, cap as i64], |r| {
        let signals_json: String = r.get(5)?;
        let signals: Vec<String> = serde_json::from_str(&signals_json).unwrap_or_default();
        Ok(SignalAuditRow {
            ts: r.get(0)?,
            tool_name: r.get(1)?,
            command_or_path: r.get(2)?,
            surface: r.get(3)?,
            kind: r.get(4)?,
            signals,
            correction: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
            reread: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// One agent surface's activity, drawn from `compress_decisions` provenance (ADR 0018 / CTX-34).
/// `seen` is false when ctx has recorded nothing for the surface, so the cross-surface view can
/// say "not yet" instead of presenting zeros as a measured result. All counts are real rows;
/// none are fabricated for an unseen surface.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SurfaceSummary {
    pub surface: String,
    pub seen: bool,
    /// Tool results ctx recorded a decision for on this surface.
    pub decisions: i64,
    /// Decisions where ctx actually shortened what the agent read back (applied = 1).
    pub acted: i64,
    /// Decisions ctx watched without changing anything (applied = 0).
    pub observed: i64,
    /// Decisions with a known outcome (a correction/re-read window that has closed).
    pub joined: i64,
    pub corrections: i64,
    pub rereads: i64,
    /// Characters removed from what the agent read back, summed over acted decisions.
    pub chars_saved: i64,
    pub today: i64,
    /// Most recent decision timestamp (RFC3339), or `None` when the surface is unseen.
    pub last_seen: Option<String>,
    /// Transcripts ctx has parsed for this surface, independent of any hook decision (CTX-53).
    /// Lets an agent ctx only watches (Cursor today) render real sessions instead of a fake zero.
    pub sessions_seen: i64,
    /// Tool calls observed across those transcripts. Calls, not results: some surfaces omit output.
    pub tool_calls_seen: i64,
    /// Corrections observed in transcripts (lexical guard), for a surface with no hook outcomes yet.
    pub transcript_corrections: i64,
    /// How ctx knows this surface: "hook" (live decisions), "transcript" (parsed sessions only),
    /// "hook+transcript" (both), or "" when unseen. Drives honest per-agent copy in the UI.
    pub observed_via: String,
}

/// Per-surface activity for the cross-surface view (CTX-34). Always returns Claude Code and
/// Cursor, in that order, so the UI renders both with an honest empty state for whichever ctx
/// has not seen yet. A NULL `surface` is a Claude/legacy row (provenance predates the column),
/// so it folds into claude-code. Read-only.
pub fn surface_summary(conn: &Connection) -> Vec<SurfaceSummary> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut by_surface: std::collections::HashMap<String, SurfaceSummary> =
        std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(surface,'claude-code') AS s,
                COUNT(*),
                SUM(CASE WHEN applied=1 THEN 1 ELSE 0 END),
                SUM(COALESCE(outcome_joined,0)),
                SUM(COALESCE(outcome_correction,0)),
                SUM(COALESCE(outcome_reread,0)),
                SUM(CASE WHEN applied=1 THEN (chars_in - would_chars_out) ELSE 0 END),
                SUM(CASE WHEN substr(ts,1,10)=?1 THEN 1 ELSE 0 END),
                MAX(ts)
         FROM compress_decisions
         GROUP BY s",
    ) {
        let rows = stmt.query_map(params![today], |r| {
            let decisions: i64 = r.get(1)?;
            let acted: i64 = r.get(2)?;
            Ok(SurfaceSummary {
                surface: r.get(0)?,
                seen: decisions > 0,
                decisions,
                acted,
                observed: decisions - acted,
                joined: r.get(3)?,
                corrections: r.get(4)?,
                rereads: r.get(5)?,
                chars_saved: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                today: r.get(7)?,
                last_seen: r.get(8)?,
                sessions_seen: 0,
                tool_calls_seen: 0,
                transcript_corrections: 0,
                observed_via: if decisions > 0 { "hook".into() } else { String::new() },
            })
        });
        if let Ok(it) = rows {
            for s in it.flatten() {
                by_surface.insert(s.surface.clone(), s);
            }
        }
    }
    ["claude-code", "cursor"]
        .iter()
        .map(|id| {
            by_surface.remove(*id).unwrap_or_else(|| SurfaceSummary {
                surface: id.to_string(),
                seen: false,
                ..Default::default()
            })
        })
        .collect()
}

/// Per-surface activity enriched with the transcript corpus under `home` (CTX-53). The hook-based
/// `surface_summary` only knows an agent ctx has trimmed; this folds in agents ctx has merely
/// watched on disk, so an agent with real transcripts but zero hook decisions (Cursor, once the
/// user has moved off it) still renders as a seen agent with genuine sessions and tool calls
/// instead of a permanent "not seen yet". Read-only; the transcript files and DB are never
/// mutated here.
pub fn surface_summary_full(conn: &Connection, home: &std::path::Path) -> Vec<SurfaceSummary> {
    let mut base = surface_summary(conn);
    let corpus = crate::surface::ingest::transcript_corpus_summary(home);
    for stats in corpus {
        let Some(entry) = base.iter_mut().find(|s| s.surface == stats.surface) else {
            continue;
        };
        entry.sessions_seen = stats.sessions;
        entry.tool_calls_seen = stats.tool_calls;
        entry.transcript_corrections = stats.corrections;
        if stats.sessions > 0 {
            entry.seen = true;
            entry.observed_via = if entry.observed_via == "hook" {
                "hook+transcript".into()
            } else {
                "transcript".into()
            };
            // Transcripts have no timestamps; use the newest file mtime only when the hook path
            // gave us no decision timestamp for this surface, so we never overwrite a real one.
            if entry.last_seen.is_none() {
                entry.last_seen = stats.last_activity;
            }
        }
    }
    base
}

/// A labeled decision row for training / benchmarking (Act 1 / Act 2).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LabeledDecision {
    pub tool_name: String,
    pub kind: String,
    pub lines_total: i64,
    pub lines_drop: i64,
    pub chars_in: i64,
    pub would_chars_out: i64,
    pub features_json: String,
    pub correction: i64,
    pub reread: i64,
    /// Same-file edit-follow label (CTX-46 / ADR 0031): the file was edited within the window.
    /// The observational "needed this read whole" target the file-aware model trains on in a later
    /// increment. Distinct from `correction` (causal harm) and `reread` (any later same-path touch).
    pub edit_follow: i64,
}

/// All decisions that have been joined to an outcome and are trustworthy enough to train
/// on. Transcript-derived surfaces (Cursor) are excluded until their correction precision
/// is proven: their labels are still kept for the fail-safe activation gate, just not fed
/// to the learned model or the benchmark. A NULL surface is a Claude/legacy row (Cursor
/// could never join before provenance existed), so it stays in.
pub fn load_joined_decisions(conn: &Connection) -> Vec<LabeledDecision> {
    let mut stmt = match conn.prepare(
        &format!("SELECT tool_name, kind, lines_total, lines_drop, chars_in, would_chars_out,
                features_json, COALESCE(outcome_correction,0), COALESCE(outcome_reread,0),
                COALESCE(outcome_edit_follow,0)
         FROM compress_decisions
         WHERE outcome_joined = 1 AND COALESCE(surface,'') != 'cursor'{EXCLUDE_SELF_DEV}{EXCLUDE_EDIT_TOOLS}"),
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| {
        Ok(LabeledDecision {
            tool_name: r.get(0)?,
            kind: r.get(1)?,
            lines_total: r.get(2)?,
            lines_drop: r.get(3)?,
            chars_in: r.get(4)?,
            would_chars_out: r.get(5)?,
            features_json: r.get(6)?,
            correction: r.get(7)?,
            reread: r.get(8)?,
            edit_follow: r.get(9)?,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// One positive-labeled decision with the raw evidence that produced its label, for the
/// `ctx context labels` audit. Read-only: this mirrors the join logic in
/// `join_compress_outcomes` so we can eyeball, by hand, whether a label is a real
/// context-harm signal or noise (a short turn during normal work, an unrelated re-read).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LabelAuditRow {
    pub id: i64,
    pub ts: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub kind: String,
    pub command_or_path: Option<String>,
    pub correction: bool,
    pub reread: bool,
    pub surface: Option<String>,
    pub correction_evidence: Vec<CorrectionEvidence>,
    pub reread_evidence: Vec<RereadEvidence>,
}

/// A user turn flagged as a correction that landed inside the window after a decision.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CorrectionEvidence {
    pub ts: String,
    pub minutes_after: f64,
    pub text: String,
}

/// A later decision on the same path/command that landed inside the window (the re-read).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RereadEvidence {
    pub ts: String,
    pub minutes_after: f64,
    pub tool_name: String,
}

/// Pull the most recent positive-labeled decisions (correction or re-read) for a tool, each
/// with the evidence that produced the label. `tool_filter` is an exact `tool_name` match
/// (None = all tools). Used to judge label precision by hand before trusting the corpus.
pub fn audit_labeled_decisions(
    conn: &Connection,
    tool_filter: Option<&str>,
    limit: usize,
) -> Vec<LabelAuditRow> {
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    let base_sql = format!(
        "SELECT id, ts, session_id, tool_name, kind, command_or_path,
                COALESCE(outcome_correction,0), COALESCE(outcome_reread,0), surface
         FROM compress_decisions
         WHERE outcome_joined = 1
           AND (COALESCE(outcome_correction,0) = 1 OR COALESCE(outcome_reread,0) = 1){EXCLUDE_SELF_DEV}"
    );
    let sql = match tool_filter {
        Some(_) => format!("{base_sql} AND tool_name = ?2 ORDER BY id DESC LIMIT ?1"),
        None => format!("{base_sql} ORDER BY id DESC LIMIT ?1"),
    };
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let map = |r: &rusqlite::Row<'_>| {
        Ok(LabelAuditRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            session_id: r.get(2)?,
            tool_name: r.get(3)?,
            kind: r.get(4)?,
            command_or_path: r.get(5)?,
            correction: r.get::<_, i64>(6)? == 1,
            reread: r.get::<_, i64>(7)? == 1,
            surface: r.get(8)?,
            correction_evidence: Vec::new(),
            reread_evidence: Vec::new(),
        })
    };
    let rows_res = match tool_filter {
        Some(t) => stmt.query_map(params![limit as i64, t], map),
        None => stmt.query_map(params![limit as i64], map),
    };
    let mut rows: Vec<LabelAuditRow> = match rows_res {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    for row in &mut rows {
        let Some(sid) = row.session_id.clone() else {
            continue;
        };
        let sid_like = format!("%{sid}%");
        if row.correction {
            if let Ok(mut s) = conn.prepare(
                "SELECT t.ts,
                        (julianday(t.ts) - julianday(?2)) * 1440.0,
                        COALESCE(t.human_text_prefix, '')
                 FROM turns t
                 JOIN sessions s ON s.id = t.session_id
                 WHERE s.external_key LIKE ?1
                   AND t.flags LIKE '%correction%'
                   AND t.ts IS NOT NULL
                   AND julianday(t.ts) > julianday(?2)
                   AND julianday(t.ts) <= julianday(?2) + ?3
                 ORDER BY t.ts",
            ) {
                if let Ok(it) = s.query_map(params![sid_like, row.ts, window_days], |r| {
                    Ok(CorrectionEvidence {
                        ts: r.get(0)?,
                        minutes_after: r.get(1)?,
                        text: r.get(2)?,
                    })
                }) {
                    row.correction_evidence = it.filter_map(|x| x.ok()).collect();
                }
            }
        }
        if row.reread {
            if let Some(cmd) = row.command_or_path.clone() {
                if let Ok(mut s) = conn.prepare(
                    "SELECT d2.ts,
                            (julianday(d2.ts) - julianday(?2)) * 1440.0,
                            d2.tool_name
                     FROM compress_decisions d2
                     WHERE d2.session_id = ?1
                       AND d2.command_or_path = ?4
                       AND d2.command_or_path IS NOT NULL
                       AND d2.id <> ?5
                       AND julianday(d2.ts) > julianday(?2)
                       AND julianday(d2.ts) <= julianday(?2) + ?3
                     ORDER BY d2.ts",
                ) {
                    if let Ok(it) =
                        s.query_map(params![sid, row.ts, window_days, cmd, row.id], |r| {
                            Ok(RereadEvidence {
                                ts: r.get(0)?,
                                minutes_after: r.get(1)?,
                                tool_name: r.get(2)?,
                            })
                        })
                    {
                        row.reread_evidence = it.filter_map(|x| x.ok()).collect();
                    }
                }
            }
        }
    }
    rows
}

/// SQL fragment that drops ctx's own development activity from a corpus query (CTX-32). Decisions
/// made inside ctx's source repo are tagged `"self_dev":true` in `features_json` (see
/// `agent::is_self_dev_repo`); building and editing ctx is the developer's churn, not user behavior,
/// so it must not bias the gate, the learned model, or the precision audit. The token is the exact
/// compact serialization of `ShadowFeatures::self_dev = Some(true)`; a unit test guards against drift.
pub(crate) const EXCLUDE_SELF_DEV: &str =
    " AND COALESCE(features_json,'') NOT LIKE '%\"self_dev\":true%'";

/// SQL fragment that drops edit/write tool rows from trim-facing corpus queries (CTX-46 / ADR
/// 0031). Edits are recorded as timeline events so the edit-follow label can find them, but ctx
/// never trims an edit, so they must not train the trim model or appear as trimmable tools on the
/// ladder/gate. The list mirrors `outcome_signals::EDIT_TOOL_NAMES`; a test pins the two together.
pub(crate) const EXCLUDE_EDIT_TOOLS: &str = " AND LOWER(TRIM(tool_name)) NOT IN ('write','edit','multiedit','str_replace','str_replace_editor','create_file','applypatch','apply_patch','searchreplace','search_replace')";

/// Causal before/after counts for one tool (SAU-150). The control and treatment share the
/// same selection (the heuristic wanted to drop lines, `lines_drop > 0`) and differ only on
/// whether the trim was actually applied. Comparing these isolates the effect of trimming,
/// which an absolute rate on shadow decisions cannot do. `trimmed_*` stays zero until the
/// tool is deliberately activated and real trimmed usage accrues. Excludes ctx self-dev rows.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CausalToolOutcome {
    pub tool_name: String,
    pub baseline_n: i64,
    pub baseline_corrections: i64,
    /// The tool's re-touch harm count in the baseline arm. Re-reads for reference tools, re-edits
    /// (outcome_edit_follow) for edit tools, so the gate judges each tool by the signal that means
    /// harm for it (CTX-62). The field name is historical; `is_edit_tool` says which it holds.
    pub baseline_rereads: i64,
    pub trimmed_n: i64,
    pub trimmed_corrections: i64,
    pub trimmed_rereads: i64,
    /// Applied trims collected so far, whether or not their outcome window has closed. `trimmed_n`
    /// only counts scored (joined) trims, so during a fresh trial it sits at 0 while trims are
    /// already happening; this is the count that matches what the user just watched (CTX-62).
    pub trimmed_collected: i64,
    /// True when the re-touch counts above are re-edits, so the UI can label them honestly.
    pub is_edit_tool: bool,
}

/// Per-tool causal before/after outcome counts over joined decisions. `tool_filter` is an
/// exact `tool_name` match (None = all tools, ordered by total decided volume). Edit tools are
/// included and judged by re-edit rather than re-read (CTX-62).
pub fn causal_tool_outcomes(
    conn: &Connection,
    tool_filter: Option<&str>,
) -> Vec<CausalToolOutcome> {
    // The re-touch signal that means harm for this tool: a re-edit of the same file for edit tools,
    // a re-read for everything else. One expression so the gate stays uniform across tool families.
    let retouch = format!(
        "(CASE WHEN {edit} THEN COALESCE(outcome_edit_follow,0) ELSE COALESCE(outcome_reread,0) END)",
        edit = crate::outcome_signals::edit_tool_sql_in_list("tool_name")
    );
    // Region-aware scoping (CTX-62): for an edit tool, a row can only be a scored re-edit observation
    // if it carries the content anchor (`edit_wrote`). Rows recorded before the anchor cannot be
    // measured for a same-region redo, so they are not counted, and the edit gate stays fail-closed
    // until enough anchored edits accrue instead of earning on the old file-level noise. Non-edit
    // tools are unaffected.
    let region_ok = format!(
        "(NOT ({edit}) OR features_json LIKE '%edit_wrote%')",
        edit = crate::outcome_signals::edit_tool_sql_in_list("tool_name")
    );
    // Joined-only counts feed the causal verdict; `trimmed_collected` counts every applied trim so a
    // fresh trial does not read as "0 trimmed" while its outcomes are still landing. The joined
    // filter moved from the WHERE into each CASE so both live side by side.
    let base = format!(
        "SELECT tool_name,
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=0 AND lines_drop>0 AND {region_ok} THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=0 AND lines_drop>0 AND {region_ok} AND COALESCE(outcome_correction,0)=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=0 AND lines_drop>0 AND {region_ok} AND {retouch}=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=1 AND lines_drop>0 AND {region_ok} THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=1 AND lines_drop>0 AND {region_ok} AND COALESCE(outcome_correction,0)=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=1 AND lines_drop>0 AND {region_ok} AND {retouch}=1 AND COALESCE(outcome_recovered,0)=0 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN applied=1 AND lines_drop>0 AND {region_ok} THEN 1 ELSE 0 END),0)
         FROM compress_decisions
         WHERE 1=1{EXCLUDE_SELF_DEV}"
    );
    let sql = match tool_filter {
        Some(_) => format!("{base} AND tool_name = ?1 GROUP BY tool_name"),
        None => format!(
            "{base} GROUP BY tool_name ORDER BY (
                SUM(CASE WHEN lines_drop>0 THEN 1 ELSE 0 END)
             ) DESC"
        ),
    };
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let map = |r: &rusqlite::Row<'_>| {
        let tool_name: String = r.get(0)?;
        Ok(CausalToolOutcome {
            is_edit_tool: crate::outcome_signals::is_edit_tool(&tool_name),
            tool_name,
            baseline_n: r.get(1)?,
            baseline_corrections: r.get(2)?,
            baseline_rereads: r.get(3)?,
            trimmed_n: r.get(4)?,
            trimmed_corrections: r.get(5)?,
            trimmed_rereads: r.get(6)?,
            trimmed_collected: r.get(7)?,
        })
    };
    let rows = match tool_filter {
        Some(t) => stmt.query_map(params![t], map),
        None => stmt.query_map([], map),
    };
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// The north-star metric, made real (CTX-63). A developer-week is net-ahead when, that week, ctx
/// reclaimed a meaningful share of the developer's context AND trimming did not raise harm above the
/// developer's own baseline. Defined in the revamp plan, never instrumented until now. One machine =
/// one developer, so this is a per-week scoreboard for this developer.
///
/// Fail-closed: a week only counts as net-ahead when its safety can actually be confirmed (enough
/// scored trims to compare harm). A week that reclaimed room but has not yet proven the trims were
/// safe is honest "reclaiming, safety not yet confirmed", not a win. That matches ctx's discipline
/// and keeps WNAD from being gamed by volume alone.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WeekNetAhead {
    /// ISO-ish week label (e.g. "2026-W26") and the Monday-or-first date seen, for display.
    pub week: String,
    pub first_day: String,
    /// Tokens ctx actually removed this week: the total of both taxes, output trims plus input
    /// prunes. This is the scoreboard figure and what the reclaim bar checks. The split below labels
    /// where it came from (CTX-68 folds the input side in).
    pub reclaimed_tokens: i64,
    /// Output tax reclaimed: tool-result characters trimmed, in tokens.
    pub output_reclaimed_tokens: i64,
    /// Input tax reclaimed: tool-menu tokens no longer carried because a server is pruned, summed
    /// over the week's managed requests. Fixed savings that repeat every request.
    pub input_reclaimed_tokens: i64,
    /// Tokens ctx could have removed across every would-trim decision (output eligible).
    pub eligible_tokens: i64,
    pub capture_rate: f64,
    /// The reclaim bar for the week: the lower of 50K tokens or 25% of eligible.
    pub reclaim_bar_tokens: i64,
    pub reclaim_ok: bool,
    /// Harm arms this week, over scored (joined) trims. Re-touch is re-read for reference tools,
    /// re-edit for edit tools, so each tool is judged by the signal that means harm for it.
    pub trimmed_scored: i64,
    pub trimmed_harm: i64,
    pub baseline_scored: i64,
    pub baseline_harm: i64,
    /// True when the trimmed re-touch rate did not exceed the baseline rate by more than the margin.
    pub harm_ok: bool,
    /// Too few scored trims to confirm safety this week, so net-ahead stays false (fail-closed).
    pub harm_unconfirmed: bool,
    pub net_ahead: bool,
}

/// Insight-actions: behavior changes the developer made from what ctx showed them (CTX-63 / L4).
/// The plan's insight-engagement KPI ("education only counts when it changes behavior"). ctx has no
/// telemetry, so this counts only actions it logs locally: recovering a trim with ctx_expand, and
/// pruning MCP servers by switching to a leaner profile. Session splits are not tracked, so they are
/// honestly absent rather than guessed.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct InsightActions {
    /// Times the agent pulled a trimmed original back with ctx_expand (engaged with reversibility).
    pub recoveries: i64,
    /// Profile switches that removed at least one MCP server (acted on the waste / bill insight).
    pub mcp_prunes: i64,
    pub total: i64,
}

pub fn insight_actions(conn: &Connection) -> InsightActions {
    let recoveries: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM rewind_store WHERE expanded_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    // A prune is a profile change that removed a server. `servers_removed` is a JSON array string;
    // empty ('[]', '', NULL) means the switch added or kept servers, not a prune.
    let mcp_prunes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM profile_changes
             WHERE servers_removed IS NOT NULL
               AND TRIM(servers_removed) NOT IN ('', '[]')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    InsightActions {
        recoveries,
        mcp_prunes,
        total: recoveries + mcp_prunes,
    }
}

/// Reclaim floor and eligible fraction from the plan's WNAD definition, and the harm margin (shared
/// with the causal gate) plus the minimum scored trims to confirm a week's safety.
const WNAD_RECLAIM_FLOOR_TOKENS: i64 = 50_000;
const WNAD_ELIGIBLE_FRACTION: f64 = 0.25;
const WNAD_HARM_MARGIN: f64 = 0.10;
const WNAD_MIN_SCORED_TRIMS: i64 = 10;

pub fn weekly_net_ahead(conn: &Connection) -> Vec<WeekNetAhead> {
    // Input tax reclaimed per week: the fixed per-request prune savings the hook recorded on every
    // managed request (CTX-68), summed by week and keyed the same way as the output query below.
    let mut input_by_week: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT strftime('%Y-W%W', ts) AS wk, COALESCE(SUM(tokens_saved), 0)
         FROM requests WHERE tokens_saved > 0 GROUP BY wk",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
            for (wk, toks) in rows.flatten() {
                input_by_week.insert(wk, toks);
            }
        }
    }

    let retouch = format!(
        "(CASE WHEN {edit} THEN COALESCE(outcome_edit_follow,0) ELSE COALESCE(outcome_reread,0) END)",
        edit = crate::outcome_signals::edit_tool_sql_in_list("tool_name")
    );
    let sql = format!(
        "SELECT strftime('%Y-W%W', ts) AS wk,
                MIN(date(ts)),
                COALESCE(SUM(CASE WHEN applied=1 THEN (chars_in - would_chars_out) ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lines_drop>0 THEN (chars_in - would_chars_out) ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=1 AND lines_drop>0 THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=1 AND lines_drop>0 AND {retouch}=1 AND COALESCE(outcome_recovered,0)=0 THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=0 AND lines_drop>0 THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN COALESCE(outcome_joined,0)=1 AND applied=0 AND lines_drop>0 AND {retouch}=1 THEN 1 ELSE 0 END),0)
         FROM compress_decisions
         WHERE 1=1{EXCLUDE_SELF_DEV}
         GROUP BY wk
         ORDER BY wk DESC"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |r| {
        let week: String = r.get(0)?;
        let reclaimed_chars: i64 = r.get(2)?;
        let eligible_chars: i64 = r.get(3)?;
        let trimmed_scored: i64 = r.get(4)?;
        let trimmed_harm: i64 = r.get(5)?;
        let baseline_scored: i64 = r.get(6)?;
        let baseline_harm: i64 = r.get(7)?;

        let output_reclaimed_tokens = reclaimed_chars / 4;
        let input_reclaimed_tokens = input_by_week.get(&week).copied().unwrap_or(0);
        // The scoreboard figure and the reclaim bar both run on the total of both taxes.
        let reclaimed_tokens = output_reclaimed_tokens + input_reclaimed_tokens;
        let eligible_tokens = eligible_chars / 4;
        let reclaim_bar_tokens =
            WNAD_RECLAIM_FLOOR_TOKENS.min((eligible_tokens as f64 * WNAD_ELIGIBLE_FRACTION) as i64);
        let reclaim_ok = reclaimed_tokens >= reclaim_bar_tokens && reclaimed_tokens > 0;

        let harm_unconfirmed = trimmed_scored < WNAD_MIN_SCORED_TRIMS || baseline_scored == 0;
        let trimmed_rate = if trimmed_scored > 0 {
            trimmed_harm as f64 / trimmed_scored as f64
        } else {
            0.0
        };
        let baseline_rate = if baseline_scored > 0 {
            baseline_harm as f64 / baseline_scored as f64
        } else {
            0.0
        };
        let harm_ok = !harm_unconfirmed && trimmed_rate <= baseline_rate + WNAD_HARM_MARGIN;

        Ok(WeekNetAhead {
            week,
            first_day: r.get(1)?,
            reclaimed_tokens,
            output_reclaimed_tokens,
            input_reclaimed_tokens,
            eligible_tokens,
            capture_rate: if eligible_tokens > 0 {
                output_reclaimed_tokens as f64 / eligible_tokens as f64
            } else {
                0.0
            },
            reclaim_bar_tokens,
            reclaim_ok,
            trimmed_scored,
            trimmed_harm,
            baseline_scored,
            baseline_harm,
            harm_ok,
            harm_unconfirmed,
            net_ahead: reclaim_ok && harm_ok,
        })
    });
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Phase 2 randomized per-tool outcome counts (ADR 0009). Unlike `causal_tool_outcomes`, which
/// compares observational shadow vs applied rows, this compares only rows that entered the
/// randomized experiment (`explore_arm` set): control (deliberately kept) vs treatment (trimmed).
/// Because assignment is random within the trim-eligible pool, the gap between the arms is an
/// unbiased estimate of what trimming does on the user's own work. `*_collected` counts every
/// explored row so the UI can show momentum; `*_n` and the rate counts only joined rows, since an
/// outcome is unknown until the label join lands.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExploreToolOutcome {
    pub tool_name: String,
    pub control_collected: i64,
    pub control_n: i64,
    pub control_corrections: i64,
    pub control_rereads: i64,
    pub treatment_collected: i64,
    pub treatment_n: i64,
    pub treatment_corrections: i64,
    pub treatment_rereads: i64,
}

pub fn explore_tool_outcomes(
    conn: &Connection,
    tool_filter: Option<&str>,
) -> Vec<ExploreToolOutcome> {
    let base = format!(
        "SELECT tool_name,
            COALESCE(SUM(CASE WHEN explore_arm='control' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='control' AND outcome_joined=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='control' AND outcome_joined=1 AND COALESCE(outcome_correction,0)=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='control' AND outcome_joined=1 AND COALESCE(outcome_reread,0)=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='treatment' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='treatment' AND outcome_joined=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='treatment' AND outcome_joined=1 AND COALESCE(outcome_correction,0)=1 THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN explore_arm='treatment' AND outcome_joined=1 AND COALESCE(outcome_reread,0)=1 THEN 1 ELSE 0 END),0)
         FROM compress_decisions
         WHERE explore_arm IS NOT NULL{EXCLUDE_SELF_DEV}"
    );
    let sql = match tool_filter {
        Some(_) => format!("{base} AND tool_name = ?1 GROUP BY tool_name"),
        None => format!("{base} GROUP BY tool_name ORDER BY COUNT(*) DESC"),
    };
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let map = |r: &rusqlite::Row<'_>| {
        Ok(ExploreToolOutcome {
            tool_name: r.get(0)?,
            control_collected: r.get(1)?,
            control_n: r.get(2)?,
            control_corrections: r.get(3)?,
            control_rereads: r.get(4)?,
            treatment_collected: r.get(5)?,
            treatment_n: r.get(6)?,
            treatment_corrections: r.get(7)?,
            treatment_rereads: r.get(8)?,
        })
    };
    let rows = match tool_filter {
        Some(t) => stmt.query_map(params![t], map),
        None => stmt.query_map([], map),
    };
    match rows {
        Ok(it) => it.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Recent user turns flagged as corrections for SGR TaskFrame.
pub fn correction_snippets_for_session(
    conn: &Connection,
    external_session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<String>> {
    let Some(sid) = external_session_id.filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let pattern = format!("%{sid}%");
    let mut stmt = conn.prepare(
        "SELECT t.human_text_prefix FROM turns t
         JOIN sessions s ON s.id = t.session_id
         WHERE s.external_key LIKE ?1
           AND t.flags LIKE '%correction%'
         ORDER BY t.turn_index DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit as i64], |r| r.get::<_, String>(0))?;
    Ok(rows
        .filter_map(|r| r.ok())
        .filter(|s| !s.trim().is_empty())
        .collect())
}

/// How many turns after a compaction event a correction still counts as "following" it.
/// Wider than the per-tool correction window (a compaction's effects can surface a turn or
/// two later) but tight enough that an unrelated correction far down the session is not
/// attributed to it. Turn index, not wall clock, is the ground truth here: it is contiguous
/// per session and present on every surface that persists turns. See ADR 0016.
pub const COMPACTION_FOLLOWUP_WINDOW_TURNS: i64 = 5;

/// Per-surface compaction-harm counts (ADR 0016 / CTX-25). Honest by construction: a surface
/// with no persisted compaction signal reports `confidence == "unknown"` with `None` counts,
/// never zero, so the UI says "we can't see this yet" instead of implying a clean result. No
/// causal claim is made: `followed_by_correction` means a correction turn landed within the
/// window after a compaction, not that the compaction caused it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompactionFollowups {
    pub surface: String,
    /// "observed" when this surface has the full signal (compaction events and the correction
    /// follow-up, like Claude Code). "observed_low" when we count real compaction events live but
    /// can't yet compute the follow-up (Cursor, captured from the `preCompact` hook; the
    /// correction join is a named follow-up). "unknown" when the surface has no signal at all.
    pub confidence: String,
    /// Compaction events observed. `None` for an unknown surface.
    pub compaction_events: Option<i64>,
    /// Compaction events with at least one correction within the window after them. `None` when
    /// we count compactions but don't compute the follow-up yet (an "observed_low" surface).
    pub followed_by_correction: Option<i64>,
    /// Distinct sessions in which at least one compaction occurred.
    pub sessions_with_compaction: Option<i64>,
    /// The window used, in turns.
    pub window_turns: i64,
}

/// Per-surface count of corrections that followed a native compaction within
/// `COMPACTION_FOLLOWUP_WINDOW_TURNS`. Read-only. Claude Code is computed from the persisted
/// `turns` table (compaction = a `pre_compact` flagged turn, the exchange right before a
/// `compactMetadata` system row). Cursor is computed from `cursor_compactions`, captured live
/// from the `preCompact` hook, at lower confidence (no correction follow-up yet). Codex is still
/// `unknown` until it exposes a compaction signal. Never reports a causal claim (ADR 0016 / 0023).
pub fn compaction_followups(conn: &Connection) -> Vec<CompactionFollowups> {
    let window = COMPACTION_FOLLOWUP_WINDOW_TURNS;
    vec![
        claude_compaction_followups(conn, window),
        cursor_compaction_followups(conn, window),
        unknown_surface("codex", window),
    ]
}

/// Cursor arm of [`compaction_followups`] (CTX-31 increment 1, ADR 0023). Cursor compactions are
/// captured live from the `preCompact` hook into `cursor_compactions` (its transcript has no
/// compaction marker). We report a real count at `observed_low` confidence: we see the events, but
/// the correction follow-up isn't computed yet, so `followed_by_correction` is `None`. A surface
/// is `unknown` only when we've seen no Cursor activity at all; once ctx has observed any live
/// Cursor tool decision, zero compactions is a real "none seen yet" (parallel to Claude), not an
/// honest-unknown.
fn cursor_compaction_followups(conn: &Connection, window: i64) -> CompactionFollowups {
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM cursor_compactions", [], |r| r.get(0))
        .unwrap_or(0);
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM cursor_compactions WHERE session_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    // Have we ever observed Cursor live? A cursor-surface decision row means yes, even with zero
    // compactions, which lets us show an honest "none yet" instead of "not visible yet".
    let seen_cursor: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM compress_decisions WHERE surface = 'cursor')",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n == 1)
        .unwrap_or(false);

    if events == 0 && !seen_cursor {
        return unknown_surface("cursor", window);
    }

    CompactionFollowups {
        surface: "cursor".to_string(),
        confidence: "observed_low".to_string(),
        compaction_events: Some(events),
        // Increment 2 (the correction follow-up join) isn't built yet; be explicit, not a fake 0.
        followed_by_correction: None,
        sessions_with_compaction: Some(sessions),
        window_turns: window,
    }
}

fn unknown_surface(surface: &str, window: i64) -> CompactionFollowups {
    CompactionFollowups {
        surface: surface.to_string(),
        confidence: "unknown".to_string(),
        compaction_events: None,
        followed_by_correction: None,
        sessions_with_compaction: None,
        window_turns: window,
    }
}

/// Claude Code arm of [`compaction_followups`]. Walks the persisted `turns` timeline per
/// session and counts, for each `pre_compact` turn, whether a `correction` turn lands within
/// `window` turns after it. When no Claude turns are persisted at all the surface is
/// `unknown` (we have not seen sessions), distinct from "observed, zero compactions".
fn claude_compaction_followups(conn: &Connection, window: i64) -> CompactionFollowups {
    // (session_id, turn_index, flags) for every turn, in timeline order. flags is the stored
    // JSON-array string (e.g. ["pre_compact","correction"]) or a legacy bare string; a
    // substring test matches both shapes, same as the existing outcome joins.
    let rows: Vec<(i64, i64, String)> = match conn.prepare(
        "SELECT session_id, turn_index, COALESCE(flags, '')
         FROM turns
         ORDER BY session_id, turn_index",
    ) {
        Ok(mut stmt) => match stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        }) {
            Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
            Err(_) => return unknown_surface("claude-code", window),
        },
        Err(_) => return unknown_surface("claude-code", window),
    };

    if rows.is_empty() {
        return unknown_surface("claude-code", window);
    }

    let mut events = 0i64;
    let mut followed = 0i64;
    let mut sessions_with: std::collections::HashSet<i64> = std::collections::HashSet::new();

    // Walk each session's turns once. Within a session, a compaction at turn index `pc` is
    // "followed by a correction" when some correction turn has index in (pc, pc + window].
    let mut i = 0usize;
    while i < rows.len() {
        let sid = rows[i].0;
        let mut j = i;
        while j < rows.len() && rows[j].0 == sid {
            j += 1;
        }
        let session = &rows[i..j];
        let corrections: Vec<i64> = session
            .iter()
            .filter(|(_, _, f)| f.contains("correction"))
            .map(|(_, idx, _)| *idx)
            .collect();
        for (_, idx, flags) in session {
            if flags.contains("pre_compact") {
                events += 1;
                sessions_with.insert(sid);
                let hit = corrections
                    .iter()
                    .any(|&c| c > *idx && c <= *idx + window);
                if hit {
                    followed += 1;
                }
            }
        }
        i = j;
    }

    CompactionFollowups {
        surface: "claude-code".to_string(),
        confidence: "observed".to_string(),
        compaction_events: Some(events),
        followed_by_correction: Some(followed),
        sessions_with_compaction: Some(sessions_with.len() as i64),
        window_turns: window,
    }
}

/// Recent tool names invoked in this Claude session (for SGR TaskFrame).
pub fn recent_tool_names_for_session(
    conn: &Connection,
    external_session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<String>> {
    let Some(sid) = external_session_id.filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let pattern = format!("%{sid}%");
    let mut stmt = conn.prepare(
        "SELECT ti.tool_name FROM tool_invocations ti
         JOIN sessions s ON s.id = ti.session_id
         WHERE s.external_key LIKE ?1
         ORDER BY ti.ts DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit as i64], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn insert_compress_event(
    conn: &Connection,
    ts: &str,
    session_id: Option<&str>,
    tool_name: &str,
    strategy: &str,
    chars_in: usize,
    chars_out: usize,
    command_or_path: &str,
) -> Result<()> {
    conn.execute(
        r#"INSERT INTO compress_events (ts, session_id, tool_name, strategy, chars_in, chars_out, command_or_path)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        params![
            ts,
            session_id,
            tool_name,
            strategy,
            chars_in as i64,
            chars_out as i64,
            command_or_path,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CompressSummaryRow {
    pub strategy: String,
    pub count: i64,
    pub chars_saved: i64,
}

pub fn compress_summary_today(
    conn: &Connection,
    today: &str,
    since: Option<&str>,
) -> Result<Vec<CompressSummaryRow>> {
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(CompressSummaryRow {
            strategy: r.get(0)?,
            count: r.get(1)?,
            chars_saved: r.get(2)?,
        })
    };
    if let Some(s) = since {
        let mut stmt = conn.prepare(
            "SELECT strategy, COUNT(*), COALESCE(SUM(chars_in - chars_out), 0)
             FROM compress_events
             WHERE substr(ts, 1, 10) = ?1 AND ts >= ?2
             GROUP BY strategy ORDER BY 3 DESC",
        )?;
        let rows = stmt.query_map(params![today, s], map_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    } else {
        let mut stmt = conn.prepare(
            "SELECT strategy, COUNT(*), COALESCE(SUM(chars_in - chars_out), 0)
             FROM compress_events
             WHERE substr(ts, 1, 10) = ?1
             GROUP BY strategy ORDER BY 3 DESC",
        )?;
        let rows = stmt.query_map(params![today], map_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

pub fn compress_summary_all(conn: &Connection) -> Result<Vec<CompressSummaryRow>> {
    let mut stmt = conn.prepare(
        "SELECT strategy, COUNT(*), COALESCE(SUM(chars_in - chars_out), 0)
         FROM compress_events
         GROUP BY strategy ORDER BY 3 DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CompressSummaryRow {
            strategy: r.get(0)?,
            count: r.get(1)?,
            chars_saved: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn compress_totals_today(
    conn: &Connection,
    today: &str,
    since: Option<&str>,
) -> (usize, usize) {
    let sql = if since.is_some() {
        "SELECT COUNT(*), COALESCE(SUM(chars_in - chars_out), 0)
         FROM compress_events WHERE substr(ts, 1, 10) = ?1 AND ts >= ?2"
    } else {
        "SELECT COUNT(*), COALESCE(SUM(chars_in - chars_out), 0)
         FROM compress_events WHERE substr(ts, 1, 10) = ?1"
    };
    let row: (i64, i64) = if let Some(s) = since {
        conn.query_row(sql, params![today, s], |r| Ok((r.get(0)?, r.get(1)?)))
    } else {
        conn.query_row(sql, params![today], |r| Ok((r.get(0)?, r.get(1)?)))
    }
    .unwrap_or((0, 0));
    (row.0.max(0) as usize, row.1.max(0) as usize)
}

/// One-time backfill of the `self_dev` tag for decisions recorded before CTX-32. Going forward the
/// controller tags rows at record time (`agent::is_self_dev_repo`), but rows already in the log are
/// untagged, so without this the live gate stays polluted by ctx's own development until those rows
/// age out. For each repo the log has seen, if that path is ctx's own source repo today, its rows
/// are tagged so the corpus filter excludes them now. Guarded by a meta key so the filesystem scan
/// runs at most once; idempotent and safe under concurrent opens. Repos no longer on disk are left
/// untagged (conservative: kept in the corpus).
fn backfill_self_dev_tag(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row("SELECT 1 FROM meta WHERE k='self_dev_backfill_v1'", [], |_| {
            Ok(())
        })
        .is_ok();
    if already_done {
        return;
    }

    let repos: Vec<String> = match conn.prepare(
        "SELECT DISTINCT json_extract(features_json,'$.repo_key')
         FROM compress_decisions
         WHERE json_extract(features_json,'$.repo_key') IS NOT NULL",
    ) {
        Ok(mut stmt) => stmt
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .map(|it| it.filter_map(|x| x.ok().flatten()).collect())
            .unwrap_or_default(),
        Err(_) => return,
    };

    for repo in repos {
        if crate::agent::is_self_dev_repo(&repo) {
            let _ = conn.execute(
                "UPDATE compress_decisions
                 SET features_json = json_set(features_json,'$.self_dev', json('true'))
                 WHERE json_extract(features_json,'$.repo_key') = ?1
                   AND COALESCE(features_json,'') NOT LIKE '%\"self_dev\":true%'",
                params![repo],
            );
        }
    }

    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('self_dev_backfill_v1', '1')",
        [],
    );
}

/// One-time backfill of the same-file edit-follow label (CTX-46 / ADR 0031) for timestamp-joined
/// decisions recorded before the column existed. Going forward `join_compress_outcomes` sets it as
/// rows join; this fills already-joined Claude/legacy rows so the training corpus carries the label
/// on existing usage, not only future rows. Cursor rows are skipped: their edit detection needs the
/// transcript, not a self-join, and they are excluded from training anyway. Uses the exact shared
/// subquery the live join uses. Meta-guarded so the pass runs at most once; idempotent.
fn backfill_edit_follow_label(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='edit_follow_backfill_v1'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    let edit_follow = edit_follow_value_sql();
    let _ = conn.execute(
        &format!(
            "UPDATE compress_decisions
             SET outcome_edit_follow = {edit_follow}
             WHERE outcome_joined = 1
               AND outcome_edit_follow IS NULL
               AND session_id IS NOT NULL
               AND COALESCE(surface,'') != 'cursor'",
            edit_follow = edit_follow
        ),
        params![window_days],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('edit_follow_backfill_v1', '1')",
        [],
    );
}

/// Recompute `outcome_edit_follow` for existing rows under the content-overlap rule (CTX-62 fix).
/// The old file-level signal fired on any second edit to a file and read as ~70% harm on normal
/// multi-part editing. The new rule requires a follow-up edit to have sought the exact text an edit
/// wrote. Rows recorded before the content anchor existed have none, so they resolve to 0 here and
/// the inflated file-level rate drops out; reads keep their unchanged "needed whole" meaning. Runs
/// once (its own meta key), overwriting the old values rather than only filling NULLs.
fn recompute_edit_follow_content_anchor(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    if conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='edit_follow_content_anchor_v1'",
            [],
            |_| Ok(()),
        )
        .is_ok()
    {
        return;
    }
    let window_days = CORRECTION_WINDOW_MINUTES / 1440.0;
    let edit_follow = edit_follow_value_sql();
    let _ = conn.execute(
        &format!(
            "UPDATE compress_decisions
             SET outcome_edit_follow = {edit_follow}
             WHERE outcome_joined = 1
               AND session_id IS NOT NULL
               AND COALESCE(surface,'') != 'cursor'",
            edit_follow = edit_follow
        ),
        params![window_days],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('edit_follow_content_anchor_v1', '1')",
        [],
    );
    let _ = refresh_outcome_signals(conn);
}

/// One-time backfill of `path_role` on read decisions recorded before CTX-45 wired live logging.
/// Uses `command_or_path`, the same path the activity feed shows. Meta-guarded, idempotent.
fn backfill_path_role(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='path_role_backfill_v2'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }

    let rows: Vec<(i64, String)> = match conn.prepare(
        "SELECT id, command_or_path FROM compress_decisions
         WHERE kind = 'read'
           AND command_or_path IS NOT NULL
           AND TRIM(command_or_path) != ''
           AND json_extract(COALESCE(features_json,'{}'), '$.path_role') IS NULL",
    ) {
        Ok(mut stmt) => stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default(),
        Err(_) => return,
    };

    for (id, path) in rows {
        let Some(role) = crate::agent::path_role_of(&path) else {
            continue;
        };
        let _ = conn.execute(
            "UPDATE compress_decisions
             SET features_json = json_set(COALESCE(features_json,'{}'), '$.path_role', ?2)
             WHERE id = ?1",
            params![id, role],
        );
    }

    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('path_role_backfill_v2', '1')",
        [],
    );
}

/// One-time rejoin of every outcome label under CTX-48 / ADR 0033: reset joined rows, rerun
/// both timestamp and transcript joins, then refresh observation-only `outcome_signals`.
fn backfill_rejoin_outcome_labels_v3(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='rejoin_outcome_labels_v3'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }

    let _ = conn.execute(
        "UPDATE compress_decisions
         SET outcome_joined = 0, outcome_correction = NULL, outcome_signals = NULL
         WHERE outcome_joined = 1",
        [],
    );
    let _ = join_compress_outcomes(conn);
    if let Some(home) = dirs::home_dir() {
        let _ = crate::surface::ingest::join_transcript_outcomes(conn, &home);
    }
    let _ = refresh_outcome_signals(conn);

    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('rejoin_outcome_labels_v3', '1')",
        [],
    );
}

/// Strip spurious `correction` flags from interrupt-only turns stored before CTX-48.
fn backfill_interrupt_turn_flags_v1(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='interrupt_turn_flags_v1'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    let _ = conn.execute(
        "UPDATE turns
         SET flags = '[\"aborted\"]'
         WHERE flags LIKE '%aborted%'
           AND flags LIKE '%correction%'
           AND (
             LOWER(COALESCE(human_text_prefix,'')) LIKE '%[request interrupted by user%'
             OR LOWER(COALESCE(human_text_prefix,'')) = ''
           )",
        [],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('interrupt_turn_flags_v1', '1')",
        [],
    );
}

/// Rejoin after interrupt turn cleanup so join triggers and observational signals match CTX-48.
fn backfill_rejoin_outcome_labels_v4(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='rejoin_outcome_labels_v4'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    let _ = conn.execute(
        "UPDATE compress_decisions
         SET outcome_joined = 0, outcome_correction = NULL, outcome_signals = NULL
         WHERE outcome_joined = 1",
        [],
    );
    let _ = join_compress_outcomes(conn);
    if let Some(home) = dirs::home_dir() {
        let _ = crate::surface::ingest::join_transcript_outcomes(conn, &home);
    }
    let _ = refresh_outcome_signals(conn);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('rejoin_outcome_labels_v4', '1')",
        [],
    );
}

/// Rejoin after TodoWrite content fingerprints and legacy state-mutation reread exclusion (CTX-49).
fn backfill_rejoin_outcome_labels_v5(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='rejoin_outcome_labels_v5'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    let _ = conn.execute(
        "UPDATE compress_decisions
         SET outcome_joined = 0, outcome_correction = NULL, outcome_reread = NULL,
             outcome_edit_follow = NULL, outcome_signals = NULL
         WHERE outcome_joined = 1",
        [],
    );
    let _ = join_compress_outcomes(conn);
    if let Some(home) = dirs::home_dir() {
        let _ = crate::surface::ingest::join_transcript_outcomes(conn, &home);
    }
    let _ = refresh_outcome_signals(conn);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('rejoin_outcome_labels_v5', '1')",
        [],
    );
}

/// Rejoin after steer-aware correction labels and compression workaround signals (CTX-50).
fn backfill_rejoin_outcome_labels_v6(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='rejoin_outcome_labels_v6'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    backfill_steer_turn_flags_v1(conn);
    let _ = conn.execute(
        "UPDATE compress_decisions
         SET outcome_joined = 0, outcome_correction = NULL, outcome_reread = NULL,
             outcome_edit_follow = NULL, outcome_signals = NULL
         WHERE outcome_joined = 1",
        [],
    );
    let _ = join_compress_outcomes(conn);
    if let Some(home) = dirs::home_dir() {
        let _ = crate::surface::ingest::join_transcript_outcomes(conn, &home);
    }
    let _ = refresh_outcome_signals(conn);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('rejoin_outcome_labels_v6', '1')",
        [],
    );
}

/// Reclassify mislabeled explicit corrections that are session steers (CTX-50).
fn repair_steer_turn_flags(conn: &Connection) {
    let mut sel = match conn.prepare(
        "SELECT id, COALESCE(human_text_prefix,'') FROM turns WHERE flags LIKE '%correction_explicit%'",
    ) {
        Ok(s) => s,
        Err(_) => return,
    };
    let rows: Vec<(i64, String)> = sel
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()
        .map(|iter| iter.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();
    for (id, text) in rows {
        if crate::outcome_signals::classify_correction(
            &text,
            crate::outcome_signals::DEFAULT_TERSE_MAX_CHARS,
        ) != crate::outcome_signals::CorrectionClass::Steer
        {
            continue;
        }
        let _ = conn.execute(
            "UPDATE turns SET flags = '[\"session_steer\"]' WHERE id = ?1",
            params![id],
        );
    }
}

/// One-time migration wrapper for steer turn cleanup.
fn backfill_steer_turn_flags_v1(conn: &Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    );
    let already_done = conn
        .query_row(
            "SELECT 1 FROM meta WHERE k='steer_turn_flags_v1'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_done {
        return;
    }
    repair_steer_turn_flags(conn);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('steer_turn_flags_v1', '1')",
        [],
    );
}

/// Repair the live corpus for honest dashboard display: clean interrupt flags, rejoin outcomes.
pub fn repair_corpus(conn: &Connection) -> Result<(usize, usize, usize)> {
    let _ = conn.execute(
        "UPDATE turns
         SET flags = '[\"aborted\"]'
         WHERE flags LIKE '%aborted%'
           AND flags LIKE '%correction%'
           AND (
             LOWER(COALESCE(human_text_prefix,'')) LIKE '%[request interrupted by user%'
             OR LOWER(COALESCE(human_text_prefix,'')) = ''
           )",
        [],
    );
    repair_steer_turn_flags(conn);
    let _ = conn.execute(
        "UPDATE compress_decisions
         SET outcome_joined = 0, outcome_correction = NULL, outcome_reread = NULL,
             outcome_edit_follow = NULL, outcome_signals = NULL
         WHERE outcome_joined = 1",
        [],
    );
    let _ = join_compress_outcomes(conn);
    if let Some(home) = dirs::home_dir() {
        let _ = crate::surface::ingest::join_transcript_outcomes(conn, &home);
    }
    let _ = refresh_outcome_signals(conn);
    let joined: i64 = conn.query_row(
        "SELECT COALESCE(SUM(outcome_joined),0) FROM compress_decisions",
        [],
        |r| r.get(0),
    )?;
    let corrections: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(outcome_correction,0)),0) FROM compress_decisions WHERE outcome_joined=1",
        [],
        |r| r.get(0),
    )?;
    let interrupt_clean: i64 = conn.query_row(
        "SELECT COUNT(*) FROM turns
         WHERE flags LIKE '%aborted%' AND flags NOT LIKE '%correction%'
           AND LOWER(COALESCE(human_text_prefix,'')) LIKE '%[request interrupted by user%'",
        [],
        |r| r.get(0),
    )?;
    Ok((joined as usize, corrections as usize, interrupt_clean as usize))
}

pub fn ensure_schema(conn: &Connection) -> Result<()> {
    // Run column migrations unconditionally (idempotent ALTER TABLE checks)
    migrate_hook_traces_savings_columns(conn);
    migrate_hook_traces_adaptive_fired(conn);
    migrate_hook_traces_ab_columns(conn);
    migrate_hook_traces_power_columns(conn);
    migrate_hook_traces_prefix_and_budget_columns(conn);
    migrate_hook_traces_pinned_profile(conn);
    migrate_hook_traces_compress_columns(conn);
    migrate_hook_traces_expansion_column(conn);
    migrate_requests_prefix_and_budget_columns(conn);
    migrate_allowance_snapshots_table(conn);
    migrate_compress_events_table(conn);
    migrate_compress_decisions_table(conn);
    migrate_cursor_compactions_table(conn);
    backfill_self_dev_tag(conn);
    backfill_edit_follow_label(conn);
    recompute_edit_follow_content_anchor(conn);
    backfill_path_role(conn);
    backfill_rejoin_outcome_labels_v3(conn);
    backfill_interrupt_turn_flags_v1(conn);
    backfill_rejoin_outcome_labels_v4(conn);
    backfill_rejoin_outcome_labels_v5(conn);
    backfill_rejoin_outcome_labels_v6(conn);

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
            tools_sent_by_server TEXT,
            inject_chars INTEGER DEFAULT 0,
            adaptive_chars INTEGER DEFAULT 0,
            budget_blocked INTEGER DEFAULT 0
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

        -- Tool-miss harm signal (CTX-66 / M-D): the input-side re-read. Each row is a reach for a
        -- tool ctx had hidden (pruned or profile-denied), so the developer's own usage says the hide
        -- cost them a round trip. `hidden_by` records why it was hidden so the earn-it gate can judge
        -- prunes apart from profile denials.
        CREATE TABLE IF NOT EXISTS tool_misses (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            session_id TEXT,
            tool_name TEXT,
            server_prefix TEXT NOT NULL,
            hidden_by TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_tool_misses_server ON tool_misses(server_prefix);
        CREATE INDEX IF NOT EXISTS idx_tool_misses_ts ON tool_misses(ts);

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
            inject_chars INTEGER DEFAULT 0,
            adaptive_chars INTEGER DEFAULT 0,
            budget_blocked INTEGER DEFAULT 0,
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
    inject_chars: usize,
    adaptive_chars: usize,
    budget_blocked: bool,
    pinned_profile: Option<&str>,
    effective_profile: Option<&str>,
    prompt_text: Option<&str>,
    tools_expanded_json: Option<&str>,
) -> Result<i64> {
    ensure_schema(conn)?;
    let ts = chrono::Utc::now().to_rfc3339();
    let prompt_stored = prompt_text
        .map(|p| p.chars().take(2000).collect::<String>())
        .filter(|s| !s.is_empty());
    conn.execute(
        r#"INSERT INTO hook_traces (
            ts, session_id, parent_session_id, working_directory, profile, mode,
            auto_selected, auto_trigger, inject_fired, coach_kind, budget_fired,
            tools_kept, tools_removed, tokens_saved, adaptive_fired, ab_group,
            inject_chars, adaptive_chars, budget_blocked, pinned_profile, effective_profile,
            human_text_prefix, tools_expanded_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)"#,
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
            inject_chars as i64,
            adaptive_chars as i64,
            budget_blocked as i64,
            pinned_profile,
            effective_profile,
            prompt_stored,
            tools_expanded_json.unwrap_or("[]"),
        ],
    )?;
    stamp_ctx_active_since(conn);
    Ok(conn.last_insert_rowid())
}

/// Merge recovery expansions onto the latest hook trace for a session (Stop-hook Tier 1).
pub fn append_hook_trace_expansions(
    conn: &Connection,
    session_id: &str,
    added: &[crate::semantic_tools::ToolExpansionEntry],
) -> Result<()> {
    if added.is_empty() {
        return Ok(());
    }
    ensure_schema(conn)?;
    let trace_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM hook_traces
             WHERE session_id = ?1 OR session_id LIKE '%' || ?1 || '%'
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![session_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(trace_id) = trace_id else {
        return Ok(());
    };
    let existing_json: String = conn
        .query_row(
            "SELECT COALESCE(tools_expanded_json, '[]') FROM hook_traces WHERE id = ?1",
            rusqlite::params![trace_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "[]".to_string());
    let mut merged: Vec<crate::semantic_tools::ToolExpansionEntry> =
        serde_json::from_str(&existing_json).unwrap_or_default();
    for entry in added {
        if !merged
            .iter()
            .any(|e| e.target.eq_ignore_ascii_case(&entry.target))
        {
            merged.push(entry.clone());
        }
    }
    let json = serde_json::to_string(&merged)?;
    conn.execute(
        "UPDATE hook_traces SET tools_expanded_json = ?1 WHERE id = ?2",
        rusqlite::params![json, trace_id],
    )?;
    Ok(())
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
    #[serde(default)]
    pub inject_chars: usize,
    #[serde(default)]
    pub adaptive_chars: usize,
    #[serde(default)]
    pub budget_blocked: bool,
    pub pinned_profile: Option<String>,
    pub effective_profile: Option<String>,
    #[serde(default)]
    pub compress_chars_saved: usize,
    #[serde(default)]
    pub compress_event_count: usize,
    #[serde(default)]
    pub tools_expanded: Vec<crate::semantic_tools::ToolExpansionEntry>,
}

/// One bucket of the cache-safety audit (CTX-28). Requests are grouped by whether ctx
/// edited the cached prefix on that request, so we can see if prefix edits correlate with
/// more cache writes (the 1.25x penalty) and fewer cache reads (the 0.1x discount).
///
/// Honesty note: the token figures come from enrichment, which joins a hook trace to a
/// turn's usage by session and time. It is a correlational signal across the user's own
/// traffic, not a controlled A/B. Read it as a smell test, not proof.
#[derive(Debug, Default, serde::Serialize)]
pub struct CacheAuditBucket {
    pub category: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

/// Aggregate enriched hook traces by what ctx did to the cached prefix on each request.
/// `tools_removed > 0` means MCP tool schemas were stripped from the `tools` block (front of
/// the cached prefix). `inject_chars > 0` means content was added to the system block. Both
/// sit inside Anthropic's cached prefix; tool-output trimming does not and is excluded here
/// because it edits message content after the prefix. Read-only.
pub fn cache_audit(conn: &Connection, ts_since: Option<&str>) -> Vec<CacheAuditBucket> {
    let _ = ensure_schema(conn);
    let select = r#"SELECT
            CASE
                WHEN COALESCE(tools_removed,0) > 0 AND COALESCE(inject_chars,0) > 0 THEN 'tools+system'
                WHEN COALESCE(tools_removed,0) > 0 THEN 'tools-filtered'
                WHEN COALESCE(inject_chars,0) > 0 THEN 'system-injected'
                ELSE 'untouched'
            END AS category,
            COUNT(*) AS requests,
            SUM(COALESCE(input_tokens,0)) AS input_sum,
            SUM(COALESCE(cache_read_tokens,0)) AS cache_read_sum,
            SUM(COALESCE(cache_creation_tokens,0)) AS cache_creation_sum
        FROM hook_traces
        WHERE COALESCE(enriched,0) = 1
          AND (cache_read_tokens IS NOT NULL OR cache_creation_tokens IS NOT NULL)"#;
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(CacheAuditBucket {
            category: r.get(0)?,
            requests: r.get(1)?,
            input_tokens: r.get::<_, i64>(2)?,
            cache_read_tokens: r.get::<_, i64>(3)?,
            cache_creation_tokens: r.get::<_, i64>(4)?,
        })
    };
    let mut out = Vec::new();
    let res = if let Some(s) = ts_since {
        let sql = format!("{select} AND ts >= ?1 GROUP BY category ORDER BY category");
        conn.prepare(&sql).and_then(|mut stmt| {
            let rows = stmt.query_map(params![s], map_row)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(row?);
            }
            Ok(v)
        })
    } else {
        let sql = format!("{select} GROUP BY category ORDER BY category");
        conn.prepare(&sql).and_then(|mut stmt| {
            let rows = stmt.query_map([], map_row)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(row?);
            }
            Ok(v)
        })
    };
    if let Ok(v) = res {
        out = v;
    }
    out
}

/// One arm of a running A/B, scoped to one feature, with the cache tokens for that arm.
/// Used to compare cache behavior between treatment (feature on) and control (feature off)
/// on the same machine and period, which is the clean way to answer "does this feature bust
/// the cache". Read-only.
#[derive(Debug, Default, serde::Serialize)]
pub struct CacheAuditArm {
    pub feature: String,
    pub arm: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost: f64,
}

/// Break the cache audit down by A/B arm for the prefix-affecting features, reading the
/// `ab_group` tags the hook records (e.g. `P:T I:C ...`). For each feature we return the
/// treatment and control rows that have enriched cache data, so the caller can compare cache
/// shares and cost between the arms. Only meaningful while an experiment is running with that
/// feature split below 100. Read-only.
pub fn cache_audit_arms(conn: &Connection, ts_since: Option<&str>) -> Vec<CacheAuditArm> {
    let _ = ensure_schema(conn);
    // (tag in ab_group, human label). These all edit the cached prefix except compress, which
    // edits tool output after the prefix and is included only as a cache-neutral reference.
    let features: [(&str, &str); 4] = [
        ("P", "profile (MCP filtering)"),
        ("I", "system prefix injection"),
        ("A", "adaptive prefix"),
        ("C", "coaching"),
    ];
    let agg = "SELECT COUNT(*), SUM(COALESCE(input_tokens,0)), \
               SUM(COALESCE(cache_read_tokens,0)), SUM(COALESCE(cache_creation_tokens,0)), \
               SUM(COALESCE(cost_usd,0)) FROM hook_traces \
               WHERE enriched=1 AND ab_group IS NOT NULL AND ab_group LIKE ?1";
    let with_ts = format!("{agg} AND ts >= ?2");
    let mut out = Vec::new();
    for (tag, label) in features {
        for (arm_tag, arm_name) in [("T", "treatment"), ("C", "control")] {
            let pat = format!("%{tag}:{arm_tag}%");
            let map = |r: &rusqlite::Row<'_>| {
                Ok(CacheAuditArm {
                    feature: label.to_string(),
                    arm: arm_name.to_string(),
                    requests: r.get(0)?,
                    input_tokens: r.get::<_, i64>(1).unwrap_or(0),
                    cache_read_tokens: r.get::<_, i64>(2).unwrap_or(0),
                    cache_creation_tokens: r.get::<_, i64>(3).unwrap_or(0),
                    total_cost: r.get::<_, f64>(4).unwrap_or(0.0),
                })
            };
            let row = match ts_since {
                Some(s) => conn.query_row(&with_ts, params![pat, s], map),
                None => conn.query_row(agg, params![pat], map),
            };
            if let Ok(a) = row {
                if a.requests > 0 {
                    out.push(a);
                }
            }
        }
    }
    out
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
        human_text_prefix,
        COALESCE(inject_chars, 0) AS inject_chars,
        COALESCE(adaptive_chars, 0) AS adaptive_chars,
        COALESCE(budget_blocked, 0) AS budget_blocked,
        pinned_profile,
        effective_profile,
        COALESCE(compress_chars_saved, 0) AS compress_chars_saved,
        COALESCE(compress_event_count, 0) AS compress_event_count,
        COALESCE(tools_expanded_json, '[]') AS tools_expanded_json
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
            inject_chars: r.get::<_, i64>(25)? as usize,
            adaptive_chars: r.get::<_, i64>(26)? as usize,
            budget_blocked: r.get::<_, i64>(27)? != 0,
            pinned_profile: r.get(28)?,
            effective_profile: r.get(29)?,
            compress_chars_saved: r.get::<_, i64>(30)? as usize,
            compress_event_count: r.get::<_, i64>(31)? as usize,
            tools_expanded: {
                let json: String = r.get(32)?;
                serde_json::from_str(&json).unwrap_or_default()
            },
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
    for row in &mut out {
        if row.compress_event_count > 0 {
            continue;
        }
        let (saved, count) =
            compress_savings_for_hook_turn(conn, &row.ts, row.session_id.as_deref())
                .unwrap_or((0, 0));
        if count > 0 {
            row.compress_chars_saved = saved;
            row.compress_event_count = count;
        }
    }
    Ok(out)
}

/// Attach PostToolUse compress totals to hook trace rows (runs during ingest).
pub fn backfill_hook_trace_compress(conn: &Connection) -> Result<usize> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare("SELECT id, ts, session_id FROM hook_traces")?;
    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .filter_map(|x| x.ok())
        .collect();
    let mut updated = 0usize;
    for (id, ts, session_id) in rows {
        let (saved, count) =
            compress_savings_for_hook_turn(conn, &ts, session_id.as_deref()).unwrap_or((0, 0));
        conn.execute(
            "UPDATE hook_traces SET compress_chars_saved = ?1, compress_event_count = ?2 WHERE id = ?3",
            params![saved as i64, count as i64, id],
        )?;
        if count > 0 {
            updated += 1;
        }
    }
    Ok(updated)
}

/// Sum PostToolUse compression between this prompt trace and the next one in the session.
fn compress_savings_for_hook_turn(
    conn: &Connection,
    trace_ts: &str,
    session_id: Option<&str>,
) -> Result<(usize, usize)> {
    let next_ts: Option<String> = if let Some(sid) = session_id.filter(|s| !s.is_empty()) {
        conn.query_row(
            "SELECT ts FROM hook_traces WHERE session_id = ?1 AND ts > ?2 ORDER BY ts ASC LIMIT 1",
            params![sid, trace_ts],
            |r| r.get(0),
        )
        .optional()?
    } else {
        None
    };

    let end_ts = next_ts.unwrap_or_else(|| {
        trace_ts
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map(|dt| dt + chrono::Duration::hours(6))
            .unwrap_or_else(|_| chrono::Utc::now() + chrono::Duration::hours(6))
            .to_rfc3339()
    });

    let (saved, count): (i64, i64) = if let Some(sid) = session_id.filter(|s| !s.is_empty()) {
        conn.query_row(
            "SELECT COALESCE(SUM(chars_in - chars_out), 0), COUNT(*)
             FROM compress_events
             WHERE ts > ?1 AND ts <= ?2
               AND (session_id = ?3 OR session_id IS NULL)",
            params![trace_ts, end_ts, sid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
    } else {
        conn.query_row(
            "SELECT COALESCE(SUM(chars_in - chars_out), 0), COUNT(*)
             FROM compress_events WHERE ts > ?1 AND ts <= ?2",
            params![trace_ts, end_ts],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
    };
    Ok((saved.max(0) as usize, count.max(0) as usize))
}

/// Match unenriched hook_trace rows to the nearest JSONL turn by session + prompt text.
/// Called during ingest after sessions/turns are populated.
pub fn enrich_hook_traces(conn: &Connection) -> Result<usize> {
    ensure_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, ts, session_id, COALESCE(human_text_prefix, '') FROM hook_traces WHERE enriched = 0",
    )?;
    let pending: Vec<(i64, String, Option<String>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .filter_map(|x| x.ok())
        .collect();

    let mut count = 0usize;
    for (trace_id, trace_ts, session_id, hook_prompt) in &pending {
        let prompt_prefix: String = hook_prompt.chars().take(120).collect();
        let matched: Option<(i64, i64, i64, i64, f64, String, Option<String>)> =
            if let Some(sid) = session_id {
                conn.query_row(
                    r#"SELECT t.input_tokens, t.output_tokens, t.cache_read_tokens,
                              t.cache_creation_tokens, t.cost_usd, t.model, t.human_text_prefix
                       FROM turns t
                       JOIN sessions s ON t.session_id = s.id
                       WHERE s.external_key LIKE '%' || ?1 || '%'
                         AND t.ts IS NOT NULL
                         AND (
                           LENGTH(?2) = 0
                           OR t.human_text_prefix = substr(?2, 1, 500)
                           OR ?2 LIKE t.human_text_prefix || '%'
                           OR t.human_text_prefix LIKE substr(?2, 1, 120) || '%'
                         )
                       ORDER BY
                         CASE
                           WHEN LENGTH(?2) > 0 AND t.human_text_prefix = substr(?2, 1, 500) THEN 0
                           WHEN LENGTH(?2) > 0 AND ?2 LIKE t.human_text_prefix || '%' THEN 1
                           ELSE 2
                         END,
                         ABS(julianday(t.ts) - julianday(?3))
                       LIMIT 1"#,
                    params![sid, prompt_prefix, trace_ts],
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

        let matched = matched.or_else(|| {
            conn.query_row(
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

        if let Some((inp, outp, cr, cc, cost, model, turn_prefix)) = matched {
            let keep_prompt = !hook_prompt.trim().is_empty();
            let prompt_to_store = if keep_prompt {
                hook_prompt.clone()
            } else {
                turn_prefix.unwrap_or_default()
            };
            conn.execute(
                r#"UPDATE hook_traces SET
                    input_tokens = ?1, output_tokens = ?2, cache_read_tokens = ?3,
                    cache_creation_tokens = ?4, cost_usd = ?5, model = ?6, enriched = 1,
                    human_text_prefix = ?7
                   WHERE id = ?8"#,
                params![inp, outp, cr, cc, cost, model, prompt_to_store, trace_id],
            )?;
            count += 1;
        }
    }
    let _ = backfill_parent_session_ids(conn);
    let _ = backfill_hook_trace_compress(conn)?;
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
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
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
                .map(
                    |(session_id, (cost_usd, requests, tokens_saved))| TaskCostChild {
                        session_id,
                        cost_usd,
                        requests,
                        tokens_saved,
                    },
                )
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
        .query_row("SELECT v FROM meta WHERE k = 'ctx_active_since'", [], |r| {
            r.get(0)
        })
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
    conn.query_row("SELECT v FROM meta WHERE k = 'ctx_active_since'", [], |r| {
        r.get(0)
    })
    .optional()
    .ok()
    .flatten()
}

pub fn get_meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT v FROM meta WHERE k = ?1", params![key], |r| {
        r.get(0)
    })
    .optional()
    .ok()
    .flatten()
}

/// Clear the install watermark so the dashboard shows all historical rows again until the next hook or request stamps it.
pub fn reset_ctx_active_since(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE k = 'ctx_active_since'", [])?;
    Ok(())
}

/// After reinstall, `ctx_active_since` is often newer than sessions already in SQLite from JSONL
/// re-ingest. Align the watermark to the earliest indexed session so the default dashboard view
/// is not empty.
pub fn maybe_reset_stale_install_watermark(conn: &Connection) -> Result<bool> {
    let Some(since) = get_ctx_active_since(conn) else {
        return Ok(false);
    };
    let predates: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sessions
                WHERE started_at != '' AND started_at < ?1
                LIMIT 1
             )",
            params![since],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !predates {
        return Ok(false);
    }
    let earliest: Option<String> = conn
        .query_row(
            "SELECT MIN(started_at) FROM sessions WHERE started_at != ''",
            [],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(ts) = earliest.filter(|s| !s.is_empty()) {
        conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('ctx_active_since', ?1)",
            params![ts],
        )?;
        return Ok(true);
    }
    reset_ctx_active_since(conn)?;
    Ok(true)
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
            tools_sent_by_server, inject_chars, adaptive_chars, budget_blocked
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"#,
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
            rec.inject_chars as i64,
            rec.adaptive_chars as i64,
            rec.budget_blocked as i64,
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
                tools_sent_by_server, inject_chars, adaptive_chars, budget_blocked
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
            inject_chars: r.get::<_, i64>(17)? as usize,
            adaptive_chars: r.get::<_, i64>(18)? as usize,
            budget_blocked: r.get::<_, i64>(19)? != 0,
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
    conn.execute(
        "DELETE FROM turns WHERE session_id = ?1",
        params![session_id],
    )?;
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

/// Record a tool-miss: the agent reached for a tool ctx had hidden (CTX-66 / M-D). `hidden_by` is
/// "prune" for a server pruned from the tool menu, or "profile" for a profile deny. Best-effort.
pub fn insert_tool_miss(
    conn: &Connection,
    session_id: Option<&str>,
    tool_name: &str,
    server_prefix: &str,
    hidden_by: &str,
    ts: &str,
) -> Result<()> {
    ensure_schema(conn)?;
    conn.execute(
        r#"INSERT INTO tool_misses (ts, session_id, tool_name, server_prefix, hidden_by)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![ts, session_id, tool_name, server_prefix, hidden_by],
    )?;
    Ok(())
}

/// One server's tool-miss line for the harm read.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ToolMissServer {
    pub server: String,
    pub prefix: String,
    pub misses: i64,
    pub sessions_with_miss: i64,
    pub last_miss: Option<String>,
}

/// The input-side harm read (CTX-66 / M-D): reaches for hidden tools, per server, over a window,
/// plus the coarse miss rate the earn-it gate holds a prune against. The mirror of the output-side
/// re-read rate. Baseline is honest: 0 until a hidden tool is actually reached for.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ToolMissStats {
    pub servers: Vec<ToolMissServer>,
    pub total_misses: i64,
    /// Distinct sessions with any tool activity in the window (the rate denominator).
    pub sessions: i64,
    /// total_misses / sessions: the machine's overall tool-miss rate this window.
    pub miss_rate: f64,
    pub window_days: i64,
}

pub fn tool_miss_stats(conn: &Connection, days: u32) -> ToolMissStats {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let mut stats = ToolMissStats {
        window_days: days as i64,
        ..Default::default()
    };

    if let Ok(mut stmt) = conn.prepare(
        "SELECT server_prefix, COUNT(*), COUNT(DISTINCT session_id), MAX(ts)
         FROM tool_misses
         WHERE ts >= ?1 AND server_prefix IS NOT NULL AND server_prefix != ''
         GROUP BY server_prefix
         ORDER BY COUNT(*) DESC",
    ) {
        if let Ok(rows) = stmt.query_map(params![cutoff], |r| {
            Ok(ToolMissServer {
                server: crate::profiles::mcp_prefix_to_server_display(&r.get::<_, String>(0)?),
                prefix: r.get(0)?,
                misses: r.get(1)?,
                sessions_with_miss: r.get(2)?,
                last_miss: r.get(3)?,
            })
        }) {
            for row in rows.flatten() {
                stats.total_misses += row.misses;
                stats.servers.push(row);
            }
        }
    }

    // Denominator: distinct sessions with any tool activity in the window.
    stats.sessions = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM tool_invocations WHERE ts >= ?1",
            params![cutoff],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    stats.miss_rate = if stats.sessions > 0 {
        stats.total_misses as f64 / stats.sessions as f64
    } else {
        0.0
    };
    stats
}

/// Per-server before/after evidence for the auto-prune earn-it gate (CTX-67 / M-E): usage (the
/// dead-weight signal), hidden exposure (sessions active after the server was pruned, the causal
/// arm), and reaches while hidden. One row per server observed in the window.
pub fn server_prune_outcomes(
    conn: &Connection,
    days: u32,
) -> Vec<crate::compress::tool_activation::ServerPruneOutcome> {
    use crate::compress::tool_activation::ServerPruneOutcome;
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let mut out = Vec::new();

    let total_sessions: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM tool_invocations WHERE ts >= ?1",
            params![cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut stmt = match conn.prepare(
        "SELECT server_prefix, COUNT(DISTINCT session_id)
         FROM tool_invocations
         WHERE ts >= ?1 AND server_prefix IS NOT NULL AND server_prefix != ''
         GROUP BY server_prefix",
    ) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let rows = stmt.query_map(params![cutoff], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    });
    if let Ok(rows) = rows {
        for (prefix, used_sessions) in rows.flatten() {
            let display = crate::profiles::mcp_prefix_to_server_display(&prefix);
            // The server's most recent prune time, if ever pruned. Sessions active after it are the
            // hidden arm: the server was denied in the menu while those sessions ran.
            let prune_ts: Option<String> = conn
                .query_row(
                    "SELECT MAX(ts) FROM profile_changes WHERE servers_removed LIKE '%' || ?1 || '%'",
                    params![display],
                    |r| r.get(0),
                )
                .ok()
                .flatten();
            let hidden_sessions: i64 = match &prune_ts {
                Some(ts) => conn
                    .query_row(
                        "SELECT COUNT(DISTINCT session_id) FROM tool_invocations WHERE ts > ?1",
                        params![ts],
                        |r| r.get(0),
                    )
                    .unwrap_or(0),
                None => 0,
            };
            let misses: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tool_misses WHERE server_prefix = ?1 AND ts >= ?2",
                    params![prefix, cutoff],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            out.push(ServerPruneOutcome {
                server: display,
                prefix,
                total_sessions,
                used_sessions,
                hidden_sessions,
                misses,
            });
        }
    }
    out
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

pub fn list_embedding_rows(conn: &Connection) -> Result<Vec<(i64, Vec<u8>)>> {
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
/// One observed MCP tool with invocation count in the lookback window.
#[derive(Debug, Clone)]
pub struct ObservedToolRow {
    pub tool_name: String,
    pub server_prefix: String,
    pub count: u64,
}

/// All MCP tools invoked since `cutoff` (RFC3339), ordered by count descending.
pub fn observed_tools(conn: &Connection, cutoff: &str) -> Result<Vec<ObservedToolRow>> {
    let mut stmt = conn.prepare(
        "SELECT tool_name, server_prefix, COUNT(*) AS c FROM tool_invocations \
         WHERE ts >= ?1 GROUP BY tool_name, server_prefix ORDER BY c DESC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![cutoff], |r| {
            Ok(ObservedToolRow {
                tool_name: r.get(0)?,
                server_prefix: r.get(1)?,
                count: r.get::<_, i64>(2)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Distinct MCP tool names observed since `cutoff`.
pub fn distinct_observed_tool_count(conn: &Connection, cutoff: &str) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT tool_name) FROM tool_invocations WHERE ts >= ?1",
        rusqlite::params![cutoff],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as usize)
}

/// Distinct tool names under a server prefix since `cutoff`.
pub fn tools_under_prefix(conn: &Connection, prefix: &str, cutoff: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tool_name FROM tool_invocations \
         WHERE ts >= ?1 AND server_prefix = ?2 ORDER BY tool_name",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![cutoff, prefix], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

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
            !invoked
                .iter()
                .any(|p| p.starts_with(&prefix) || prefix.starts_with(p.as_str()))
        })
        .collect();
    unused.sort();
    Ok(unused)
}

#[derive(Debug, Clone)]
pub struct AllowanceSnapshotRow {
    pub id: i64,
    pub ts: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub window: String,
    pub used_pct: f64,
    pub remaining_pct: Option<f64>,
    pub resets_at: Option<i64>,
    pub session_cost_usd: Option<f64>,
}

fn map_allowance_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AllowanceSnapshotRow> {
    Ok(AllowanceSnapshotRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        session_id: r.get(2)?,
        model: r.get(3)?,
        window: r.get(4)?,
        used_pct: r.get(5)?,
        remaining_pct: r.get(6)?,
        resets_at: r.get(7)?,
        session_cost_usd: r.get(8)?,
    })
}

/// Insert snapshot unless throttled (same window + used_pct within 60s).
pub fn insert_allowance_snapshot(
    conn: &Connection,
    ts: &str,
    session_id: Option<&str>,
    model: Option<&str>,
    window: &str,
    used_pct: f64,
    remaining_pct: Option<f64>,
    resets_at: Option<i64>,
    session_cost_usd: Option<f64>,
) -> Result<bool> {
    ensure_schema(conn)?;
    if let Ok((last_used, last_ts)) = conn.query_row(
        "SELECT used_pct, ts FROM allowance_snapshots WHERE window = ?1 ORDER BY id DESC LIMIT 1",
        params![window],
        |r| Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?)),
    ) {
        if (last_used - used_pct).abs() < 0.05 {
            if let Ok(last_dt) = last_ts.parse::<chrono::DateTime<chrono::Utc>>() {
                if let Ok(cur_dt) = ts.parse::<chrono::DateTime<chrono::Utc>>() {
                    if cur_dt.signed_duration_since(last_dt).num_seconds() < 60 {
                        return Ok(false);
                    }
                }
            }
        }
    }

    conn.execute(
        "INSERT INTO allowance_snapshots \
         (ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ts,
            session_id,
            model,
            window,
            used_pct,
            remaining_pct,
            resets_at,
            session_cost_usd
        ],
    )?;
    stamp_ctx_active_since(conn);
    Ok(true)
}

pub fn latest_allowance_snapshot(conn: &Connection, window: &str) -> Option<AllowanceSnapshotRow> {
    ensure_schema(conn).ok()?;
    conn.query_row(
        "SELECT id, ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd \
         FROM allowance_snapshots WHERE window = ?1 ORDER BY id DESC LIMIT 1",
        params![window],
        map_allowance_row,
    )
    .optional()
    .ok()
    .flatten()
}

pub fn load_allowance_snapshots(
    conn: &Connection,
    window: &str,
    since_iso: Option<&str>,
    until_iso: Option<&str>,
) -> Vec<AllowanceSnapshotRow> {
    ensure_schema(conn).ok();
    let sql = match (since_iso, until_iso) {
        (Some(_), Some(_)) => {
            "SELECT id, ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd \
             FROM allowance_snapshots WHERE window = ?1 AND ts >= ?2 AND ts <= ?3 ORDER BY ts ASC"
        }
        (Some(_), None) => {
            "SELECT id, ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd \
             FROM allowance_snapshots WHERE window = ?1 AND ts >= ?2 ORDER BY ts ASC"
        }
        (None, Some(_)) => {
            "SELECT id, ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd \
             FROM allowance_snapshots WHERE window = ?1 AND ts <= ?2 ORDER BY ts ASC"
        }
        (None, None) => {
            "SELECT id, ts, session_id, model, window, used_pct, remaining_pct, resets_at, session_cost_usd \
             FROM allowance_snapshots WHERE window = ?1 ORDER BY ts ASC"
        }
    };

    let Ok(mut stmt) = conn.prepare(sql) else {
        return vec![];
    };

    let rows = match (since_iso, until_iso) {
        (Some(s), Some(u)) => stmt.query_map(params![window, s, u], map_allowance_row),
        (Some(s), None) => stmt.query_map(params![window, s], map_allowance_row),
        (None, Some(u)) => stmt.query_map(params![window, u], map_allowance_row),
        (None, None) => stmt.query_map(params![window], map_allowance_row),
    };

    rows.map(|r| r.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod compress_decision_tests {
    use super::*;

    fn decision<'a>(
        ts: &'a str,
        session: &'a str,
        tool: &'a str,
        cmd: &'a str,
    ) -> CompressDecision<'a> {
        CompressDecision {
            ts,
            session_id: Some(session),
            tool_name: tool,
            server_prefix: None,
            kind: "read",
            task_mode: "scan",
            lines_total: 100,
            lines_keep: 60,
            lines_drop: 40,
            chars_in: 5000,
            would_chars_out: 2000,
            features_json: "{}",
            command_or_path: cmd,
            applied: false,
            explore_arm: None,
            surface: None,
        }
    }

    #[test]
    fn corpus_queries_exclude_ctx_self_dev_rows() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // A real user row and a ctx self-dev row, same tool, both joined with a reread signal.
        let mut user = decision("2026-05-31T10:01:00+00:00", "s-user", "Read", "user.rs");
        user.features_json = r#"{"repo_key":"/home/me/app"}"#;
        insert_compress_decision(&conn, &user).unwrap();
        let mut dev = decision("2026-05-31T10:02:00+00:00", "s-dev", "Read", "dev.rs");
        dev.features_json = r#"{"repo_key":"/home/me/ctx","self_dev":true}"#;
        insert_compress_decision(&conn, &dev).unwrap();
        conn.execute(
            "UPDATE compress_decisions
             SET outcome_joined=1, outcome_reread=1, surface='cursor', outcome_signals='[\"reread\"]'",
            [],
        )
        .unwrap();

        // Precision audit: the self-dev row is gone.
        let audit = signal_audit_rows(&conn, None, 100);
        assert_eq!(audit.len(), 1, "self-dev row must be excluded from the audit");
        assert_eq!(audit[0].command_or_path.as_deref(), Some("user.rs"));

        // Causal gate corpus: only the user row is counted as a baseline decision.
        let baseline_n: i64 = causal_tool_outcomes(&conn, Some("Read"))
            .iter()
            .map(|c| c.baseline_n)
            .sum();
        assert_eq!(baseline_n, 1, "self-dev row must be excluded from the gate");
    }

    #[test]
    fn tool_menu_bill_ranks_dead_weight_and_uses_catalog() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('bill-sess', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid = conn.last_insert_rowid();
        let now = chrono::Utc::now().to_rfc3339();

        // Linear: 3 distinct tools invoked (catalog 47), Notion: 2 distinct (catalog 19). All in-window.
        for tool in ["get_issue", "list_issues", "save_issue"] {
            insert_tool_invocation(
                &conn,
                sid,
                None,
                &format!("mcp__claude_ai_Linear__{tool}"),
                "mcp__claude_ai_Linear__",
                &now,
            )
            .unwrap();
        }
        for tool in ["notion-fetch", "notion-search"] {
            insert_tool_invocation(
                &conn,
                sid,
                None,
                &format!("mcp__claude_ai_Notion__{tool}"),
                "mcp__claude_ai_Notion__",
                &now,
            )
            .unwrap();
        }

        let bill = tool_menu_bill(&conn, 30);
        assert_eq!(bill.tokens_per_tool, 600);
        assert_eq!(bill.servers.len(), 2);

        // Linear carries the bigger catalog and more dead weight, so it ranks first.
        let linear = &bill.servers[0];
        assert_eq!(linear.server, "Linear");
        assert_eq!(linear.catalog_tools, 47);
        assert_eq!(linear.invoked_tools, 3);
        assert_eq!(linear.dead_tools, 44);
        assert_eq!(linear.carried_tokens, 47 * 600);
        assert_eq!(linear.dead_tokens, 44 * 600);

        let notion = &bill.servers[1];
        assert_eq!(notion.server, "Notion");
        assert_eq!(notion.dead_tools, 17);

        assert_eq!(bill.biggest_dead_server.as_deref(), Some("Linear"));
        assert_eq!(bill.total_carried_tokens, (47 + 19) * 600);
        assert_eq!(bill.total_invoked_tools, 5);
    }

    #[test]
    fn tool_menu_bill_floors_catalog_at_observed() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('bill-sess2', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid = conn.last_insert_rowid();
        let now = chrono::Utc::now().to_rfc3339();

        // A server not in SERVER_COUNTS (falls back to 3), but 5 distinct tools were observed:
        // the catalog must floor at what we have actually seen, so dead weight is never negative.
        for i in 0..5 {
            insert_tool_invocation(
                &conn,
                sid,
                None,
                &format!("mcp__claude_ai_Unknownco__tool_{i}"),
                "mcp__claude_ai_Unknownco__",
                &now,
            )
            .unwrap();
        }

        let bill = tool_menu_bill(&conn, 30);
        assert_eq!(bill.servers.len(), 1);
        let s = &bill.servers[0];
        assert_eq!(s.invoked_tools, 5);
        assert!(s.catalog_tools >= 5, "catalog floored at observed distinct");
        assert_eq!(s.dead_tools, s.catalog_tools - s.invoked_tools);
        assert!(s.dead_tools >= 0);
    }

    #[test]
    fn tool_miss_stats_counts_reaches_per_server_with_a_rate() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // Baseline: no reaches recorded yet, so the harm read is honestly zero.
        let empty = tool_miss_stats(&conn, 30);
        assert_eq!(empty.total_misses, 0);
        assert_eq!(empty.miss_rate, 0.0);

        // Two sessions of activity (the rate denominator), and two reaches for a pruned Canva tool.
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('s1', 'p', '2026-05-31')",
            [],
        )
        .unwrap();
        let sid = conn.last_insert_rowid();
        let now = chrono::Utc::now().to_rfc3339();
        for _ in 0..2 {
            insert_tool_invocation(&conn, sid, None, "mcp__claude_ai_Linear__get_issue", "mcp__claude_ai_Linear__", &now).unwrap();
        }
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('s2', 'p', '2026-05-31')",
            [],
        )
        .unwrap();
        let sid2 = conn.last_insert_rowid();
        insert_tool_invocation(&conn, sid2, None, "mcp__claude_ai_Notion__notion-fetch", "mcp__claude_ai_Notion__", &now).unwrap();

        insert_tool_miss(&conn, Some("s1"), "mcp__claude_ai_Canva__export-design", "mcp__claude_ai_Canva__", "prune", &now).unwrap();
        insert_tool_miss(&conn, Some("s1"), "mcp__claude_ai_Canva__get-design", "mcp__claude_ai_Canva__", "prune", &now).unwrap();

        let stats = tool_miss_stats(&conn, 30);
        assert_eq!(stats.total_misses, 2);
        assert_eq!(stats.servers.len(), 1);
        assert_eq!(stats.servers[0].server, "Canva");
        assert_eq!(stats.servers[0].misses, 2);
        assert_eq!(stats.sessions, 2, "distinct sessions with tool activity");
        assert_eq!(stats.miss_rate, 1.0, "2 misses over 2 active sessions");
    }

    #[test]
    fn join_labels_correction_and_marks_joined() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-x', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-x'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // Explicit complaint after a trimmed decision (CTX-48 gate).
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'user', '[\"correction\",\"correction_explicit\"]', '2026-05-31T10:05:00+00:00')",
            params![sid],
        )
        .unwrap();

        let mut d = decision("2026-05-31T10:01:00+00:00", "sess-x", "Read", "a.rs");
        d.applied = true;
        insert_compress_decision(&conn, &d).unwrap();

        let n = join_compress_outcomes(&conn).unwrap();
        assert_eq!(n, 1);
        let stats = compress_decision_stats(&conn);
        assert_eq!(stats.total, 1);
        assert_eq!(stats.joined, 1);

        let progress = compress_tool_progress(&conn);
        let read = progress.iter().find(|p| p.tool_name == "Read").unwrap();
        assert_eq!(read.joined, 1);
        assert_eq!(read.corrections, 1);
        assert_eq!(read.clean_runs, 0);
    }

    #[test]
    fn interrupt_after_trim_does_not_set_gate_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-int', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-int'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'user', '[\"aborted\"]', '2026-05-31T10:05:00+00:00')",
            params![sid],
        )
        .unwrap();

        let mut d = decision("2026-05-31T10:01:00+00:00", "sess-int", "Bash", "npm test");
        d.applied = true;
        insert_compress_decision(&conn, &d).unwrap();

        let n = join_compress_outcomes(&conn).unwrap();
        assert_eq!(n, 1);
        let corr: i64 = conn
            .query_row(
                "SELECT COALESCE(outcome_correction,0) FROM compress_decisions WHERE command_or_path='npm test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(corr, 0, "interrupts must not feed the causal gate");
    }

    #[test]
    fn terse_correction_without_explicit_does_not_set_gate_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-terse', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-terse'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'user', '[\"correction\",\"correction_terse\"]', '2026-05-31T10:05:00+00:00')",
            params![sid],
        )
        .unwrap();

        let mut d = decision("2026-05-31T10:01:00+00:00", "sess-terse", "Read", "a.rs");
        d.applied = true;
        insert_compress_decision(&conn, &d).unwrap();

        join_compress_outcomes(&conn).unwrap();
        let corr: i64 = conn
            .query_row(
                "SELECT COALESCE(outcome_correction,0) FROM compress_decisions WHERE command_or_path='a.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(corr, 0, "terse redirects are observational only");
    }

    #[test]
    fn session_steer_after_trim_does_not_set_gate_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-steer', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-steer'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, human_text_prefix, ts) VALUES (?1, 1, 'user', '[\"session_steer\"]', 'nope nope.. lets do the fun stuff', '2026-05-31T10:05:00+00:00')",
            params![sid],
        )
        .unwrap();

        let mut d = decision("2026-05-31T10:01:00+00:00", "sess-steer", "Bash", "figma metadata");
        d.applied = true;
        d.lines_drop = 166;
        insert_compress_decision(&conn, &d).unwrap();

        join_compress_outcomes(&conn).unwrap();
        let (corr, sigs): (i64, String) = conn
            .query_row(
                "SELECT COALESCE(outcome_correction,0), COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path='figma metadata'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(corr, 0, "session steers must not feed the causal gate");
        assert!(sigs.contains("session_steer"));
        assert!(!sigs.contains("correction_gate"));
    }

    #[test]
    fn exclude_edit_tools_fragment_covers_every_edit_tool_name() {
        // The trim-corpus exclusion must list exactly the shared edit-tool set, or an edit tool
        // could leak into training / the ladder. Pin them together (CTX-46 / ADR 0031).
        for name in crate::outcome_signals::EDIT_TOOL_NAMES {
            assert!(
                EXCLUDE_EDIT_TOOLS.contains(&format!("'{name}'")),
                "EXCLUDE_EDIT_TOOLS is missing {name}"
            );
        }
    }

    #[test]
    fn join_sets_edit_follow_for_same_file_edit_but_not_for_a_plain_reread() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-ef', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-ef'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // A plain turn past the window closes every decision so they score as final.
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'assistant', '', '2026-05-31T10:30:00+00:00')",
            params![sid],
        )
        .unwrap();

        // a.rs: read, then edited within the window -> edit-follow (and a re-read, since an edit is
        // a later same-path touch).
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:01:00+00:00", "sess-ef", "Read", "a.rs"),
        )
        .unwrap();
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:03:00+00:00", "sess-ef", "Edit", "a.rs"),
        )
        .unwrap();
        // b.rs: read, then read again within the window -> re-read only, never edit-follow.
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:01:00+00:00", "sess-ef", "Read", "b.rs"),
        )
        .unwrap();
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:04:00+00:00", "sess-ef", "Read", "b.rs"),
        )
        .unwrap();

        join_compress_outcomes(&conn).unwrap();

        let label = |path: &str| -> (i64, i64) {
            conn.query_row(
                "SELECT COALESCE(outcome_edit_follow,0), COALESCE(outcome_reread,0)
                 FROM compress_decisions
                 WHERE command_or_path = ?1 AND tool_name = 'Read'",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };

        let (a_edit, a_reread) = label("a.rs");
        assert_eq!(a_edit, 1, "a.rs read was followed by an edit of the same file");
        assert_eq!(a_reread, 1, "an edit is also a later same-path touch");

        let (b_edit, b_reread) = label("b.rs");
        assert_eq!(b_edit, 0, "b.rs was only re-read, never edited");
        assert_eq!(b_reread, 1, "b.rs was read again within the window");
    }

    #[test]
    fn edit_follow_requires_same_region_when_ranges_are_known() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-region', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-region'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'assistant', '', '2026-05-31T10:30:00+00:00')",
            params![sid],
        )
        .unwrap();

        // An edit decision that records what it wrote and sought (the content anchors).
        let edit_at = |ts: &str, path: &str, wrote: &str, sought: &str| {
            let features = format!("{{\"edit_wrote\":\"{wrote}\",\"edit_sought\":\"{sought}\"}}");
            let row = CompressDecision {
                kind: "edit",
                tool_name: "Edit",
                features_json: &features,
                command_or_path: path,
                applied: true,
                lines_drop: 40,
                ..decision(ts, "sess-region", "Edit", path)
            };
            insert_compress_decision(&conn, &row).unwrap();
        };

        // big.rs: first edit writes one region, the follow-up seeks a different line -> not a re-edit.
        edit_at("2026-05-31T10:01:00+00:00", "big.rs", "let alpha = compute_one()", "let alpha = old_one()");
        edit_at("2026-05-31T10:03:00+00:00", "big.rs", "return other_helper(z)", "call other_helper()");
        // small.rs: the follow-up seeks the exact text the first edit wrote -> a real same-region redo.
        edit_at("2026-05-31T10:01:00+00:00", "small.rs", "let beta = compute_two()", "let beta = old_two()");
        edit_at("2026-05-31T10:03:00+00:00", "small.rs", "let beta = fixed_two()", "let beta = compute_two()");

        join_compress_outcomes(&conn).unwrap();

        let first_edit_follow = |path: &str| -> i64 {
            conn.query_row(
                "SELECT COALESCE(outcome_edit_follow,0) FROM compress_decisions
                 WHERE command_or_path = ?1 AND ts = '2026-05-31T10:01:00+00:00'",
                params![path],
                |r| r.get(0),
            )
            .unwrap()
        };

        assert_eq!(
            first_edit_follow("big.rs"),
            0,
            "an edit elsewhere in the same file is normal multi-part work, not a re-edit"
        );
        assert_eq!(
            first_edit_follow("small.rs"),
            1,
            "a follow-up that edits the exact text just written is a real same-region re-edit"
        );
    }

    #[test]
    fn join_skips_legacy_todowrite_routine_followups() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-todo', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-todo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'assistant', '', '2026-05-31T10:30:00+00:00')",
            params![sid],
        )
        .unwrap();

        let mut first = decision(
            "2026-05-31T10:01:00+00:00",
            "sess-todo",
            "TodoWrite",
            "TodoWrite",
        );
        first.kind = "generic";
        insert_compress_decision(&conn, &first).unwrap();
        let mut second = decision(
            "2026-05-31T10:05:00+00:00",
            "sess-todo",
            "TodoWrite",
            "TodoWrite",
        );
        second.kind = "generic";
        insert_compress_decision(&conn, &second).unwrap();

        join_compress_outcomes(&conn).unwrap();

        let reread: i64 = conn
            .query_row(
                "SELECT COALESCE(outcome_reread,0) FROM compress_decisions
                 WHERE session_id = 'sess-todo' AND ts = '2026-05-31T10:01:00+00:00'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reread, 0, "routine TodoWrite follow-ups must not count as re-reads");
    }

    #[test]
    fn join_counts_todowrite_reread_only_on_identical_payload() {
        use serde_json::json;
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-todo2', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-todo2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'assistant', '', '2026-05-31T10:30:00+00:00')",
            params![sid],
        )
        .unwrap();

        let payload = json!({"merge": true, "todos": [{"id": "a", "content": "x", "status": "pending"}]});
        let fp = crate::surface::fingerprint_tool_input("TodoWrite", &payload);
        let mut first = decision("2026-05-31T10:01:00+00:00", "sess-todo2", "TodoWrite", &fp);
        first.kind = "generic";
        insert_compress_decision(&conn, &first).unwrap();
        let other = json!({"merge": true, "todos": [{"id": "b", "content": "y", "status": "pending"}]});
        let fp2 = crate::surface::fingerprint_tool_input("TodoWrite", &other);
        let mut second = decision("2026-05-31T10:05:00+00:00", "sess-todo2", "TodoWrite", &fp2);
        second.kind = "generic";
        insert_compress_decision(&conn, &second).unwrap();

        join_compress_outcomes(&conn).unwrap();

        let reread_diff: i64 = conn
            .query_row(
                "SELECT COALESCE(outcome_reread,0) FROM compress_decisions
                 WHERE session_id = 'sess-todo2' AND ts = '2026-05-31T10:01:00+00:00'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reread_diff, 0);

        let mut third = decision("2026-05-31T10:08:00+00:00", "sess-todo2", "TodoWrite", &fp);
        third.kind = "generic";
        insert_compress_decision(&conn, &third).unwrap();
        conn.execute(
            "UPDATE compress_decisions SET outcome_joined = 0, outcome_reread = NULL
             WHERE session_id = 'sess-todo2'",
            [],
        )
        .unwrap();
        join_compress_outcomes(&conn).unwrap();

        let reread_same: i64 = conn
            .query_row(
                "SELECT COALESCE(outcome_reread,0) FROM compress_decisions
                 WHERE session_id = 'sess-todo2' AND ts = '2026-05-31T10:01:00+00:00'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reread_same, 1, "identical todo payload should count as a re-read");
    }

    #[test]
    fn clean_run_when_no_later_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES ('proj-sess-y', 'p', '2026-05-31T10:00:00+00:00')",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='proj-sess-y'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // A non-correction turn past the correction window closes it and confirms a
        // clean run (a turn inside the window would not be enough: the window must close).
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, 1, 'assistant', '', '2026-05-31T10:20:00+00:00')",
            params![sid],
        )
        .unwrap();

        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:01:00+00:00", "sess-y", "Grep", "pat"),
        )
        .unwrap();
        let n = join_compress_outcomes(&conn).unwrap();
        assert_eq!(n, 1);
        let progress = compress_tool_progress(&conn);
        let grep = progress.iter().find(|p| p.tool_name == "Grep").unwrap();
        assert_eq!(grep.corrections, 0);
        assert_eq!(grep.clean_runs, 1);
    }

    #[test]
    fn decisions_by_day_buckets_and_orders_oldest_first() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // Two decisions on the earlier day, one on the later day.
        insert_compress_decision(
            &conn,
            &decision("2026-05-30T09:00:00+00:00", "s1", "Read", "a.rs"),
        )
        .unwrap();
        insert_compress_decision(
            &conn,
            &decision("2026-05-30T14:00:00+00:00", "s1", "Read", "b.rs"),
        )
        .unwrap();
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:00:00+00:00", "s1", "Read", "c.rs"),
        )
        .unwrap();

        let by_day = decisions_by_day(&conn, 14);
        assert_eq!(by_day.len(), 2);
        // Oldest first.
        assert_eq!(by_day[0].day, "2026-05-30");
        assert_eq!(by_day[0].total, 2);
        assert_eq!(by_day[1].day, "2026-05-31");
        assert_eq!(by_day[1].total, 1);
        // Nothing has joined to an outcome (no turns inserted), reported honestly as zero.
        assert_eq!(by_day[0].joined, 0);
        assert_eq!(by_day[1].joined, 0);
    }

    #[test]
    fn unjoined_until_downstream_evidence() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // No session/turn rows yet: decision must stay unjoined (never scored clean prematurely).
        insert_compress_decision(
            &conn,
            &decision("2026-05-31T10:01:00+00:00", "sess-z", "Read", "a.rs"),
        )
        .unwrap();
        let n = join_compress_outcomes(&conn).unwrap();
        assert_eq!(n, 0);
        let stats = compress_decision_stats(&conn);
        assert_eq!(stats.joined, 0);
    }

    #[test]
    fn explore_tool_outcomes_separates_arms_and_ignores_non_experiment_rows() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        let row = |arm: Option<&'static str>, applied: bool| CompressDecision {
            ts: "2026-05-31T10:01:00+00:00",
            session_id: Some("s"),
            tool_name: "Read",
            server_prefix: None,
            kind: "read",
            task_mode: "scan",
            lines_total: 100,
            lines_keep: 60,
            lines_drop: 40,
            chars_in: 5000,
            would_chars_out: 2000,
            features_json: "{}",
            command_or_path: "a.rs",
            applied,
            explore_arm: arm,
            surface: None,
        };
        // 2 treatment, 2 control, and 1 ordinary (non-experiment) row that must be excluded even
        // though it is the same tool and applied=0 like a control.
        insert_compress_decision(&conn, &row(Some("treatment"), true)).unwrap();
        insert_compress_decision(&conn, &row(Some("treatment"), true)).unwrap();
        insert_compress_decision(&conn, &row(Some("control"), false)).unwrap();
        insert_compress_decision(&conn, &row(Some("control"), false)).unwrap();
        insert_compress_decision(&conn, &row(None, false)).unwrap();

        // Everything judged; one treatment row precedes a correction.
        conn.execute("UPDATE compress_decisions SET outcome_joined=1", [])
            .unwrap();
        conn.execute(
            "UPDATE compress_decisions SET outcome_correction=1
             WHERE id=(SELECT MIN(id) FROM compress_decisions WHERE explore_arm='treatment')",
            [],
        )
        .unwrap();

        let out = explore_tool_outcomes(&conn, None);
        let read = out.iter().find(|e| e.tool_name == "Read").unwrap();
        assert_eq!(read.treatment_collected, 2);
        assert_eq!(read.control_collected, 2);
        assert_eq!(read.treatment_n, 2);
        assert_eq!(read.control_n, 2);
        assert_eq!(read.treatment_corrections, 1, "one treatment correction");
        assert_eq!(read.control_corrections, 0, "control had no corrections");
    }

    fn seed_session(conn: &Connection, key: &str) -> i64 {
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at) VALUES (?1, 'p', '2026-05-31T10:00:00+00:00')",
            params![key],
        )
        .unwrap();
        conn.query_row(
            "SELECT id FROM sessions WHERE external_key=?1",
            params![key],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn seed_turn(conn: &Connection, sid: i64, idx: i64, flags: &str) {
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, flags, ts) VALUES (?1, ?2, 'turn', ?3, NULL)",
            params![sid, idx, flags],
        )
        .unwrap();
    }

    #[test]
    fn compaction_unknown_when_no_turns_persisted() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        let out = compaction_followups(&conn);
        let claude = out.iter().find(|s| s.surface == "claude-code").unwrap();
        // No turns at all means we have not seen sessions, which is unknown, not "zero
        // compactions". Zero would falsely imply we looked and found a clean result.
        assert_eq!(claude.confidence, "unknown");
        assert_eq!(claude.compaction_events, None);
        // Surfaces we cannot see are always unknown, never fabricated as zero.
        let cursor = out.iter().find(|s| s.surface == "cursor").unwrap();
        assert_eq!(cursor.confidence, "unknown");
        assert_eq!(cursor.followed_by_correction, None);
    }

    #[test]
    fn compaction_counts_correction_inside_window_only() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // Session A: a compaction at turn 2, a correction at turn 4 (inside the 5-turn window).
        let a = seed_session(&conn, "sess-a");
        seed_turn(&conn, a, 0, "[]");
        seed_turn(&conn, a, 1, "[]");
        seed_turn(&conn, a, 2, r#"["pre_compact"]"#);
        seed_turn(&conn, a, 3, "[]");
        seed_turn(&conn, a, 4, r#"["correction","correction_explicit"]"#);

        // Session B: a compaction at turn 1, a correction far away at turn 9 (outside window).
        let b = seed_session(&conn, "sess-b");
        seed_turn(&conn, b, 0, "[]");
        seed_turn(&conn, b, 1, r#"["pre_compact"]"#);
        for idx in 2..9 {
            seed_turn(&conn, b, idx, "[]");
        }
        seed_turn(&conn, b, 9, r#"["correction"]"#);

        let out = compaction_followups(&conn);
        let claude = out.iter().find(|s| s.surface == "claude-code").unwrap();
        assert_eq!(claude.confidence, "observed");
        assert_eq!(claude.compaction_events, Some(2), "two compaction events seen");
        assert_eq!(
            claude.followed_by_correction,
            Some(1),
            "only session A's correction is inside the window"
        );
        assert_eq!(claude.sessions_with_compaction, Some(2));
        assert_eq!(claude.window_turns, COMPACTION_FOLLOWUP_WINDOW_TURNS);
    }

    #[test]
    fn compaction_observed_zero_when_sessions_never_compact() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // Real sessions with turns but no compaction: this is an honest "observed, none",
        // distinct from "unknown". A correction with no compaction before it is not counted.
        let s = seed_session(&conn, "sess-clean");
        seed_turn(&conn, s, 0, "[]");
        seed_turn(&conn, s, 1, r#"["correction"]"#);

        let out = compaction_followups(&conn);
        let claude = out.iter().find(|s| s.surface == "claude-code").unwrap();
        assert_eq!(claude.confidence, "observed");
        assert_eq!(claude.compaction_events, Some(0));
        assert_eq!(claude.followed_by_correction, Some(0));
        assert_eq!(claude.sessions_with_compaction, Some(0));
    }

    #[test]
    fn cursor_compaction_observed_low_when_event_recorded() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        insert_cursor_compaction(
            &conn,
            &CursorCompaction {
                ts: "2026-06-14T10:00:00Z".to_string(),
                session_id: Some("conv-1".to_string()),
                trigger: Some("auto".to_string()),
                context_usage_percent: Some(92.0),
                message_count: Some(120),
                ..Default::default()
            },
        )
        .unwrap();

        let out = compaction_followups(&conn);
        let cursor = out.iter().find(|s| s.surface == "cursor").unwrap();
        // We saw the compaction live, so it's a real count, but lower confidence: the correction
        // follow-up isn't computed yet, so it must be None (not a fake 0), never "observed".
        assert_eq!(cursor.confidence, "observed_low");
        assert_eq!(cursor.compaction_events, Some(1));
        assert_eq!(cursor.followed_by_correction, None);
        assert_eq!(cursor.sessions_with_compaction, Some(1));
    }

    #[test]
    fn cursor_compaction_observed_low_zero_once_cursor_seen() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // A Cursor decision exists but no compaction yet: this is an honest "none seen yet" at
        // lower confidence, parallel to Claude's observed-zero, not "not visible yet".
        insert_compress_decision(
            &conn,
            &CompressDecision {
                ts: "2026-06-14T10:00:00Z",
                session_id: Some("conv-1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "all",
                lines_total: 10,
                lines_keep: 10,
                lines_drop: 0,
                chars_in: 100,
                would_chars_out: 100,
                features_json: "{}",
                command_or_path: "",
                applied: false,
                explore_arm: None,
                surface: Some("cursor"),
            },
        )
        .unwrap();

        let out = compaction_followups(&conn);
        let cursor = out.iter().find(|s| s.surface == "cursor").unwrap();
        assert_eq!(cursor.confidence, "observed_low");
        assert_eq!(cursor.compaction_events, Some(0));
        assert_eq!(cursor.followed_by_correction, None);
    }

    #[test]
    fn surface_summary_splits_by_provenance_with_honest_empty_state() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // A NULL-surface (legacy Claude) row that ctx acted on, and a live cursor row it observed.
        let mut claude = decision("2026-06-14T10:00:00+00:00", "c1", "Read", "/a.rs");
        claude.applied = true; // chars_in 5000, would_chars_out 2000 -> 3000 saved
        insert_compress_decision(&conn, &claude).unwrap();

        let mut cursor = decision("2026-06-14T11:00:00+00:00", "u1", "Grep", "fn main");
        cursor.surface = Some("cursor");
        cursor.applied = false; // observe-only
        insert_compress_decision(&conn, &cursor).unwrap();

        let out = surface_summary(&conn);
        assert_eq!(out.len(), 2, "always reports both known surfaces");
        let cc = out.iter().find(|s| s.surface == "claude-code").unwrap();
        assert!(cc.seen);
        assert_eq!(cc.decisions, 1);
        assert_eq!(cc.acted, 1);
        assert_eq!(cc.chars_saved, 3000);

        let cu = out.iter().find(|s| s.surface == "cursor").unwrap();
        assert!(cu.seen);
        assert_eq!(cu.decisions, 1);
        assert_eq!(cu.acted, 0);
        assert_eq!(cu.observed, 1);
    }

    #[test]
    fn surface_summary_unseen_surface_is_not_fabricated() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();
        // Empty DB: both surfaces present, both unseen, zero counts, no last_seen.
        let out = surface_summary(&conn);
        assert_eq!(out.len(), 2);
        for s in &out {
            assert!(!s.seen, "{} should be unseen on an empty db", s.surface);
            assert_eq!(s.decisions, 0);
            assert!(s.last_seen.is_none());
        }
    }

    #[test]
    fn tool_attribution_aggregates_suspects_never_counts_shadow() {
        // Attribution must only look at applied trims that dropped lines, and count a trim as a
        // suspect when the agent worked around it, re-read the source, or re-expanded it. A shadow
        // decision (applied=0) is never a suspect, and a trim with two signals counts once (CTX-54).
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        let trim = |cmd: &'static str, applied: bool| CompressDecision::<'static> {
            ts: "2026-06-14T00:00:00+00:00",
            session_id: Some("s1"),
            tool_name: "Bash",
            server_prefix: None,
            kind: "generic",
            task_mode: "scan",
            lines_total: 100,
            lines_keep: 60,
            lines_drop: 40,
            chars_in: 5000,
            would_chars_out: 2000,
            features_json: "{}",
            command_or_path: cmd,
            applied,
            explore_arm: None,
            surface: Some("claude-code"),
        };
        // Applied trim with a workaround AND a reread (should count once as a suspect).
        insert_compress_decision(&conn, &trim("cmd-a", true)).unwrap();
        // Applied trim, no suspect signal.
        insert_compress_decision(&conn, &trim("cmd-b", true)).unwrap();
        // Shadow decision with a suspect-looking signal: must be ignored (not applied).
        insert_compress_decision(&conn, &trim("cmd-c", false)).unwrap();
        conn.execute(
            "UPDATE compress_decisions SET outcome_signals='[\"compression_workaround\",\"reread\"]' WHERE command_or_path='cmd-a'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE compress_decisions SET outcome_signals='[\"compression_workaround\"]' WHERE command_or_path='cmd-c'",
            [],
        )
        .unwrap();

        let out = tool_attribution(&conn);
        let bash = out.iter().find(|t| t.tool == "Bash").expect("bash row");
        assert_eq!(bash.applied_trims, 2, "shadow decision must not count");
        assert_eq!(bash.workaround, 1);
        assert_eq!(bash.reread, 1);
        assert_eq!(bash.suspect, 1, "two signals on one trim count once");
        assert!((bash.suspect_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn tool_attribution_counts_reexpand_via_rewind() {
        // The re-expand event (rewind_store.expanded_at) is the closest-to-causal suspect: the agent
        // asked for exactly the dropped block back. It must count even with no other signal.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        insert_compress_decision(
            &conn,
            &CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("s1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "scan",
                lines_total: 200,
                lines_keep: 40,
                lines_drop: 160,
                chars_in: 8000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "/big.rs",
                applied: true,
                explore_arm: None,
                surface: Some("claude-code"),
            },
        )
        .unwrap();
        insert_rewind(&conn, "rw1", "2026-06-14T00:00:00+00:00", Some("s1"), "Read", "/big.rs", "full original", "trimmed");
        link_decision_rewind(&conn, Some("s1"), "Read", "rw1");
        mark_rewind_expanded(&conn, "rw1");

        let out = tool_attribution(&conn);
        let read = out.iter().find(|t| t.tool == "Read").expect("read row");
        assert_eq!(read.applied_trims, 1);
        assert_eq!(read.reexpanded, 1);
        assert_eq!(read.suspect, 1);
    }

    #[test]
    fn weekly_net_ahead_is_fail_closed_on_unconfirmed_safety() {
        // A week that reclaimed plenty but has too few scored trims to confirm safety is NOT
        // net-ahead: safety must be measured, never assumed (CTX-63).
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        // One big applied trim: ~250K chars saved (well over the 50K-token bar), but only 1 scored
        // trim, so safety is unconfirmed.
        insert_compress_decision(
            &conn,
            &CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("s1"),
                tool_name: "Bash",
                server_prefix: None,
                kind: "generic",
                task_mode: "scan",
                lines_total: 100,
                lines_keep: 20,
                lines_drop: 80,
                chars_in: 1_000_000,
                would_chars_out: 4_000,
                features_json: "{}",
                command_or_path: "big",
                applied: true,
                explore_arm: None,
                surface: Some("claude-code"),
            },
        )
        .unwrap();
        conn.execute("UPDATE compress_decisions SET outcome_joined=1", []).unwrap();

        let wk = weekly_net_ahead(&conn);
        let w = wk.first().expect("one week");
        assert!(w.reclaimed_tokens > 50_000, "reclaimed well over the bar");
        assert!(w.reclaim_ok);
        assert!(w.harm_unconfirmed, "1 scored trim cannot confirm safety");
        assert!(!w.net_ahead, "fail-closed: no net-ahead without confirmed safety");
    }

    #[test]
    fn surface_summary_full_folds_in_watched_cursor_transcripts() {
        // No hook decisions for Cursor, but a real transcript on disk: the full summary must mark
        // Cursor seen via transcripts with genuine session/tool-call counts (CTX-53), while Claude
        // Code stays hook-provenanced.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        insert_compress_decision(
            &conn,
            &CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("cc-1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "scan",
                lines_total: 100,
                lines_keep: 60,
                lines_drop: 40,
                chars_in: 5000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "/a.rs",
                applied: true,
                explore_arm: None,
                surface: Some("claude-code"),
            },
        )
        .unwrap();

        let proj = tmp
            .path()
            .join(".cursor/projects/Users-me-Projects-ctx/agent-transcripts/sess-a");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess-a.jsonl");
        let lines = [
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>build a cursor adapter and walk me through the tradeoffs carefully</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Reading the adapter boundary before proposing anything, so the plan lines up with the real ingest path."},{"type":"tool_use","name":"Read","input":{"path":"/x.rs"}}]}}"#,
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>no that's wrong, revert it</user_query>"}]}}"#,
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();

        let out = surface_summary_full(&conn, tmp.path());
        let cc = out.iter().find(|s| s.surface == "claude-code").unwrap();
        assert_eq!(cc.observed_via, "hook");
        assert_eq!(cc.sessions_seen, 0);

        let cu = out.iter().find(|s| s.surface == "cursor").unwrap();
        assert!(cu.seen);
        assert_eq!(cu.decisions, 0);
        assert_eq!(cu.observed_via, "transcript");
        assert_eq!(cu.sessions_seen, 1);
        assert_eq!(cu.tool_calls_seen, 1);
        assert_eq!(cu.transcript_corrections, 1);
        assert!(cu.last_seen.is_some());
    }
}

#[cfg(test)]
mod compress_attach_tests {
    use super::*;

    #[test]
    fn compress_savings_attach_to_hook_turn_window() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO hook_traces (ts, session_id, working_directory, profile, enriched)
             VALUES ('2026-05-31T10:00:00+00:00', 'sess-a', '/proj', 'all', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO hook_traces (ts, session_id, working_directory, profile, enriched)
             VALUES ('2026-05-31T10:05:00+00:00', 'sess-a', '/proj', 'all', 1)",
            [],
        )
        .unwrap();
        insert_compress_event(
            &conn,
            "2026-05-31T10:01:00+00:00",
            Some("sess-a"),
            "Read",
            "read",
            5000,
            800,
            "src/lib.rs",
        )
        .unwrap();
        insert_compress_event(
            &conn,
            "2026-05-31T10:06:00+00:00",
            Some("sess-a"),
            "Grep",
            "grep",
            4000,
            900,
            "pattern",
        )
        .unwrap();
        backfill_hook_trace_compress(&conn).unwrap();

        let rows = load_hook_traces(&conn, 10, 0, None).unwrap();
        assert_eq!(rows.len(), 2);
        let by_ts: std::collections::HashMap<_, _> =
            rows.iter().map(|r| (r.ts.as_str(), r)).collect();
        let first = by_ts["2026-05-31T10:05:00+00:00"];
        let second = by_ts["2026-05-31T10:00:00+00:00"];
        assert_eq!(first.compress_chars_saved, 4000 - 900);
        assert_eq!(first.compress_event_count, 1);
        assert_eq!(second.compress_chars_saved, 5000 - 800);
        assert_eq!(second.compress_event_count, 1);
    }

    #[test]
    fn backfill_path_role_tags_historical_reads_from_command_or_path() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();

        insert_compress_decision(
            &conn,
            &CompressDecision {
                ts: "2026-06-19T10:00:00+00:00",
                session_id: Some("s1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "default",
                lines_total: 20,
                lines_keep: 10,
                lines_drop: 10,
                chars_in: 100,
                would_chars_out: 50,
                features_json: r#"{"repo_key":"/proj"}"#,
                command_or_path: "src/main.rs",
                applied: false,
                explore_arm: None,
                surface: Some("claude-code"),
            },
        )
        .unwrap();

        conn.execute(
            "DELETE FROM meta WHERE k='path_role_backfill_v2'",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();

        let role: Option<String> = conn
            .query_row(
                "SELECT json_extract(features_json,'$.path_role') FROM compress_decisions",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(role.as_deref(), Some("src"));
    }
}

#[cfg(test)]
mod watermark_tests {
    use super::*;

    #[test]
    fn stale_install_watermark_aligns_to_earliest_session() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = open_db().unwrap();
        ensure_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO meta (k, v) VALUES ('ctx_active_since', '2026-05-31T17:00:00+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
             VALUES ('s1', 'p', '2026-05-28T10:00:00+00:00', 'all', '/tmp', 5)",
            [],
        )
        .unwrap();
        assert!(maybe_reset_stale_install_watermark(&conn).unwrap());
        assert_eq!(
            get_ctx_active_since(&conn).as_deref(),
            Some("2026-05-28T10:00:00+00:00")
        );
    }
}
