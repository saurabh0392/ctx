//! Dashboard A/B API smoke tests.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn ab_report_empty_when_no_experiment_rows() {
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
    let url = format!("http://127.0.0.1:{port}/api/ab-report?since=all");
    let body = client
        .get(&url)
        .send()
        .await
        .expect("GET ab-report")
        .text()
        .await
        .expect("body");
    let report: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(report.len(), 4);
    for row in &report {
        assert_eq!(row["treatment"]["count"].as_i64().unwrap_or(0), 0);
        assert_eq!(row["control"]["count"].as_i64().unwrap_or(0), 0);
    }
}
