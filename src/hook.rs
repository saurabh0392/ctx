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

/// Notify open dashboard tabs via SSE (fire-and-forget).
fn spawn_dashboard_push(dashboard_port: u16, kind: &str) {
    let kind = kind.replace('"', "");
    std::thread::spawn(move || {
        let body = format!(r#"{{"kind":"{kind}"}}"#);
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, dashboard_port));
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
            return;
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let req = format!(
            "POST /api/dashboard/push HTTP/1.1\r\nHost: 127.0.0.1:{dashboard_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(req.as_bytes());
    });
}

pub fn find_claude_session_jsonl(session_id: &str) -> Option<std::path::PathBuf> {
    let home = crate::config::home_dir_for_paths()?;
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

pub fn human_text_from_user_json_line(v: &Value) -> Option<String> {
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
    content.as_str().map(|s| s.chars().take(2000).collect())
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

/// Last assistant prose line from a session JSONL tail scan (for Stop-hook recovery).
pub fn latest_assistant_text_for_session(session_id: &str) -> Option<String> {
    let path = find_claude_session_jsonl(session_id)?;
    tail_assistant_text_from_jsonl(&path)
}

fn tail_assistant_text_from_jsonl(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let len = meta.len();
    let mut f = std::fs::File::open(path).ok()?;
    let start = len.saturating_sub(JSONL_TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return None;
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return None;
    }
    let mut lines: Vec<&str> = buf.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let mut last_text: Option<String> = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let type_ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if type_ != "assistant" {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        let Some(content) = msg.get("content") else {
            continue;
        };
        if let Some(arr) = content.as_array() {
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                        if !txt.trim().is_empty() {
                            last_text = Some(txt.to_string());
                        }
                    }
                }
            }
        } else if let Some(s) = content.as_str() {
            if !s.trim().is_empty() {
                last_text = Some(s.to_string());
            }
        }
    }
    last_text
}

/// Prior user rows from session JSONL (tail scan), plus the in-flight prompt as the last turn.
fn coaching_user_texts(session_id: Option<&str>, current_prompt: &str) -> Vec<String> {
    coaching_user_texts_inner(session_id, current_prompt)
}

/// Public wrapper for `simulate.rs` -- reads session JSONL without side effects.
pub fn coaching_user_texts_public(session_id: Option<&str>, current_prompt: &str) -> Vec<String> {
    coaching_user_texts_inner(session_id, current_prompt)
}

