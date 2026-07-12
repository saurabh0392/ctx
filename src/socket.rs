//! Local read-only ctx state queries over a small newline-delimited JSON protocol.
//!
//! Transport is `cfg`-gated: a Unix domain socket at `ctx_dir()/ctx.sock` on unix, and a
//! `127.0.0.1` TCP listener on an ephemeral port (published to `ctx_dir()/ctx.sock.port`) on
//! Windows, where AF_UNIX is not available. The request/response protocol is identical on both.

use std::path::PathBuf;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::config::{ctx_dir, Config};

/// The filesystem artifact the listener owns: the socket file on unix, or the file that publishes
/// the chosen TCP port on Windows. Used for cleanup and for the dashboard status line.
pub fn socket_path() -> PathBuf {
    #[cfg(windows)]
    {
        ctx_dir().join("ctx.sock.port")
    }
    #[cfg(not(windows))]
    {
        ctx_dir().join("ctx.sock")
    }
}

pub async fn run_listener() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        run_unix_listener().await
    }
    #[cfg(windows)]
    {
        run_tcp_listener().await
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(())
    }
}

#[cfg(unix)]
async fn run_unix_listener() -> anyhow::Result<()> {
    use tokio::net::UnixListener;

    let path = socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ctx.sock bind failed ({e}), retrying after remove");
            let _ = std::fs::remove_file(&path);
            UnixListener::bind(&path)?
        }
    };

    eprintln!("ctx event stream listening on {}", path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = handle_connection(stream).await;
        });
    }
}

#[cfg(windows)]
async fn run_tcp_listener() -> anyhow::Result<()> {
    use tokio::net::TcpListener;

    // AF_UNIX is unavailable on Windows, so bind an ephemeral loopback port and publish it to a
    // file under ctx_dir() for local clients to discover. 127.0.0.1 keeps it host-local.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();

    let port_file = socket_path();
    if let Some(parent) = port_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&port_file, port.to_string())?;

    eprintln!("ctx event stream listening on 127.0.0.1:{port}");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = handle_connection(stream).await;
        });
    }
}

/// One request, one response. Generic over the transport stream so the Unix socket and the Windows
/// TCP path share the same protocol handling.
async fn handle_connection<S>(mut stream: S) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(());
    }
    let req: Value = serde_json::from_str(line.trim()).unwrap_or(json!({}));
    let resp = dispatch_query(&req).await;
    let out = serde_json::to_string(&resp)? + "\n";
    stream.write_all(out.as_bytes()).await?;
    Ok(())
}

async fn dispatch_query(req: &Value) -> Value {
    let q = req.get("q").and_then(|v| v.as_str()).unwrap_or("");
    match q {
        "profile" => query_profile().await,
        "budget" => query_budget().await,
        "experiment" => query_experiment().await,
        "last-trace" => query_last_trace().await,
        "adaptive-status" => query_adaptive_status().await,
        _ => json!({ "error": "unknown query", "q": q }),
    }
}

async fn query_profile() -> Value {
    let cfg = Config::load();
    json!({
        "profile": cfg.active_profile.as_deref().unwrap_or("all"),
        "mode": cfg.active_mode,
    })
}

async fn query_budget() -> Value {
    let cfg = Config::load();
    let budget = cfg.monthly_budget_usd.unwrap_or(0.0);
    let used = cfg.monthly_actual_spend_usd.unwrap_or(0.0);
    let remaining = (budget - used).max(0.0);
    let pct = if budget > 0.0 {
        (used / budget * 100.0).min(100.0)
    } else {
        0.0
    };
    json!({
        "remaining_usd": remaining,
        "used_usd": used,
        "pct": pct,
    })
}

async fn query_experiment() -> Value {
    let cfg = Config::load();
    match &cfg.ab_test {
        Some(ab) => json!({
            "active": ab.profile_pct < 100 || ab.inject_pct < 100 || ab.adaptive_pct < 100 || ab.coaching_pct < 100,
            "profile_pct": ab.profile_pct,
            "inject_pct": ab.inject_pct,
            "adaptive_pct": ab.adaptive_pct,
            "coaching_pct": ab.coaching_pct,
        }),
        None => json!({ "active": false }),
    }
}

async fn query_last_trace() -> Value {
    let Ok(conn) = crate::db::open_db() else {
        return json!({ "error": "db" });
    };
    let Ok(rows) = crate::db::load_hook_traces(&conn, 1, 0, None) else {
        return json!({ "error": "load" });
    };
    let Some(ht) = rows.into_iter().next() else {
        return json!({ "empty": true });
    };
    json!({
        "ts": ht.ts,
        "profile": ht.profile,
        "mode": ht.mode,
        "tokens_saved": ht.tokens_saved,
        "cost_usd": ht.cost_usd,
    })
}

async fn query_adaptive_status() -> Value {
    let cfg = Config::load();
    let path = crate::config::adaptive_prefix_path();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let chars = content.chars().count();
    let budget = cfg.adaptive_prefix_max_chars.unwrap_or(2000);
    json!({
        "enabled": cfg.adaptive_prefix_enabled,
        "chars": chars,
        "budget": budget,
        "stale": chars == 0 && cfg.adaptive_prefix_enabled,
    })
}

pub fn spawn_socket_task() {
    tokio::spawn(async {
        if let Err(e) = run_listener().await {
            eprintln!("ctx.sock listener stopped: {e}");
        }
    });
}

pub fn cleanup_socket_file() {
    let path = socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
