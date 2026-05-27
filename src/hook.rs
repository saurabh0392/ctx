//! Claude Code command hooks: read JSON from stdin, write JSON to stdout only.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

/// Last chunk of JSONL to scan for user rows (fast on large session files).
const JSONL_TAIL_BYTES: u64 = 96 * 1024;

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

fn find_claude_session_jsonl(session_id: &str) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let projects = home.join(".claude").join("projects");
    let rd = std::fs::read_dir(&projects).ok()?;
    let fname = format!("{session_id}.jsonl");
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let candidate = p.join(&fname);
        if candidate.is_file() {
            let name = candidate.file_name()?.to_string_lossy();
            if name.contains("compact") {
                continue;
            }
            return Some(candidate);
        }
    }
    None
}

fn human_text_from_user_json_line(v: &Value) -> Option<String> {
    if v.get("type").and_then(|x| x.as_str()) != Some("user") {
        return None;
    }
    if v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false) {
        return None;
    }
    let msg = v.get("message")?;
    let content = msg.get("content")?;
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                    return Some(txt.chars().take(2000).collect());
                }
            }
        }
        return None;
    }
    content
        .as_str()
        .map(|s| s.chars().take(2000).collect())
}

fn tail_user_texts_from_jsonl(path: &Path) -> Vec<String> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let len = meta.len();
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let start = len.saturating_sub(JSONL_TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = buf.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let mut out = Vec::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(t) = human_text_from_user_json_line(&v) {
            if !t.trim().is_empty() {
                out.push(t);
            }
        }
    }
    out
}

/// Prior user rows from session JSONL (tail scan), plus the in-flight prompt as the last turn.
fn coaching_user_texts(session_id: Option<&str>, current_prompt: &str) -> Vec<String> {
    let mut out = session_id
        .and_then(|sid| find_claude_session_jsonl(sid))
        .map(|p| tail_user_texts_from_jsonl(&p))
        .unwrap_or_default();
    if !current_prompt.trim().is_empty() {
        out.push(current_prompt.to_string());
    }
    out
}

fn coach_kind_for_signal(sig: &crate::coach::CoachSignal) -> &'static str {
    match sig.kind {
        crate::coach::SignalKind::CorrectionCascade => "correction-cascade",
        crate::coach::SignalKind::ReAsk => "re-ask",
    }
}

/// `UserPromptSubmit` handler: auto-profile, budget hard-stop, system prefix via `additionalContext`.
/// Optional JSONL-based coaching. Records a hook_trace row for the dashboard request trace.
pub fn user_prompt_submit() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let input: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cwd = input["cwd"].as_str().unwrap_or("");
    let prompt = input["prompt"].as_str().unwrap_or("");
    let session_id = input["session_id"].as_str().or_else(|| input["sessionId"].as_str());
    let pseudo_system = format!("Primary working directory: {cwd}\n");

    let mut cfg = crate::config::Config::load();
    let active = cfg.active_profile.as_deref().unwrap_or("all").to_string();

    let mut auto_selected = false;
    let mut auto_trigger: Option<String> = None;

    if cfg.auto_profile_enabled {
        if let Some((new_slug, trigger)) = crate::profiles::auto_select(&pseudo_system, &active) {
            auto_selected = true;
            auto_trigger = Some(trigger);
            crate::profiles::apply_profile(&new_slug, true, true)?;
            cfg = crate::config::Config::load();
        }
    }

    let effective_profile = cfg.active_profile.as_deref().unwrap_or("all").to_string();

    if let Some(reason) = crate::budget_guard::hard_block_reason_for_prompt(prompt) {
        let out = json!({
            "decision": "block",
            "reason": reason
        });
        print!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    let coaching_texts = if cfg.coaching_enabled {
        coaching_user_texts(session_id, prompt)
    } else {
        Vec::new()
    };

    if cfg.coaching_enabled {
        if let Some(reason) = crate::coach::severe_correction_fatigue_reason(&coaching_texts) {
            let profiles = crate::profiles::load_all();
            let (tools_kept, tools_removed, tokens_saved) =
                if let Some(p) = profiles.get(&effective_profile) {
                    let kept = p.tool_count();
                    let removed = crate::profiles::TOTAL_TOOLS.saturating_sub(kept);
                    let saved = p.savings_vs_all();
                    (kept, removed, saved)
                } else {
                    (crate::profiles::TOTAL_TOOLS, 0, 0)
                };
            if let Ok(conn) = crate::db::open_db() {
                let _ = crate::db::ensure_schema(&conn);
                let _ = crate::db::insert_hook_trace(
                    &conn,
                    session_id,
                    cwd,
                    &effective_profile,
                    auto_selected,
                    auto_trigger.as_deref(),
                    false,
                    Some("correction-fatigue"),
                    false,
                    tools_kept,
                    tools_removed,
                    tokens_saved,
                    false,
                );
            }
            let out = json!({
                "decision": "block",
                "reason": reason
            });
            print!("{}", serde_json::to_string(&out)?);
            return Ok(());
        }
    }

    let mut inject_fired = false;
    let mut extra = String::new();
    if cfg.inject_enabled {
        if let Some(prefix) = crate::inject::load_prefix() {
            extra.push_str(prefix.trim());
            extra.push_str("\n\n");
            inject_fired = true;
        }
    }

    let mut coach_kind: Option<String> = None;
    if cfg.coaching_enabled {
        if let Some(sig) = crate::coach::detect_from_user_texts(&coaching_texts) {
            extra.push_str(sig.suggestion.trim());
            extra.push_str("\n\n");
            coach_kind = Some(coach_kind_for_signal(&sig).to_string());
        }
    }

    let model_hint = input
        .get("model")
        .and_then(|x| x.as_str())
        .or_else(|| input.pointer("/transcript/model").and_then(|x| x.as_str()));
    let max_adaptive = crate::adaptive::max_chars_for_hook_input(model_hint);
    let mut adaptive_fired = false;
    if cfg.adaptive_prefix_enabled {
        if let Some(ad) = crate::adaptive::load_adaptive_prefix() {
            let trimmed = crate::adaptive::truncate_to_char_budget(ad.trim(), max_adaptive);
            if !trimmed.is_empty() {
                extra.push_str(&trimmed);
                extra.push_str("\n\n");
                adaptive_fired = true;
            }
        }
    }

    let budget_fired = false;

    // Compute savings from profile definition
    let profiles = crate::profiles::load_all();
    let (tools_kept, tools_removed, tokens_saved) = if let Some(p) = profiles.get(&effective_profile) {
        let kept = p.tool_count();
        let removed = crate::profiles::TOTAL_TOOLS.saturating_sub(kept);
        let saved = p.savings_vs_all();
        (kept, removed, saved)
    } else {
        (crate::profiles::TOTAL_TOOLS, 0, 0)
    };

    // Record hook trace row for dashboard
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_hook_trace(
            &conn,
            session_id,
            cwd,
            &effective_profile,
            auto_selected,
            auto_trigger.as_deref(),
            inject_fired,
            coach_kind.as_deref(),
            budget_fired,
            tools_kept,
            tools_removed,
            tokens_saved,
            adaptive_fired,
        );
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
