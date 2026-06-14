use chrono::Datelike;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

fn ok_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

fn err_response(id: Value, code: i64, msg: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(json!({ "code": code, "message": msg })),
    }
}

const TOOL_DEFS: &[(&str, &str, &str)] = &[
    (
        "ctx_status",
        "Current ctx status: active profile, session count, token savings, cost saved, budget info.",
        "{}",
    ),
    (
        "ctx_spend",
        "Monthly spend breakdown: total USD, tokens by type, session count, ctx savings, budget vs actual. Optional month param (YYYY-MM).",
        r#"{"type":"object","properties":{"month":{"type":"string","description":"Month in YYYY-MM format. Defaults to current month."}}}"#,
    ),
    (
        "ctx_sessions",
        "Recent sessions with cost, duration, turns, model, project folder. Returns up to 20.",
        "{}",
    ),
    (
        "ctx_tips",
        "Cost optimization tips based on session analysis: what is driving spend and how to reduce it.",
        "{}",
    ),
    (
        "ctx_patterns",
        "Detected repeat patterns: recurring prompts, project concentration, evening spikes. Each has estimated USD impact.",
        "{}",
    ),
    (
        "ctx_settings",
        "Full ctx configuration and data transparency: config values, DB stats, row counts, file sizes, last ingest time.",
        "{}",
    ),
    (
        "ctx_profiles",
        "List all MCP filter profiles with token estimates. Shows which is active.",
        "{}",
    ),
    (
        "ctx_waste",
        "Lists MCP servers that were loaded on every request but never actually invoked in the last 30 days. These are pure token waste — add them to a profile's strip list.",
        r#"{"type":"object","properties":{},"required":[]}"#,
    ),
];

fn build_tools_list() -> Value {
    let tools: Vec<Value> = TOOL_DEFS.iter().map(|(name, desc, schema_str)| {
        let schema: Value = serde_json::from_str(schema_str).unwrap_or(json!({}));
        json!({
            "name": name,
            "description": desc,
            "inputSchema": if schema.is_object() && schema.as_object().map_or(true, |m| m.is_empty()) {
                json!({"type": "object", "properties": {}})
            } else {
                schema
            }
        })
    }).collect();
    json!({ "tools": tools })
}

