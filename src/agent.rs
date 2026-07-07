//! Act 3: agent-agnostic controller interface.
//!
//! The retention controller is transport-independent: it takes a tool result and a task
//! context and returns a decision. The platform's compaction only helps one agent; one
//! learned model keyed by **repo + task** (not by agent) is the portfolio no single
//! vendor will build. This module abstracts the per-agent plumbing behind one trait so
//! Claude Code, Cursor, and Codex all drive the same brain.
//!
//! `ClaudeCodeTransport` is the reference implementation; it reuses the existing
//! PostToolUse extract/wrap helpers. New agents implement `AgentTransport` and get the
//! same shadow collection, evidence gate, and learned model for free.

use serde_json::Value;

use crate::compress::{self, ShadowDecision};
use crate::config::Config;

/// A tool result lifted out of an agent's native payload shape.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_name: String,
    pub tool_input: Value,
    pub raw_output: String,
    pub session_id: Option<String>,
    pub cwd: String,
    /// The agent's most recent readable narration, lifted from the session transcript by the
    /// transport (ADR 0004 / CTX-11). `None` on surfaces that do not persist a transcript or when
    /// no readable narration is available. Used by the read guard to protect declared working reads.
    pub recent_intent_text: Option<String>,
}

/// What the controller decided for one tool result, independent of agent.
#[derive(Debug, Clone)]
pub struct ControllerDecision {
    pub shadow: Option<ShadowDecision>,
    /// Whether user-facing compression applies (preset allows the kind AND the tool
    /// cleared its evidence gate).
    pub apply: bool,
    pub kind_label: String,
    /// Phase 2 exploration arm (ADR 0009), set only when this decision entered the randomized
    /// experiment: "treatment" (trimmed) or "control" (deliberately kept). `None` means the
    /// decision was not part of the experiment, so it must not be read as a clean control.
    pub explore_arm: Option<&'static str>,
}

/// One agent's plumbing: how to read a tool result out of its payload, and how to put a
/// compressed result back into the shape that agent validates.
pub trait AgentTransport {
    fn agent_name(&self) -> &'static str;
    fn extract(&self, payload: &Value) -> Option<ToolResult>;
    fn wrap(&self, tool_name: &str, original: &Value, compressed: &str) -> Value;
}

/// Compute the controller decision for a tool result. Pure with respect to the agent:
/// every transport runs the same shadow computation, preset check, and evidence gate.
pub fn decide(cfg: &Config, tr: &ToolResult) -> ControllerDecision {
    decide_inner(cfg, tr, explore_unit_draw())
}

