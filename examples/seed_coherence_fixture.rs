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
        // Eligible reads with applied trims: real reclaimed output to reconcile on Home vs See.
        dec("Read", "read", 20, 6000, 2000, true, None),
        dec("Edit", "edit", 14, 4000, 1500, true, None),
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
            total += 1;
        }
    }

    println!(
        "seeded {total} compress_decisions into {}",
        ctx::config::db_path().display()
    );
    Ok(())
}
