//! Unix socket event stream (profile, budget, and related queries).

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::io::{BufRead, Write};
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn socket_query(sock_path: &std::path::Path, line: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(sock_path)?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    Ok(buf)
}

#[test]
#[serial]
fn journey_unix_socket() {
    let h = CtxHarness::new();
    h.write_config("active_profile = \"carrier\"\nmonthly_budget_usd = 100.0\n");
    let sock_path = h.tmp.path().join("ctx.sock");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let listener = rt.spawn(async {
        let _ = ctx::socket::run_listener().await;
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !sock_path.exists() {
        if std::time::Instant::now() > deadline {
            listener.abort();
            panic!("ctx.sock did not appear");
        }
        std::thread::sleep(Duration::from_millis(30));
    }

    let profile_line = socket_query(&sock_path, r#"{"q":"profile"}"#).unwrap();
    let profile: serde_json::Value = serde_json::from_str(profile_line.trim()).unwrap();
    assert_eq!(profile["profile"], "carrier");

    let budget_line = socket_query(&sock_path, r#"{"q":"budget"}"#).unwrap();
    let budget: serde_json::Value = serde_json::from_str(budget_line.trim()).unwrap();
    assert!(budget.get("remaining_usd").is_some());

    let _ = socket_query(&sock_path, "not json");

    listener.abort();
    rt.shutdown_background();
    ctx::socket::cleanup_socket_file();

    assert!(!sock_path.exists());
    assert!(UnixStream::connect(&sock_path).is_err());
}
