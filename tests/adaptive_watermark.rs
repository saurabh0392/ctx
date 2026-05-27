//! Adaptive prefix file write and install watermark meta (uses `CTX_HOME`).

mod harness;

use ctx::adaptive::regenerate_adaptive_prefix_file;
use ctx::config;
use ctx::db;
use harness::CtxHarness;
use serial_test::serial;

#[test]
#[serial]
fn reset_ctx_active_since_clears_meta() {
    let _h = CtxHarness::new();
    let conn = db::open_db().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('ctx_active_since', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    assert!(db::get_ctx_active_since(&conn).is_some());
    db::reset_ctx_active_since(&conn).unwrap();
    assert!(db::get_ctx_active_since(&conn).is_none());
}

#[test]
#[serial]
fn regenerate_adaptive_prefix_writes_file() {
    let _h = CtxHarness::new();
    regenerate_adaptive_prefix_file().unwrap();
    let p = config::adaptive_prefix_path();
    assert!(p.is_file());
    let s = std::fs::read_to_string(&p).unwrap();
    assert!(
        s.contains("ctx adaptive") || s.contains("Not enough indexed"),
        "unexpected adaptive file: {s:?}"
    );
}