/// A uniform draw in [0, 1) for exploration assignment. Dependency-free (no `rand` crate, per the
/// local-first constraints in ADR 0008): mix the high-resolution clock so successive decisions get
/// independent draws. Good enough for randomized assignment; we are not doing cryptography.
fn explore_unit_draw() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs().wrapping_mul(2654435761)))
        .unwrap_or(0);
    // SplitMix64-style finalizer to decorrelate the low bits of the timestamp.
    let mut z = nanos.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn decide_inner(cfg: &Config, tr: &ToolResult, explore_draw: f64) -> ControllerDecision {
    let mut shadow = compress::compute_shadow_decision(
        &tr.tool_name,
        &tr.tool_input,
        &tr.raw_output,
        cfg,
        tr.session_id.as_deref(),
        &tr.cwd,
    );
    let kind_label = shadow
        .as_ref()
        .map(|d| d.kind_str().to_string())
        .unwrap_or_else(|| "generic".to_string());
    // Edit/Write confirmations go through the same gate as every other tool (CTX-62). Only their
    // echo is trimmed, by the edit strategy that collapses the giant minified lines the agent cannot
    // read anyway; the file and the change itself are untouched, and ctx_expand recovers the full
    // echo on demand. Like any tool, an edit trim applies only under an explicit trial or once it has
    // earned activation, and edit tools are judged by re-edit rather than re-read, so a trim earns
    // only if re-edits do not rise. The old blanket "edits are never trimmed" rule is gone.

    // A deliberate trial (`compress_trial_tools`) trims the chosen tool live even while the preset
    // stays off and the evidence gate is unmet. Otherwise the autopilot path: the preset must allow
    // the kind AND the tool must either have earned activation OR be in automatic burn-in (ADR 0012
    // / CTX-23), the bounded on-ramp that lets a tool with a clean baseline build its "after" arm.
    // Burn-in respects the preset, so it never trims when autopilot is off.
    let activated = compress::activation::tool_activated(cfg, &tr.tool_name, &kind_label);
    let base_apply = cfg.compress_trialing(&tr.tool_name)
        || (cfg.compress_applies_kind(&kind_label)
            && (activated || compress::activation::tool_in_burn_in(cfg, &tr.tool_name)));

    let is_read = kind_label == "read";
    let read_path = read_file_path(&tr.tool_input);

    // Edit-intent guard (ADR 0001 / CTX-8): a Read only applies for reference reads (files the
    // agent is not positioned to edit). Working reads of editable project files are never trimmed,
    // even under a trial or after activation, so a re-trial cannot re-create the observed harm.
    let static_guard_blocks = cfg.compress_read_edit_guard
        && is_read
        && !compress::edit_intent::read_is_trim_eligible(read_path, &tr.cwd);

    // Intent signal (ADR 0004 / CTX-11): read the agent's recent narration and, when it shows
    // edit-intent for this file, protect the read even if the static path heuristic would trim it
    // (e.g. a reference path the agent has declared it will edit). Purely additive: it can only
    // protect, never trim more. The signal is recorded in shadow features for prevalence.
    let intent_blocks = if cfg.compress_intent_log && is_read {
        let intent =
            compress::intent::IntentSignal::from_text(tr.recent_intent_text.as_deref(), read_path);
        let blocks = intent.edit_intent_for_path();
        if let Some(d) = shadow.as_mut() {
            d.features.intent = Some(intent);
        }
        blocks
    } else {
        false
    };

    let read_guard_blocks = static_guard_blocks || intent_blocks;
    // Record the protection so the activity feed can tell a deliberately-kept read apart from one
    // that is merely still being watched. Only set it for reads the guard actually held back; an
    // unprotected read leaves the flag absent (None).
    if read_guard_blocks {
        if let Some(d) = shadow.as_mut() {
            d.features.read_protected = Some(true);
        }
    }

    // Per-decision model groundwork (ADR 0007 / CTX-16): record the repo key and file extension,
    // and what the served retention model *would* predict for this decision. Logging only: none of
    // this touches `apply` in this phase. It is the data the per-decision model needs to be trained
    // and proven per repo before it is ever allowed to steer a trim.
    if let Some(d) = shadow.as_mut() {
        d.features.repo_key = repo_key_for(&tr.cwd);
        // Tag decisions made inside ctx's own source repo so the corpus queries can exclude them
        // (CTX-32). Developing ctx is not user behavior; left in, it dominates and biases the gate.
        d.features.self_dev = is_self_dev_repo(&tr.cwd).then_some(true);
        d.features.file_ext = read_file_path(&tr.tool_input).and_then(file_ext_of);
        if is_read {
            d.features.path_role = read_file_path(&tr.tool_input)
                .and_then(|p| path_role_of(p).map(str::to_string));
        }
        let features_json = d.features_json();
        if let Some(score) = crate::learn::score_parts(
            &kind_label,
            d.lines_total as i64,
            d.lines_drop as i64,
            &features_json,
        ) {
            d.features.model_score = Some(score);
            d.features.would_model_apply = Some(score < crate::learn::LEARNED_ACT_THRESHOLD);
        }
    }

    // Model proposal (ADR 0032 / CTX-46 increment 3), default off. When the flag is on, the
    // file-aware model may *propose* trimming a working read the static guard would otherwise hold
    // back, but only for a read, only when the model beat its kind-only twin, only in a repo that
    // cleared its own label gate, and only when it is confident the read is throwaway. This can
    // unblock the read guard; it can never satisfy `base_apply`, so a proposal alone never trims:
    // the preset, burn-in, and causal activation gate still govern (see `trim_eligible` below).
    let model_proposes = cfg.compress_model_propose
        && is_read
        && read_guard_blocks
        && shadow
            .as_ref()
            .map(|d| {
                crate::learn::model_proposes_safe_trim(
                    &kind_label,
                    d.lines_total as i64,
                    d.lines_drop as i64,
                    &d.features_json(),
                    d.features.repo_key.as_deref(),
                )
            })
            .unwrap_or(false);
    if model_proposes {
        if let Some(d) = shadow.as_mut() {
            d.features.model_proposed = Some(true);
        }
    }

    // A proposal can only lift the read guard. `base_apply` (preset + activation/burn-in, and never
    // an edit tool) is still required, so the model alone can never make a trim apply.
    let trim_eligible = base_apply && (!read_guard_blocks || model_proposes);
    // Only decisions that would actually drop lines are worth experimenting on; keeping a no-op
    // "trim" tells us nothing, so it never enters the experiment.
    let would_drop = shadow.as_ref().map(|d| d.lines_drop > 0).unwrap_or(false);

    // Phase 2 randomized exploration (ADR 0009 / CTX-15). Among decisions that would really trim,
    // with probability `compress_explore_rate` withhold the trim and tag the row as a control
    // sample; the rest are treatment. Both arms come from the same eligible pool by random
    // assignment, so comparing their outcomes is an unbiased per-tool estimate of trimming on the
    // user's own work. Exploration can only ever withhold a trim, never add one.
    let mut apply = trim_eligible;
    let mut explore_arm: Option<&'static str> = None;
    let explore_rate = explore_rate_for(&kind_label, cfg);
    // Once a tool has earned activation, stop the control arm: it trims every would-trim run and
    // reclaims the full amount, paying no exploration tax. The causal verdict is still recomputed
    // each pass from the now-frozen baseline against the always-fresh trimmed arm, so a tool that
    // drifts flips back off activation, which re-enables exploration and re-validates it. Reversion
    // self-heals; earned is not a latch.
    if trim_eligible && would_drop && explore_rate > 0.0 && !activated {
        if explore_draw < explore_rate {
            explore_arm = Some("control");
            apply = false;
        } else {
            explore_arm = Some("treatment");
        }
    }

    ControllerDecision {
        shadow,
        apply,
        kind_label,
        explore_arm,
    }
}

