use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone)]
pub struct UserProfile {
    pub p25_msg_len: usize,
    pub median_msg_len: usize,
    pub median_session_turns: usize,
    pub long_session_threshold: usize,
    pub correction_threshold: usize,
    pub calibrated: bool,
    pub session_count: usize,
}

impl Default for UserProfile {
    fn default() -> Self {
        UserProfile {
            p25_msg_len: 40,
            median_msg_len: 120,
            median_session_turns: 15,
            long_session_threshold: 26,
            correction_threshold: 40,
            calibrated: false,
            session_count: 0,
        }
    }
}

impl UserProfile {
    pub fn compute() -> Self {
        if crate::db::db_exists() {
            if let Ok(conn) = crate::db::open_db() {
                if crate::db::ensure_schema(&conn).is_ok() {
                    if let Some(p) = try_compute_from_db(&conn) {
                        if p.calibrated {
                            return p;
                        }
                    }
                }
            }
        }

        let home = dirs::home_dir().unwrap_or_default();
        let projects_dir = home.join(".claude").join("projects");

        let mut msg_lens: Vec<usize> = Vec::new();
        let mut session_turns: Vec<usize> = Vec::new();

        let Ok(proj_entries) = std::fs::read_dir(&projects_dir) else {
            return Self::default();
        };

        for proj_entry in proj_entries.flatten() {
            let proj_path = proj_entry.path();
            if !proj_path.is_dir() { continue; }
            let dir_name = proj_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if dir_name == "subagents" { continue; }

            let Ok(file_entries) = std::fs::read_dir(&proj_path) else { continue };
            for file_entry in file_entries.flatten() {
                let fpath = file_entry.path();
                let fname = fpath.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !fname.ends_with(".jsonl") || fname.contains("compact") { continue; }

                let Ok(content) = std::fs::read_to_string(&fpath) else { continue };
                let (lens, turns) = scan_file(&content);
                msg_lens.extend(lens);
                if turns >= 3 { session_turns.push(turns); }
            }
        }

        let session_count = session_turns.len();
        let calibrated = session_count >= 5;

        if msg_lens.is_empty() {
            return Self::default();
        }

        msg_lens.sort_unstable();
        session_turns.sort_unstable();

        let p25_msg_len = percentile(&msg_lens, 0.25).max(15);
        let median_msg_len = percentile(&msg_lens, 0.50);
        let median_session_turns = if session_turns.is_empty() {
            15
        } else {
            percentile(&session_turns, 0.50).max(5)
        };
        let long_session_threshold = ((median_session_turns as f64) * 1.75) as usize;

        UserProfile {
            p25_msg_len,
            median_msg_len,
            median_session_turns,
            long_session_threshold,
            correction_threshold: p25_msg_len,
            calibrated,
            session_count,
        }
    }
}

fn try_compute_from_db(conn: &rusqlite::Connection) -> Option<UserProfile> {
    let lens: Vec<i64> = conn
        .prepare(
            "SELECT LENGTH(human_text_prefix) FROM turns WHERE LENGTH(TRIM(human_text_prefix)) > 0 LIMIT 20000",
        )
        .ok()?
        .query_map([], |r| r.get(0))
        .ok()?
        .collect::<Result<_, _>>()
        .ok()?;
    let mut turns_per: Vec<i64> = conn
        .prepare(
            "SELECT turn_count FROM sessions WHERE turn_count >= 3 LIMIT 5000",
        )
        .ok()?
        .query_map([], |r| r.get(0))
        .ok()?
        .collect::<Result<_, _>>()
        .ok()?;

    let session_count = turns_per.len();
    let calibrated = session_count >= 5;
    if lens.is_empty() {
        return None;
    }

    let mut msg_lens: Vec<usize> = lens.into_iter().map(|x| x as usize).collect();
    msg_lens.sort_unstable();
    turns_per.sort_unstable();
    let turn_usize: Vec<usize> = turns_per.into_iter().map(|x| x as usize).collect();

    let p25_msg_len = percentile(&msg_lens, 0.25).max(15);
    let median_msg_len = percentile(&msg_lens, 0.50);
    let median_session_turns = if turn_usize.is_empty() {
        15
    } else {
        percentile(&turn_usize, 0.50).max(5)
    };
    let long_session_threshold = ((median_session_turns as f64) * 1.75) as usize;

    Some(UserProfile {
        p25_msg_len,
        median_msg_len,
        median_session_turns,
        long_session_threshold,
        correction_threshold: p25_msg_len,
        calibrated,
        session_count,
    })
}

fn scan_file(content: &str) -> (Vec<usize>, usize) {
    let mut msg_lens: Vec<usize> = Vec::new();
    let mut turn_count = 0usize;
    let mut last_was_assistant = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let type_ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

        if type_ == "user" {
            let is_meta = v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false);
            if is_meta { continue; }
            if let Some(text) = extract_user_text(&v) {
                let len = text.trim().len();
                if len > 0 {
                    msg_lens.push(len);
                    if last_was_assistant { turn_count += 1; }
                }
                last_was_assistant = false;
            }
        } else if type_ == "assistant" {
            if let Some(msg) = v.get("message") {
                let model = msg.get("model").and_then(|x| x.as_str()).unwrap_or("");
                if !model.is_empty() && model != "<synthetic>" {
                    last_was_assistant = true;
                }
            }
        }
    }

    (msg_lens, turn_count)
}

fn extract_user_text(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(txt) = item.get("text").and_then(|v| v.as_str()) {
                    return Some(txt.to_string());
                }
            }
        }
    }
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    None
}

fn percentile(sorted: &[usize], pct: f64) -> usize {
    if sorted.is_empty() { return 0; }
    let idx = ((sorted.len() as f64 - 1.0) * pct) as usize;
    sorted[idx]
}
