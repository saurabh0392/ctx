//! Claude Code command hooks: read JSON from stdin, write JSON to stdout only.

use anyhow::Result;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Fire-and-forget: ask the dashboard to run `ingest_claude_jsonl()` so Cursor IDE sessions
/// stay fresh (async HTTP hooks often do not run there; this runs once per user prompt).
fn spawn_trigger_ingest(dashboard_port: u16) {
    std::thread::spawn(move || {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, dashboard_port));
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
            return;
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let req = format!(
            "POST /api/trigger-ingest HTTP/1.1\r\nHost: 127.0.0.1:{dashboard_port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(req.as_bytes());
    });
}

/// `UserPromptSubmit` handler: auto-profile, budget hard-stop, system prefix via `additionalContext`.
pub fn user_prompt_submit() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let input: serde_json::Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cwd = input["cwd"].as_str().unwrap_or("");
    let prompt = input["prompt"].as_str().unwrap_or("");
    let pseudo_system = format!("Primary working directory: {cwd}\n");

    let mut cfg = crate::config::Config::load();
    let active = cfg.active_profile.as_deref().unwrap_or("all");

    if cfg.auto_profile_enabled {
        if let Some((new_slug, _)) = crate::profiles::auto_select(&pseudo_system, active) {
            crate::profiles::apply_profile(&new_slug, true, true)?;
            cfg = crate::config::Config::load();
        }
    }

    if let Some(reason) = crate::budget_guard::hard_block_reason_for_prompt(prompt) {
        let out = json!({
            "decision": "block",
            "reason": reason
        });
        print!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    let mut extra = String::new();
    if cfg.inject_enabled {
        if let Some(prefix) = crate::inject::load_prefix() {
            extra.push_str(prefix.trim());
            extra.push_str("\n\n");
        }
    }

    if extra.trim().is_empty() {
        print!("{}", serde_json::to_string(&json!({}))?);
    } else {
        let out = json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": extra.trim_end()
            }
        });
        print!("{}", serde_json::to_string(&out)?);
    }

    let dash = cfg.dashboard_port.unwrap_or(8789);
    spawn_trigger_ingest(dash);

    Ok(())
}