/// The file path a Read/Edit tool input points at, used by the edit-intent guard. Mirrors the
/// extraction in `compress::shadow::compute_shadow_decision` (file_path, then path).
fn read_file_path(tool_input: &Value) -> Option<&str> {
    tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str())
}

/// Coarse file role for a read path (`src`, `test`, `config`, `generated`, `vendored`, `docs`).
/// Single public name for ADR 0030 / CTX-45; the classifier lives in `compress::path_role`.
pub fn path_role_of(path: &str) -> Option<&'static str> {
    crate::compress::path_role::path_role_of(path)
}

fn explore_rate_for(kind_label: &str, cfg: &Config) -> f64 {
    // Reads and edits are the working-file tools that only ever trim under a trial or activation, so
    // without a control (left-untrimmed) arm they run 100% trimmed and can never build the baseline
    // the causal gate compares against. They share the same explore slice so an edit trial can
    // actually resolve (CTX-62), instead of reaching its trimmed target with nothing to compare.
    if kind_label == "read" || kind_label == "edit" {
        cfg.compress_explore_read_rate
    } else {
        cfg.compress_explore_rate
    }
}

/// Lowercased file extension for a path, when it is a plausible one (non-empty, short). Used as a
/// cheap contextual feature for the per-decision model (ADR 0007). Returns None for extensionless
/// paths or dotfiles.
fn file_ext_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty()
        || ext.is_empty()
        || ext.len() > 12
        || !ext.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// A stable key for the repo a decision happened in: the nearest ancestor of `cwd` that contains a
