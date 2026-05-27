//! Self-tuning A/B comparison writes ab-results.json and optional auto-apply.

mod harness;

use harness::CtxHarness;
use serial_test::serial;

#[test]
#[serial]
fn journey_self_tuning_recommendations() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
[ab_test]
profile_pct = 50
inject_pct = 100
adaptive_pct = 100
coaching_pct = 100
"#,
    );

    let conn = h.open();
    for i in 0..200 {
        let group = if i < 100 { "P:T I:T" } else { "P:C I:T" };
        let cost = if i < 100 { 0.03 } else { 0.05 };
        conn.execute(
            r#"INSERT INTO hook_traces (ts, session_id, working_directory, profile, ab_group, cost_usd, enriched, coach_kind)
               VALUES (datetime('now'), ?1, '/tmp', 'carrier', ?2, ?3, 1, '')"#,
            rusqlite::params![format!("s-{i}"), group, cost],
        )
        .unwrap();
    }
    drop(conn);

    let conn = h.open();
    let results = ctx::tuning::run_tuning_after_ingest(&conn)
        .unwrap()
        .expect("results");
    let profile = results
        .features
        .iter()
        .find(|f| f.feature == "profile")
        .expect("profile verdict");
    assert_eq!(profile.verdict, "beneficial");
    assert!(profile.delta_cost_pct.unwrap() < -10.0);

    let path = h.tmp.path().join("ab-results.json");
    assert!(path.is_file());
    let loaded: ctx::tuning::AbResultsFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!loaded.features.is_empty());
}

#[test]
#[serial]
fn journey_self_tuning_auto_apply() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
auto_apply_recommendations = true
inject_enabled = true
adaptive_prefix_enabled = true

[ab_test]
profile_pct = 50
inject_pct = 50
adaptive_pct = 50
coaching_pct = 100
"#,
    );

    let conn = h.open();
    for i in 0..120 {
        conn.execute(
            r#"INSERT INTO hook_traces (ts, session_id, working_directory, profile, ab_group, cost_usd, enriched, coach_kind)
               VALUES (datetime('now'), ?1, '/tmp', 'carrier', 'A:C I:T', 0.05, 1, '')"#,
            [format!("s-{i}")],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO hook_traces (ts, session_id, working_directory, profile, ab_group, cost_usd, enriched, coach_kind)
               VALUES (datetime('now'), ?1, '/tmp', 'carrier', 'A:T I:C', 0.03, 1, '')"#,
            [format!("s2-{i}")],
        )
        .unwrap();
    }
    drop(conn);

    let conn = h.open();
    let _ = ctx::tuning::run_tuning_after_ingest(&conn).unwrap();

    let cfg = std::fs::read_to_string(h.tmp.path().join("config.toml")).unwrap();
    assert!(!cfg.contains("[ab_test]"));
}
