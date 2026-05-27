//! Five lightweight "journey" checks: fresh DB, adaptive content, correction signal, profile config, watermark reset.

mod harness;

use ctx::adaptive::generate_adaptive_prefix;
use ctx::config::Config;
use ctx::db;
use harness::CtxHarness;
use serial_test::serial;

#[test]
#[serial]
fn journey_fresh_install_has_no_watermark_meta() {
    let _h = CtxHarness::new();
    let conn = db::open_db().unwrap();
    assert!(db::get_ctx_active_since(&conn).is_none());
}

#[test]
#[serial]
fn journey_adaptive_prefix_picks_up_seeded_usage() {
    let h = CtxHarness::new();
    h.seed_session_tool_and_correction();
    let conn = h.open();
    let md = generate_adaptive_prefix(&conn, 2000);
    assert!(
        md.to_lowercase().contains("python") || md.contains("Slack") || md.contains("typescript"),
        "expected signals in prefix: {md:?}"
    );
}

#[test]
#[serial]
fn journey_correction_turn_surfaces_in_prefix() {
    let h = CtxHarness::new();
    h.seed_session_tool_and_correction();
    let conn = h.open();
    let md = generate_adaptive_prefix(&conn, 2000);
    assert!(
        md.contains("Correction") || md.contains("correction"),
        "expected correction section: {md:?}"
    );
}

#[test]
#[serial]
fn journey_profile_switch_persists_in_config() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "minimal"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false
"#,
    );
    let cfg = Config::load();
    assert_eq!(cfg.active_profile.as_deref(), Some("minimal"));
}

#[test]
#[serial]
fn journey_watermark_reset_roundtrip() {
    let h = CtxHarness::new();
    let conn = h.open();
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('ctx_active_since', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    assert!(db::get_ctx_active_since(&conn).is_some());
    db::reset_ctx_active_since(&conn).unwrap();
    assert!(db::get_ctx_active_since(&conn).is_none());
}