/// `.git`, else `cwd` itself. Lets the per-decision model later be trained and reported per repo
/// (ADR 0007). Local-only, stored in the user's own decision log. None for an empty cwd.
fn repo_key_for(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let mut dir = std::path::Path::new(cwd);
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_string_lossy().into_owned());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    Some(cwd.to_string())
}

/// True when `cwd` is inside ctx's own source repo, identified by content rather than path: the
/// nearest `.git` ancestor (the repo root) has a `Cargo.toml` whose `[package].name` is `ctx`
/// (CTX-32). Content-based so it holds on any machine and checkout path. Used to keep ctx's own
/// development churn out of the learning/gate/audit corpus. A missing or unreadable Cargo.toml, or
/// any other package name, returns false, so the default is to keep the decision. Also reused by the
/// one-time historical backfill in `db`, which passes a stored `repo_key` (already a repo root).
pub(crate) fn is_self_dev_repo(cwd: &str) -> bool {
    if cwd.is_empty() {
        return false;
    }
    let mut dir = std::path::Path::new(cwd);
    loop {
        if dir.join(".git").exists() {
            return cargo_package_is_ctx(&dir.join("Cargo.toml"));
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return false,
        }
    }
}

/// True when a `Cargo.toml` declares `[package] name = "ctx"`. Parsed with the toml crate so a
/// dependency or workspace member that merely mentions `ctx` cannot trigger a false match.
fn cargo_package_is_ctx(cargo_toml: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    text.parse::<toml::Value>()
        .ok()
        .and_then(|v| {
            v.get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .map(|n| n == "ctx")
        })
        .unwrap_or(false)
}

/// Reference transport for Claude Code PostToolUse payloads. Delegates to the existing
/// extract/wrap helpers so there is a single source of truth for the wire shape.
pub struct ClaudeCodeTransport;