fn coaching_user_texts_inner(session_id: Option<&str>, current_prompt: &str) -> Vec<String> {
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

fn profile_savings(profile_slug: &str) -> (usize, usize, usize) {
    let profiles = crate::profiles::load_all();
    let total = crate::profiles::dynamic_total_tools();
    if let Some(p) = profiles.get(profile_slug) {
        let kept = p.tool_count();
        let removed = total.saturating_sub(kept);
        let saved = p.savings_vs_all();
        (kept, removed, saved)
    } else {
        (total, 0, 0)
    }
}

fn parent_session_from_input(input: &Value) -> Option<&str> {
    input
        .get("parentSessionId")
        .or_else(|| input.get("parent_session_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn record_hook_trace(
    session_id: Option<&str>,
    parent_session_id: Option<&str>,
    cwd: &str,
    profile: &str,
    mode: Option<&str>,
    auto_selected: bool,
    auto_trigger: Option<&str>,
    inject_fired: bool,
    coach_kind: Option<&str>,
    budget_fired: bool,
    tools_kept: usize,
    tools_removed: usize,
    tokens_saved: usize,
    adaptive_fired: bool,
    ab_group: Option<&str>,
    inject_chars: usize,
    adaptive_chars: usize,
    budget_blocked: bool,
    pinned_profile: Option<&str>,
    effective_profile: Option<&str>,
    prompt: &str,
    tools_expanded: &[crate::semantic_tools::ToolExpansionEntry],
) {
    let expansions_json =
        serde_json::to_string(tools_expanded).unwrap_or_else(|_| "[]".to_string());
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_hook_trace(
            &conn,
            session_id,
            parent_session_id,
            cwd,
            profile,
            mode,
            auto_selected,
            auto_trigger,
            inject_fired,
            coach_kind,
            budget_fired,
            tools_kept,
            tools_removed,
            tokens_saved,
            adaptive_fired,
            ab_group,
            inject_chars,
            adaptive_chars,
            budget_blocked,
            pinned_profile,
            effective_profile,
            Some(prompt),
            Some(&expansions_json),
        );
        let port = crate::config::Config::load().dashboard_port.unwrap_or(8789);
        spawn_dashboard_push(port, "hook_trace");
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
    let session_id = input["session_id"]
        .as_str()
        .or_else(|| input["sessionId"].as_str());
    let parent_session_id = parent_session_from_input(&input);

    let mut cfg = crate::config::Config::load();
    let active_mode_name = cfg.active_mode.clone();
    let ab_request_key = crate::ab::request_key(session_id, cwd, prompt);
    let (ab, ab_group) = if let Some(ab_cfg) = cfg.ab_test.clone() {
        let assignments = crate::ab::AbAssignments::from_config(&ab_cfg, &ab_request_key);
        let group = crate::ab::AbAssignments::format_group(&ab_cfg, &assignments);
        (assignments, group)
    } else {
        (
            crate::ab::AbAssignments {
                profile: true,
                inject: true,
                adaptive: true,
                coaching: true,
                compress: true,
                compress_sgr: true,
                tool_mix: true,
            },
            None,
        )
    };

    let active = cfg.active_profile.as_deref().unwrap_or("all").to_string();

    let mut auto_selected = false;
    let mut auto_trigger: Option<String> = None;
    let mut effective_profile = active.clone();

    // Auto-profile runs regardless of A/B arm; it picks the effective profile when enabled.
    if cfg.auto_profile_enabled {
        if let Some((new_slug, trigger)) = crate::profiles::auto_select(cwd, prompt, &active) {
            auto_selected = true;
            auto_trigger = Some(trigger);
            effective_profile = new_slug;
        }
    }

    // Profile-filter experiment: treatment applies auto's pick; control strips filtering for this prompt.
    let filter_profile = if ab.profile {
        effective_profile.as_str()
    } else {
        "all"
    };
    let mut trace_profile = filter_profile.to_string();
    let mut trace_expansions: Vec<crate::semantic_tools::ToolExpansionEntry> = Vec::new();

    if cfg.filter_mode == crate::config::FilterMode::Soft {
        if ab.profile {
            let run_semantic_mix = cfg.semantic_tool_mix_enabled && ab.tool_mix;
            trace_expansions = crate::filter_control::hook_sync_profile(
                filter_profile,
                prompt,
                cwd,
                true,
                run_semantic_mix,
            )?;
            cfg = crate::config::Config::load();
            trace_profile = cfg.active_profile.as_deref().unwrap_or("all").to_string();
        } else {
            crate::filter_control::hook_apply_control_filter(true)?;
        }
    } else if !ab.profile {
        trace_profile = "all".to_string();
    }

    // Carried-set snapshot (CTX-66 / M-D, Part 2): once the menu is actively managed (a server is
    // pruned), record what this prompt carries per server so the literal invoked-vs-carried
    // cross-check has forward data. The primary harm signal is the reach-based tool-miss, not this,
    // so it stays cheap and only fires under active management. Best-effort; never fails the hook.
    if cfg.filter_mode == crate::config::FilterMode::Soft && !cfg.pruned_servers.is_empty() {
        let by_server = crate::profiles::carried_menu_by_server();
        if !by_server.is_empty() {
            // Input tax reclaimed on this request: the cost of exactly the tool schemas ctx removes
            // from the menu. A dead server's full catalog, or just the named dead tools of a live
            // server that was pruned tool by tool (never its used tools). Folds into WNAD (CTX-68).
            let mut prune_expansion = cfg.session_expansion.clone();
            prune_expansion.extend(cfg.session_semantic_tools.clone());
            let tokens_saved = crate::profiles::pruned_input_tax_tokens(
                &cfg.pruned_servers,
                &prune_expansion,
                &[],
            );
            let rec = crate::analytics::Record {
                ts: chrono::Utc::now().to_rfc3339(),
                profile: trace_profile.clone(),
                working_directory: cwd.to_string(),
                kept_servers: by_server.keys().cloned().collect(),
                tools_sent_count: by_server.values().sum(),
                tools_sent_by_server: by_server,
                tokens_saved,
                ..Default::default()
            };
            if let Ok(conn) = crate::db::open_db() {
                let _ = crate::db::insert_request(&conn, &rec);
            }
        }
    }

    // Autopilot server management (CTX-67 / M-E): when auto-apply is on, the earn-it gate may
    // trial-hide a strongly dead-weight server or reverse a prune that started drawing reaches.
    // Self-gated, fail-closed, reversible; a no-op unless the causal evidence supports an action.
    if cfg.auto_apply_recommendations {
        let (pruned, unpruned) = crate::compress::tool_activation::autopilot_manage_servers(&cfg);
        if !pruned.is_empty() {
            eprintln!("[ctx] autopilot trial-hid dead-weight server(s): {}", pruned.join(", "));
        }
        if !unpruned.is_empty() {
            eprintln!("[ctx] autopilot re-added reached-for server(s): {}", unpruned.join(", "));
        }
    }

    let trace_effective = if auto_selected {
        Some(effective_profile.as_str())
    } else {
        None
    };

    if let Some(reason) = crate::budget_guard::hard_block_reason_for_prompt(prompt) {
        let (tools_kept, tools_removed, tokens_saved) = if ab.profile {
            profile_savings(&trace_profile)
        } else {
            profile_savings("all")
        };
        record_hook_trace(
            session_id,
            parent_session_id,
            cwd,
            &trace_profile,
            active_mode_name.as_deref(),
            auto_selected,
            auto_trigger.as_deref(),
            false,
            None,
            false,
            tools_kept,
            tools_removed,
            tokens_saved,
            false,
            ab_group.as_deref(),
            0,
            0,
            true,
            Some(active.as_str()),
            trace_effective,
            prompt,
            &trace_expansions,
        );
        let out = json!({
            "decision": "block",
            "reason": reason
        });
        print!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    let coaching_enabled = cfg.coaching_enabled && ab.coaching;
    let coaching_texts = if coaching_enabled {
        coaching_user_texts(session_id, prompt)
    } else {
        Vec::new()
    };

    if coaching_enabled {
        if let Some(reason) = crate::coach::severe_correction_fatigue_reason(&coaching_texts) {
            let (tools_kept, tools_removed, tokens_saved) = if ab.profile {
                profile_savings(&trace_profile)
            } else {
                profile_savings("all")
            };
            record_hook_trace(
                session_id,
                parent_session_id,
                cwd,
                &trace_profile,
                active_mode_name.as_deref(),
                auto_selected,
                auto_trigger.as_deref(),
                false,
                Some("correction-fatigue"),
                false,
                tools_kept,
                tools_removed,
                tokens_saved,
                false,
                ab_group.as_deref(),
                0,
                0,
                false,
                Some(active.as_str()),
                trace_effective,
                prompt,
                &trace_expansions,
            );
            let out = json!({
                "decision": "block",
                "reason": reason
            });
            print!("{}", serde_json::to_string(&out)?);
            return Ok(());
        }
    }

    let mut inject_fired = false;
    let mut inject_chars = 0usize;
    let mut extra = String::new();
    if cfg.inject_enabled && ab.inject {
        if let Some(prefix) = crate::inject::load_prefix() {
            let trimmed = prefix.trim();
            inject_chars = trimmed.chars().count();
            extra.push_str(trimmed);
            extra.push_str("\n\n");
            inject_fired = true;
        }
    }

    let mut coach_kind: Option<String> = None;
    if coaching_enabled {
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
    let mut adaptive_chars = 0usize;
    if cfg.adaptive_prefix_enabled && ab.adaptive {
        if let Some(ad) = crate::adaptive::load_adaptive_prefix() {
            let trimmed = crate::adaptive::truncate_to_char_budget(ad.trim(), max_adaptive);
            if !trimmed.is_empty() {
                adaptive_chars = trimmed.chars().count();
                extra.push_str(&trimmed);
                extra.push_str("\n\n");
                adaptive_fired = true;
            }
        }
    }

    let mut budget_fired = false;
    let mut budget_texts = coaching_texts.clone();
    budget_texts.push(prompt.to_string());
    if let Some(warning) =
        crate::budget_guard::soft_warning_for_hook_input(&input, prompt, &budget_texts, model_hint)
    {
        extra.push_str(warning.trim());
        extra.push_str("\n\n");
        budget_fired = true;
    }

    let budget_blocked = false;

    // Cross-session restore scratchpad: if an earlier session queued a pruned tool to come back (via
    // ctx_restore) and this is a different session, its catalog is now in the menu. Hand the saved
    // note to the agent once so the blocked work resumes, then mark it delivered.
    let sid = session_id.unwrap_or("");
    let pending = crate::restore_queue::pending_for_new_session(sid);
    if !pending.is_empty() {
        let mut block =
            String::from("Restored MCP tools from a previous session (available now):\n");
        for r in &pending {
            if r.tasks.trim().is_empty() {
                block.push_str(&format!("- {}\n", r.display));
            } else {
                block.push_str(&format!("- {}: {}\n", r.display, r.tasks.trim()));
            }
        }
        extra.push_str(block.trim_end());
        extra.push_str("\n\n");
        let _ = crate::restore_queue::mark_delivered_for_new_session(sid);
    }

    let (tools_kept, tools_removed, tokens_saved) = if ab.profile {
        profile_savings(&trace_profile)
    } else {
        profile_savings("all")
    };

    record_hook_trace(
        session_id,
        parent_session_id,
        cwd,
        &trace_profile,
        active_mode_name.as_deref(),
        auto_selected,
        auto_trigger.as_deref(),
        inject_fired,
        coach_kind.as_deref(),
        budget_fired,
        tools_kept,
        tools_removed,
        tokens_saved,
        adaptive_fired,
        ab_group.as_deref(),
        inject_chars,
        adaptive_chars,
        budget_blocked,
        Some(active.as_str()),
        trace_effective,
        prompt,
        &trace_expansions,
    );

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
