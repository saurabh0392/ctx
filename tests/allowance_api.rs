//! Allowance snapshot API and burn-rate tests.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::time::Duration;

fn seed_window(
    conn: &rusqlite::Connection,
    start: &str,
    count: usize,
    step_mins: i64,
    step_pct: f64,
) {
    let start_dt: chrono::DateTime<chrono::Utc> = start.parse().unwrap();
    for i in 0..count {
        let ts = (start_dt + chrono::Duration::minutes(step_mins * i as i64)).to_rfc3339();
        let used = step_pct * i as f64;
        ctx::db::insert_allowance_snapshot(
            conn,
            &ts,
            Some("sess"),
            Some("Sonnet"),
            ctx::allowance::PRIMARY_WINDOW,
            used,
            Some((100.0 - used).max(0.0)),
            Some(start_dt.timestamp() + 86400 * 7),
            None,
        )
        .unwrap();
    }
}

#[tokio::test]
#[serial]
async fn allowance_snapshot_post_and_current() {
    let _h = CtxHarness::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    tokio::spawn(async move {
        let _ = ctx::dashboard::serve(port, true).await;
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let now = chrono::Utc::now();
    let payload = serde_json::json!({
        "session_id": "abc",
        "model": { "display_name": "Sonnet" },
        "rate_limits": {
            "five_hour": { "used_percentage": 12.0, "resets_at": now.timestamp() + 3600 },
            "seven_day": { "used_percentage": 34.0, "resets_at": now.timestamp() + 86400 * 5 }
        }
    });

    let post = client
        .post(format!("http://127.0.0.1:{port}/api/allowance/snapshot"))
        .json(&payload)
        .send()
        .await
        .expect("POST snapshot");
    assert!(post.status().is_success());

    let cur: serde_json::Value = client
        .get(format!("http://127.0.0.1:{port}/api/allowance/current"))
        .send()
        .await
        .expect("GET current")
        .json()
        .await
        .expect("json");
    assert_eq!(cur["configured"], true);
    assert_eq!(
        cur["windows"]["seven_day"]["used_pct"].as_f64().unwrap(),
        34.0
    );
    assert_eq!(
        cur["windows"]["seven_day"]["remaining_pct"]
            .as_f64()
            .unwrap(),
        66.0
    );
}

#[test]
#[serial]
fn burn_rate_ready_with_enough_snapshots() {
    let h = CtxHarness::new();
    let conn = h.open();
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('ctx_active_since', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    seed_window(&conn, "2020-01-01T00:00:00Z", 12, 30, 2.0);
    let recent = chrono::Utc::now() - chrono::Duration::days(6);
    seed_window(&conn, &recent.to_rfc3339(), 12, 60, 0.5);

    let resp = ctx::allowance::burn_rate(&conn);
    assert!(resp.metrics_ready, "{:?}", resp.message);
    assert_eq!(resp.direction.as_deref(), Some("slower"));
}

#[test]
#[serial]
fn current_rejects_stale_snapshot_ts() {
    let h = CtxHarness::new();
    let conn = h.open();
    let past = chrono::Utc::now().timestamp() - 3600;
    ctx::db::insert_allowance_snapshot(
        &conn,
        &(chrono::Utc::now() - chrono::Duration::hours(7)).to_rfc3339(),
        None,
        Some("Sonnet"),
        ctx::allowance::PRIMARY_WINDOW,
        34.0,
        Some(66.0),
        Some(past),
        None,
    )
    .unwrap();
    let resp = ctx::allowance::current_allowance(&conn);
    assert!(!resp.configured);
    assert!(resp.stale);
}

#[test]
#[serial]
fn current_accepts_fresh_snapshot_with_past_resets_at() {
    let h = CtxHarness::new();
    let conn = h.open();
    let past = chrono::Utc::now().timestamp() - 3600;
    ctx::db::insert_allowance_snapshot(
        &conn,
        &chrono::Utc::now().to_rfc3339(),
        None,
        Some("Sonnet"),
        ctx::allowance::PRIMARY_WINDOW,
        34.0,
        Some(66.0),
        Some(past),
        None,
    )
    .unwrap();
    let resp = ctx::allowance::current_allowance(&conn);
    assert!(resp.configured);
    assert!(!resp.stale);
    assert_eq!(resp.windows["seven_day"].used_pct, 34.0);
}

#[test]
#[serial]
fn ingest_throttles_duplicate_snapshots() {
    let h = CtxHarness::new();
    let conn = h.open();
    let payload = serde_json::json!({
        "rate_limits": {
            "seven_day": { "used_percentage": 10.0, "resets_at": chrono::Utc::now().timestamp() + 86400 }
        }
    });
    let n1 = ctx::allowance::ingest_statusline_payload(&conn, &payload).unwrap();
    let n2 = ctx::allowance::ingest_statusline_payload(&conn, &payload).unwrap();
    assert_eq!(n1, 1);
    assert_eq!(n2, 0);
}