impl AgentTransport for ClaudeCodeTransport {
    fn agent_name(&self) -> &'static str {
        "claude-code"
    }

    fn extract(&self, payload: &Value) -> Option<ToolResult> {
        let tool_name = payload
            .get("tool_name")
            .or_else(|| payload.get("toolName"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tool_input = payload
            .get("tool_input")
            .or_else(|| payload.get("toolInput"))
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let response = compress::tool_response_value(payload)?;
        let raw_output = compress::extract_compressible_text(&tool_name, &response);
        if raw_output.is_empty() {
            return None;
        }
        let session_id = payload
            .get("session_id")
            .or_else(|| payload.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let cwd = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Lift the agent's most recent narration from the transcript the payload points at, so the
        // read guard can act on declared edit-intent. Works for both Claude Code and Cursor
        // transcripts (ADR 0011 / CTX-21). Best-effort: None on any failure.
        let recent_intent_text = compress::intent::recent_intent_text_for_payload(payload);
        Some(ToolResult {
            tool_name,
            tool_input,
            raw_output,
            session_id,
            cwd,
            recent_intent_text,
        })
    }

    fn wrap(&self, tool_name: &str, original: &Value, compressed: &str) -> Value {
        compress::wrap_updated_tool_output(tool_name, original, compressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_transport_extracts_bash_output() {
        let t = ClaudeCodeTransport;
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": {"stdout": "on branch main", "stderr": ""},
            "cwd": "/proj",
            "session_id": "s1"
        });
        let tr = t.extract(&payload).expect("extract");
        assert_eq!(tr.tool_name, "Bash");
        assert_eq!(tr.cwd, "/proj");
        assert!(tr.raw_output.contains("branch"));
    }

    #[test]
    fn decision_is_shadow_only_when_preset_off() {
        let cfg = Config {
            compress_preset: crate::config::CompressPreset::Off,
            ..Default::default()
        };
        let tr = ToolResult {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "git status"}),
            raw_output: "a\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
            recent_intent_text: None,
        };
        let d = decide(&cfg, &tr);
        assert!(!d.apply, "preset off must never apply");
        assert!(d.shadow.is_some());
    }

    #[test]
    fn trial_tool_applies_even_with_preset_off() {
        // A deliberate trial trims the chosen tool live to collect the "after" arm, even though
        // the preset is off and the tool has not earned the evidence gate.
        let cfg = Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec!["Bash".into()],
            ..Default::default()
        };
        let tr = ToolResult {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "git status"}),
            raw_output: "a\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
            recent_intent_text: None,
        };
        let d = decide(&cfg, &tr);
        assert!(d.apply, "a trialed tool must apply even with preset off");

        // A tool that is not under trial stays shadow-only on preset off.
        let other = ToolResult {
            tool_name: "Read".into(),
            ..tr.clone()
        };
        assert!(
            !decide(&cfg, &other).apply,
            "non-trialed tools must stay shadow only"
        );
    }

    #[test]
    fn edit_tools_trim_under_trial_like_any_tool() {
        // CTX-62: the old blanket "edits are never trimmed" rule is gone. An edit tool now goes
        // through the same gate as any tool: only its echo is trimmed (by the edit strategy), it
        // applies under an explicit trial, and it stays shadow-only when neither trialed nor earned.
        let trialed = Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec!["Write".into(), "Edit".into()],
            ..Default::default()
        };
        let untrialed = Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec![],
            ..Default::default()
        };
        for tool in ["Write", "Edit"] {
            let tr = ToolResult {
                tool_name: tool.into(),
                tool_input: json!({"file_path": "/proj/src/foo.rs"}),
                raw_output: "changed line\n".repeat(500),
                session_id: None,
                cwd: "/proj".into(),
                recent_intent_text: None,
            };
            let d = decide(&trialed, &tr);
            assert!(d.apply, "{tool} trims under an explicit trial, like any tool");
            assert!(
                d.shadow.is_some(),
                "{tool} is still recorded for the edit-follow timeline"
            );
            assert!(
                !decide(&untrialed, &tr).apply,
                "{tool} stays shadow-only when it is neither trialed nor earned"
            );
        }
    }

    /// Write a served model that would propose trimming any read in repo `/proj`, so the propose
    /// path is live. Returns nothing; the model lands at the CTX_HOME-scoped model path.
    fn write_proposing_model() {
        let n = crate::learn::feature_count();
        let model = crate::learn::RetentionModel {
            version: 3,
            trained_at: "2026-06-19T00:00:00Z".into(),
            target: "needed_whole".into(),
            n_train: 200,
            n_holdout: 40,
            feature_names: vec!["f".into(); n],
            // Large negative bias drives P(needed whole) far below the act threshold, so the model
            // is maximally confident the read is throwaway.
            weights: vec![0.0; n],
            bias: -6.0,
            holdout_auc: 0.72,
            kind_only_auc: 0.64,
            file_aware_wins: true,
            holdout_accuracy: 0.7,
            base_correction_rate: 0.05,
            base_need_rate: 0.4,
            per_tool: vec![],
            per_repo: vec![crate::learn::PerRepoReadiness {
                repo_key: "/proj".into(),
                joined: 120,
                positives: 50,
                ready: true,
            }],
        };
        crate::config::ensure_dir().unwrap();
        let json = serde_json::to_value(&model).unwrap();
        crate::config::write_json_atomic(&crate::config::retention_model_path(), &json).unwrap();
    }

    #[test]
    fn a_model_proposal_can_lift_the_read_guard_but_never_applies_without_base_apply() {
        // The increment-3 contract (ADR 0032): the file-aware model may *propose* trimming a
        // working read the guard would keep, but a proposal alone can never trim. `base_apply`
        // (preset + activation/burn-in) is still required, and the flag is off by default.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        write_proposing_model();

        // Preset OFF and the read is not trialed: base_apply is false. Even with the propose flag
        // on and a maximally-confident proposing model, the read must not trim.
        let cfg_off = Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_read_edit_guard: true,
            compress_model_propose: true,
            ..Default::default()
        };
        let d = decide(&cfg_off, &read_tr("src/foo.rs"));
        assert!(
            !d.apply,
            "a model proposal must never apply without base_apply (preset off)"
        );

        // Now the read is trialed, so base_apply is true. The guard would still block this working
        // read, but the proposal lifts it: with the flag on, the read trims.
        let cfg_trial_on = Config {
            compress_model_propose: true,
            ..read_trial_cfg(true)
        };
        let d_on = decide(&cfg_trial_on, &read_tr("src/foo.rs"));
        assert!(
            d_on.apply,
            "with base_apply met, a confident proposal should lift the guard"
        );
        assert_eq!(
            d_on.shadow.as_ref().and_then(|s| s.features.model_proposed),
            Some(true),
            "the lifted-guard proposal must be recorded for audit"
        );

        // Same trialed read, flag OFF: the guard holds and nothing trims. Proves the proposal, not
        // some other path, is what lifted the guard above.
        let cfg_trial_off = read_trial_cfg(true);
        let d_off = decide(&cfg_trial_off, &read_tr("src/foo.rs"));
        assert!(
            !d_off.apply,
            "with the propose flag off, the guard must still block the working read"
        );

        std::env::remove_var("CTX_HOME");
    }

    fn read_trial_cfg(guard: bool) -> Config {
        Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec!["Read".into()],
            compress_read_edit_guard: guard,
            ..Default::default()
        }
    }

    fn read_tr(file_path: &str) -> ToolResult {
        ToolResult {
            tool_name: "Read".into(),
            tool_input: json!({ "file_path": file_path }),
            raw_output: "line\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
            recent_intent_text: None,
        }
    }

    #[test]
    fn edit_guard_blocks_trimming_a_project_read_even_under_trial() {
        // The exact harm from CTX-8: a trialed Read of an editable project file must not trim.
        let cfg = read_trial_cfg(true);
        let d = decide(&cfg, &read_tr("src/foo.rs"));
        assert!(
            !d.apply,
            "guard must block trimming an editable project read"
        );
        assert!(
            d.shadow.is_some(),
            "shadow decision is still recorded (would-trim)"
        );
    }

    #[test]
    fn edit_guard_allows_trimming_a_reference_read_under_trial() {
        let cfg = read_trial_cfg(true);
        let d = decide(&cfg, &read_tr("/proj/node_modules/react/index.js"));
        assert!(
            d.apply,
            "reference reads stay trim-eligible under the guard"
        );
    }

    #[test]
    fn edit_guard_off_restores_pre_guard_trial_behavior() {
        let cfg = read_trial_cfg(false);
        let d = decide(&cfg, &read_tr("src/foo.rs"));
        assert!(
            d.apply,
            "with the guard off, a trial trims regardless of edit intent"
        );
    }

    fn read_tr_narration(file_path: &str, narration: &str) -> ToolResult {
        ToolResult {
            recent_intent_text: Some(narration.to_string()),
            ..read_tr(file_path)
        }
    }

    #[test]
    fn intent_protects_a_reference_read_under_trial() {
        // The static guard would let this vendored path trim, but the agent's recent narration
        // declares it is about to edit that exact file, so the intent signal protects it.
        let cfg = Config {
            compress_intent_log: true,
            ..read_trial_cfg(true)
        };
        let tr = read_tr_narration(
            "/proj/vendor/dep/widget.rs",
            "I'll patch widget.rs to fix the off-by-one before moving on.",
        );
        let d = decide(&cfg, &tr);
        assert!(
            !d.apply,
            "declared edit-intent must protect even a reference read"
        );
        let recorded = d
            .shadow
            .as_ref()
            .and_then(|s| s.features.intent.clone())
            .expect("intent signal must be recorded in features");
        assert!(recorded.has_text);
        assert!(recorded.mentions_path);
        assert!(recorded.has_edit_verb);
        assert!(recorded.edit_intent_for_path());
    }

    #[test]
    fn reference_read_without_edit_intent_still_trims_and_records_components() {
        let cfg = Config {
            compress_intent_log: true,
            ..read_trial_cfg(true)
        };
        let tr = read_tr_narration(
            "/proj/vendor/dep/widget.rs",
            "Let me read widget.rs just to understand the API shape.",
        );
        let d = decide(&cfg, &tr);
        assert!(d.apply, "a pure reference read should still trim");
        let recorded = d
            .shadow
            .as_ref()
            .and_then(|s| s.features.intent.clone())
            .expect("signal ran, so components are recorded even when intent is false");
        assert!(recorded.has_text, "narration was present");
        assert!(
            !recorded.has_edit_verb,
            "no edit verb in a pure reference read"
        );
        assert!(!recorded.edit_intent_for_path());
    }

    #[test]
    fn intent_signal_off_leaves_reference_read_trimming() {
        // With the signal disabled, declared edit-intent has no effect; static guard rules.
        let cfg = Config {
            compress_intent_log: false,
            ..read_trial_cfg(true)
        };
        let tr = read_tr_narration("/proj/vendor/dep/widget.rs", "I'll edit widget.rs next.");
        let d = decide(&cfg, &tr);
        assert!(
            d.apply,
            "with the intent signal off, reference reads trim as before"
        );
        let recorded = d.shadow.as_ref().and_then(|s| s.features.intent.clone());
        assert!(recorded.is_none(), "disabled signal records nothing");
    }

    #[test]
    fn file_ext_is_lowercased_and_filtered() {
        assert_eq!(file_ext_of("src/Foo.RS").as_deref(), Some("rs"));
        assert_eq!(file_ext_of("a/b/c.tsx").as_deref(), Some("tsx"));
        assert_eq!(file_ext_of("README").as_deref(), None, "no extension");
        assert_eq!(
            file_ext_of(".gitignore").as_deref(),
            None,
            "dotfile, empty stem"
        );
        assert_eq!(file_ext_of("archive.tar.gz").as_deref(), Some("gz"));
    }

    #[test]
    fn repo_key_falls_back_to_cwd_without_git() {
        assert_eq!(repo_key_for(""), None);
        // A path with no .git anywhere up the tree resolves to the cwd itself.
        assert_eq!(
            repo_key_for("/nonexistent/ctx-test/deep/dir").as_deref(),
            Some("/nonexistent/ctx-test/deep/dir")
        );
    }

    #[test]
    fn self_dev_repo_detected_by_package_name_not_path() {
        let tmp = std::env::temp_dir().join(format!("ctx-selfdev-{}", std::process::id()));
        let nested = tmp.join("src/compress");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(tmp.join(".git")).unwrap();

        // No Cargo.toml yet: a git repo that is not ctx is not self-dev.
        assert!(!is_self_dev_repo(nested.to_str().unwrap()));

        // A Cargo.toml whose package is ctx marks the whole tree as self-dev, from any depth.
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"ctx\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert!(is_self_dev_repo(tmp.to_str().unwrap()));
        assert!(is_self_dev_repo(nested.to_str().unwrap()));

        // A different package, even one that mentions ctx as a dependency, is not self-dev.
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"my-app\"\n\n[dependencies]\nctx = \"1\"\n",
        )
        .unwrap();
        assert!(!is_self_dev_repo(nested.to_str().unwrap()));

        // Empty cwd is never self-dev.
        assert!(!is_self_dev_repo(""));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_decisions_log_path_role_for_training() {
        let d = decide(&Config::default(), &read_tr("src/components/App.tsx"));
        assert_eq!(
            d.shadow
                .as_ref()
                .and_then(|s| s.features.path_role.as_deref()),
            Some("src")
        );
        let vendored = decide(
            &Config::default(),
            &read_tr("/proj/node_modules/react/index.js"),
        );
        assert_eq!(
            vendored
                .shadow
                .as_ref()
                .and_then(|s| s.features.path_role.as_deref()),
            Some("vendored")
        );
    }

    // A trialed reference read of 500 lines is trim-eligible and actually drops lines, so it is a
    // clean fixture for the Phase 2 exploration tests (ADR 0009).
    fn explore_read_cfg(rate: f64) -> Config {
        Config {
            compress_explore_read_rate: rate,
            ..read_trial_cfg(true)
        }
    }

    fn reference_read() -> ToolResult {
        read_tr("/proj/node_modules/react/index.js")
    }

    #[test]
    fn exploration_premise_reference_read_would_drop() {
        // Guards the fixture: if this read stops dropping lines, the exploration tests below would
        // silently pass for the wrong reason.
        let cfg = explore_read_cfg(0.0);
        let d = decide(&cfg, &reference_read());
        let drop = d.shadow.as_ref().map(|s| s.lines_drop).unwrap_or(0);
        assert!(drop > 0, "fixture must actually drop lines, got {drop}");
    }

    #[test]
    fn exploration_assigns_control_when_draw_below_rate() {
        let cfg = explore_read_cfg(0.20);
        let d = decide_inner(&cfg, &reference_read(), 0.05);
        assert_eq!(d.explore_arm, Some("control"));
        assert!(
            !d.apply,
            "a control sample withholds the trim to observe the kept outcome"
        );
    }

    #[test]
    fn exploration_assigns_treatment_when_draw_above_rate() {
        let cfg = explore_read_cfg(0.20);
        let d = decide_inner(&cfg, &reference_read(), 0.80);
        assert_eq!(d.explore_arm, Some("treatment"));
        assert!(d.apply, "a treatment sample still trims as normal");
    }

    #[test]
    fn exploration_off_leaves_no_arm_and_trims() {
        let cfg = explore_read_cfg(0.0);
        let d = decide_inner(&cfg, &reference_read(), 0.01);
        assert_eq!(d.explore_arm, None, "rate 0 disables the experiment");
        assert!(d.apply, "an eligible trial trims as before");
    }

    #[test]
    fn exploration_gives_edits_a_control_arm_too() {
        // An edit on trial with an explore slice must sometimes be left untrimmed, so it can build
        // the baseline the causal gate compares against (CTX-62). Without it, edits run 100% trimmed
        // and the trial can never resolve.
        let cfg = Config {
            compress_trial_tools: vec!["Edit".into()],
            compress_explore_read_rate: 0.20,
            ..read_trial_cfg(true)
        };
        let edit = ToolResult {
            tool_name: "Edit".into(),
            tool_input: json!({
                "file_path": "/proj/src/foo.rs",
                "old_string": "let a = 1",
                "new_string": "let a = 2",
            }),
            raw_output: "some edited line of content here\n".repeat(600),
            session_id: None,
            cwd: "/proj".into(),
            recent_intent_text: None,
        };
        let control = decide_inner(&cfg, &edit, 0.05);
        assert_eq!(control.explore_arm, Some("control"));
        assert!(!control.apply, "a control-arm edit is left untrimmed to build the baseline");
        let treatment = decide_inner(&cfg, &edit, 0.80);
        assert_eq!(treatment.explore_arm, Some("treatment"));
        assert!(treatment.apply, "the rest still trim on trial");
    }

    #[test]
    fn exploration_never_touches_guarded_reads() {
        // A guarded project read is not trim-eligible, so it must never enter the experiment, no
        // matter the draw. Exploration can only ever withhold a trim ctx would otherwise make.
        let cfg = explore_read_cfg(0.20);
        let d = decide_inner(&cfg, &read_tr("src/foo.rs"), 0.01);
        assert_eq!(d.explore_arm, None);
        assert!(!d.apply);
    }

    #[test]
    fn explore_unit_draw_stays_in_unit_interval() {
        for _ in 0..2000 {
            let u = explore_unit_draw();
            assert!((0.0..1.0).contains(&u), "draw {u} out of range");
        }
    }
}