fn handle_tool_call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "ctx_status" => tool_status(),
        "ctx_spend" => tool_spend(args),
        "ctx_sessions" => tool_sessions(),
        "ctx_tips" => tool_tips(),
        "ctx_patterns" => tool_patterns(),
        "ctx_settings" => tool_settings(),
        "ctx_profiles" => tool_profiles(),
        "ctx_waste" => tool_waste(),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn tool_status() -> Result<Value, String> {
    let records = crate::analytics::load_records();
    let config = crate::config::Config::load();
    let filter_recs: Vec<_> = records.iter().filter(|r| r.tools_removed > 0).collect();
    let total_tokens: usize = filter_recs.iter().map(|r| r.tokens_saved).sum();
    let total_tools: usize = filter_recs.iter().map(|r| r.tools_removed).sum();
    let all_tokens = total_tokens;

    let spend_sessions = crate::conversations::all_sessions();
    let now = chrono::Utc::now();
    let current_month = format!("{}-{:02}", now.year(), now.month());
    let month_spend: f64 = spend_sessions
        .iter()
        .filter(|s| s.started_at.starts_with(&current_month))
        .map(|s| s.total_usd)
        .sum();
    let month_sessions = spend_sessions
        .iter()
        .filter(|s| s.started_at.starts_with(&current_month))
        .count();

    use chrono::Datelike;
    let day = now.day().max(1) as f64;
    let days_in_month = chrono::NaiveDate::from_ymd_opt(
        if now.month() == 12 {
            now.year() + 1
        } else {
            now.year()
        },
        if now.month() == 12 {
            1
        } else {
            now.month() + 1
        },
        1,
    )
    .unwrap()
    .signed_duration_since(chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap())
    .num_days() as f64;
    let projection = if month_spend > 0.0 {
        Some(month_spend / day * days_in_month)
    } else {
        None
    };

    Ok(json!({
        "active_profile": config.active_profile.as_deref().unwrap_or("all"),
        "month": current_month,
        "month_spend_usd": month_spend,
        "month_sessions": month_sessions,
        "month_end_projection_usd": projection,
        "budget_usd": config.monthly_budget_usd,
        "total_tokens_saved": all_tokens,
        "total_tools_removed": total_tools,
        "cost_saved_usd": (all_tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK,
        "filtered_requests": filter_recs.len(),
        "inject_enabled": config.inject_enabled,
        "store_prompt_text": config.store_prompt_text_enabled(),
        "embeddings_enabled": config.embeddings_enabled(),
    }))
}

fn tool_spend(args: &Value) -> Result<Value, String> {
    let sessions = crate::conversations::all_sessions();
    let all = crate::conversations::monthly_spend(&sessions);
    let month_filter = args.get("month").and_then(|v| v.as_str());
    let filtered: Vec<_> = if let Some(m) = month_filter {
        all.into_iter().filter(|s| s.month == m).collect()
    } else {
        all
    };
    serde_json::to_value(&filtered).map_err(|e| e.to_string())
}

fn tool_sessions() -> Result<Value, String> {
    let sessions = crate::conversations::all_sessions();
    let recent: Vec<_> = sessions.into_iter().take(20).collect();
    let summaries: Vec<Value> = recent
        .iter()
        .map(|s| {
            json!({
                "started_at": s.started_at,
                "project": s.project,
                "turns": s.turn_count,
                "total_usd": s.total_usd,
                "input_tokens": s.input_tokens,
                "cache_read_tokens": s.cache_read_tokens,
                "output_tokens": s.output_tokens,
                "models": s.models_used,
                "hit_compact": s.hit_compact,
                "correction_turns": s.correction_turns,
                "clarifying_turns": s.clarifying_turns,
                "first_message_preview": s.first_user_message.chars().take(200).collect::<String>(),
            })
        })
        .collect();
    Ok(json!(summaries))
}

fn tool_tips() -> Result<Value, String> {
    let mut sessions = crate::conversations::all_sessions();
    let now = chrono::Utc::now();
    let current_month = format!("{}-{:02}", now.year(), now.month());
    sessions.retain(|s| s.started_at.starts_with(&current_month));
    let tips = crate::conversations::generate_tips(&sessions);
    serde_json::to_value(&tips).map_err(|e| e.to_string())
}

fn tool_patterns() -> Result<Value, String> {
    let conn = crate::db::open_db().map_err(|e| e.to_string())?;
    crate::db::ensure_schema(&conn).map_err(|e| e.to_string())?;
    let alerts = crate::conversations::detect_patterns(&conn);
    serde_json::to_value(&alerts).map_err(|e| e.to_string())
}

fn tool_settings() -> Result<Value, String> {
    let cfg = crate::config::Config::load();
    let db_path = crate::config::db_path();
    let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let conn = crate::db::open_db().ok();
    let count = |table: &str| -> i64 {
        conn.as_ref()
            .and_then(|c| {
                c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                    .ok()
            })
            .unwrap_or(0)
    };
    let last_ingest: Option<String> = conn.as_ref().and_then(|c| {
        c.query_row("SELECT v FROM meta WHERE k = 'last_ingest_at'", [], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
    });

    Ok(json!({
        "active_profile": cfg.active_profile,
        "dashboard_port": cfg.dashboard_port,
        "auto_profile_enabled": cfg.auto_profile_enabled,
        "inject_enabled": cfg.inject_enabled,
        "store_prompt_text": cfg.store_prompt_text_enabled(),
        "embeddings_enabled": cfg.embeddings_enabled(),
        "monthly_budget_usd": cfg.monthly_budget_usd,
        "monthly_actual_spend_usd": cfg.monthly_actual_spend_usd,
        "ctx_home": crate::config::ctx_dir().to_string_lossy().into_owned(),
        "db_size_bytes": db_size,
        "last_ingest_at": last_ingest,
        "row_counts": json!({
            "sessions": count("sessions"),
            "turns": count("turns"),
            "tool_invocations": count("tool_invocations"),
            "session_embeddings": count("session_embeddings"),
            "requests": count("requests"),
        }),
    }))
}

fn tool_profiles() -> Result<Value, String> {
    let profiles = crate::profiles::list_profiles_json();
    Ok(profiles)
}

fn tool_waste() -> Result<Value, String> {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => return Ok(serde_json::json!({"error": format!("db: {e}")})),
    };
    let unused = crate::db::zero_usage_servers(&conn, 30).unwrap_or_default();
    if unused.is_empty() {
        return Ok(serde_json::json!({
            "content": [{"type": "text", "text": "No waste detected — every loaded MCP server was invoked at least once in the last 30 days."}]
        }));
    }
    let list = unused.join("\n  - ");
    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": format!(
                "Servers loaded but never invoked (last 30 days):\n  - {list}\n\nAdd these to a profile's strip list with:\n  ctx profile add <name> --keep <servers-you-want-to-keep>"
            )
        }]
    }))
}

fn handle_request(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let id = req.id.clone().unwrap_or(Value::Null);

    if req.jsonrpc != "2.0" {
        return Some(err_response(id, -32600, "Invalid JSON-RPC version"));
    }

    match req.method.as_str() {
        "initialize" => Some(ok_response(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "ctx",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )),
        "notifications/initialized" => None,
        "tools/list" => Some(ok_response(id, build_tools_list())),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            match handle_tool_call(name, &args) {
                Ok(result) => {
                    let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                    Some(ok_response(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": text }],
                        }),
                    ))
                }
                Err(e) => Some(ok_response(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true,
                    }),
                )),
            }
        }
        _ => {
            if req.id.is_some() {
                Some(err_response(
                    id,
                    -32601,
                    &format!("Method not found: {}", req.method),
                ))
            } else {
                None
            }
        }
    }
}

pub fn serve_stdio() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = err_response(Value::Null, -32700, &format!("Parse error: {e}"));
                let out = serde_json::to_string(&resp)?;
                writeln!(stdout, "{out}")?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_request(&req) {
            let out = serde_json::to_string(&resp)?;
            writeln!(stdout, "{out}")?;
            stdout.flush()?;
        }
    }

    Ok(())
}
