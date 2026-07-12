//! ctx state event stream (profile, budget, and related queries).
//!
//! The transport is a Unix domain socket on unix and a 127.0.0.1 TCP listener on Windows, so the
//! journey is exercised per platform: identical newline-delimited JSON protocol, different connect.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

/// One newline-delimited request/response over an already-connected stream. Generic so the Unix
/// socket and the Windows TCP path share it.
fn query_stream<S: Read + Write>(mut stream: S, line: &str) -> std::io::Result<String> {
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    Ok(buf)
}

fn spawn_listener() -> (tokio::runtime::Runtime, tokio::task::JoinHandle<()>) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let listener = rt.spawn(async {
        let _ = ctx::socket::run_listener().await;
    });
    (rt, listener)
}

/// Block until the listener has published its artifact (the socket file, or the port file on
/// Windows), failing the test if it never appears.
fn wait_for(path: &std::path::Path, listener: &tokio::task::JoinHandle<()>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if std::time::Instant::now() > deadline {
            listener.abort();
            panic!("socket artifact did not appear: {}", path.display());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(unix)]
#[test]
#[serial]
fn journey_unix_socket() {
    use std::os::unix::net::UnixStream;

    let h = CtxHarness::new();
    h.write_config("active_profile = \"carrier\"\nmonthly_budget_usd = 100.0\n");
    let sock_path = h.tmp.path().join("ctx.sock");

    let (rt, listener) = spawn_listener();
    wait_for(&sock_path, &listener);

    let profile_line =
        query_stream(UnixStream::connect(&sock_path).unwrap(), r#"{"q":"profile"}"#).unwrap();
    let profile: serde_json::Value = serde_json::from_str(profile_line.trim()).unwrap();
    assert_eq!(profile["profile"], "carrier");

    let budget_line =
        query_stream(UnixStream::connect(&sock_path).unwrap(), r#"{"q":"budget"}"#).unwrap();
    let budget: serde_json::Value = serde_json::from_str(budget_line.trim()).unwrap();
    assert!(budget.get("remaining_usd").is_some());

    let _ = query_stream(UnixStream::connect(&sock_path).unwrap(), "not json");

    listener.abort();
    rt.shutdown_background();
    ctx::socket::cleanup_socket_file();

    assert!(!sock_path.exists());
    assert!(UnixStream::connect(&sock_path).is_err());
}

#[cfg(windows)]
#[test]
#[serial]
fn journey_tcp_socket() {
    use std::net::TcpStream;

    let h = CtxHarness::new();
    h.write_config("active_profile = \"carrier\"\nmonthly_budget_usd = 100.0\n");
    let port_file = h.tmp.path().join("ctx.sock.port");

    let (rt, listener) = spawn_listener();
    wait_for(&port_file, &listener);

    let port: u16 = std::fs::read_to_string(&port_file)
        .unwrap()
        .trim()
        .parse()
        .expect("published port is a u16");
    let addr = format!("127.0.0.1:{port}");

    let profile_line =
        query_stream(TcpStream::connect(&addr).unwrap(), r#"{"q":"profile"}"#).unwrap();
    let profile: serde_json::Value = serde_json::from_str(profile_line.trim()).unwrap();
    assert_eq!(profile["profile"], "carrier");

    let budget_line =
        query_stream(TcpStream::connect(&addr).unwrap(), r#"{"q":"budget"}"#).unwrap();
    let budget: serde_json::Value = serde_json::from_str(budget_line.trim()).unwrap();
    assert!(budget.get("remaining_usd").is_some());

    let _ = query_stream(TcpStream::connect(&addr).unwrap(), "not json");

    listener.abort();
    rt.shutdown_background();
    ctx::socket::cleanup_socket_file();

    assert!(!port_file.exists());
}
