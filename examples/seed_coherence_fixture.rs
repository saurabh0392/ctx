//! Seed a deterministic ctx fixture (db + config) for the behavioral coherence suite in CI, where
//! there is no real `~/.ctx` to clone. It writes into `$CTX_HOME` using ctx's own schema and insert
//! helpers, so the fixture can never drift from the real column layout.
//!
//!   CTX_HOME=/tmp/fixture cargo run --release --example seed_coherence_fixture
//!
//! The shape is chosen to exercise the invariants: held tools (a built-in one-shot and an MCP write),
//! eligible tools (built-in reads and an MCP read) with enough calls to render a trial control, some
//! applied trims so there is reclaimed output to reconcile, and a clean-test control-arm row.

use ctx::config::Config;
use ctx::db::{self, CompressDecision};

fn dec<'a>(
    tool: &'a str,
    kind: &'a str,
    n: usize,
    chars_in: usize,
    would_out: usize,
    applied: bool,
    explore_arm: Option<&'a str>,
) -> Vec<CompressDecision<'a>> {
    // A batch of `n` identical decisions for one tool. lines follow chars roughly (50 lines/result).
    let drop = if would_out < chars_in { 10 } else { 0 };
    (0..n)
        .map(|_| CompressDecision {
            ts: "2026-07-05T00:00:00Z",
            session_id: Some("fixture"),
            tool_name: tool,
            server_prefix: None,
            kind,
            task_mode: "code",
            lines_total: 50,
            lines_keep: 50 - drop,
            lines_drop: drop,
            chars_in,
            would_chars_out: would_out,
            features_json: "{}",
            command_or_path: "fixture/path",
            applied,
            explore_arm,
            surface: None,
        })
        .collect()
}

fn main() -> anyhow::Result<()> {
    // Config: the deny-set (held tools) only holds mutations under trim_all, so the fixture must set it.
    let mut cfg = Config::load();
    cfg.compress_trim_all = true;
    // A live trial with already-collected treatment rows reproduces CTX-74: after the backend
    // removes this exact config entry, historical evidence must not keep rendering it as live.
    cfg.compress_trial_tools = vec!["Edit".into()];
    cfg.save()?;

    let conn = db::open_db()?;
    db::ensure_schema(&conn)?;

    // Held (deny-set): a built-in one-shot, another one-shot, and an MCP write. Each has reclaimable
    // output (would_out < chars_in) so the reclaimable-excludes-held check has something to catch.
    let batches: Vec<Vec<CompressDecision>> = vec![
        dec("TodoWrite", "generic", 6, 2000, 1700, false, None),
        dec("TaskOutput", "generic", 3, 1500, 1400, false, None),
        dec(
            "mcp__acme_Widgets__save_thing",
            "mcp",
            4,
            3000,
            2600,
            false,
            None,
        ),
        // Codex app tools carry an extra MCP name segment. Include one eligible read and one held
        // mutation so the dashboard cannot collapse both into the same display name and leak the
        // eligible read into the held section (or present the mutation as trimmable on See).
        dec(
            "mcp__codex_apps__notion__fetch",
            "mcp",
            3,
            2400,
            1600,
            false,
            None,
        ),
        dec(
            "mcp__codex_apps__notion__notion_update_page",
            "mcp",
            3,
            2400,
            1900,
            false,
            None,
        ),
        // Eligible reads with applied trims: real reclaimed output to reconcile on Home vs See.
        dec("Read", "read", 20, 6000, 2000, true, None),
        dec("Edit", "edit", 14, 4000, 1500, true, Some("treatment")),
        // One clean-test control holdout on an eligible tool (applied=false, explore_arm=control).
        dec("Read", "read", 4, 6000, 2000, false, Some("control")),
        // Eligible tools with enough calls but no trims yet, so they sit in Watching with a trial control.
        dec("Bash", "bash", 12, 1200, 900, false, None),
        dec(
            "mcp__acme_Widgets__get_thing",
            "mcp",
            12,
            2500,
            1600,
            false,
            None,
        ),
    ];

    let mut total = 0;
    for batch in &batches {
        for d in batch {
            db::insert_compress_decision(&conn, d)?;
            if d.applied {
                // An applied proposal is not proof that shortened output reached the model. Seed
                // the same retained text + exact emitted-size receipt the live adapters write, so
                // coherence exercises the shipped savings semantics instead of shadow estimates.
                let decision_id = conn.last_insert_rowid();
                let rewind_id = format!("fixture-{decision_id}");
                let original = "x".repeat(d.chars_in);
                let trimmed = "y".repeat(d.would_chars_out);
                db::insert_rewind_checked(
                    &conn,
                    &rewind_id,
                    d.ts,
                    d.session_id,
                    d.tool_name,
                    d.command_or_path,
                    &original,
                    &trimmed,
                )?;
                db::mark_decision_emitted(&conn, decision_id, &rewind_id, trimmed.chars().count())?;
            }
            total += 1;
        }
    }

    println!(
        "seeded {total} compress_decisions into {}",
        ctx::config::db_path().display()
    );
    Ok(())
}
