//! The only boundary allowed to turn a validated MCP proposal into model-visible output.
//!
//! Applying is deliberately two phase: `prepare_mcp_trim` durably stores the exact original
//! before returning a shortened value; the platform adapter emits that value and only then calls
//! `mark_mcp_trim_emitted`. This keeps recovery fail-closed and applied telemetry truthful.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    render_validated_mcp_proposal, CanonicalMcpResult, McpStrategyManifest, McpTransformProposal,
    ToolContract, ValidatedMcpProposal,
};

pub struct McpApplyRequest<'a> {
    pub surface: &'a str,
    pub server_id: &'a str,
    pub protocol_version: &'a str,
    pub tool_name: &'a str,
    pub tool_input: &'a Value,
    pub session_id: Option<&'a str>,
    pub command_or_path: &'a str,
    pub contract: Option<&'a ToolContract>,
    pub manifest: &'a McpStrategyManifest,
    pub proposal: &'a McpTransformProposal,
    /// Permission comes from the existing evidence gate. Eligibility alone is never permission.
    pub authorized: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedMcpTrim {
    pub result: Value,
    pub rewind_id: String,
    pub validated: ValidatedMcpProposal,
    surface: String,
    server_id: String,
    protocol_version: String,
    tool_name: String,
    session_id: Option<String>,
    command_or_path: String,
    strategy_id: String,
    strategy_version: String,
    chars_in: usize,
    chars_out: usize,
    lines_total: usize,
    lines_keep: usize,
    prepared_at: String,
}

#[derive(Debug, Clone)]
pub enum McpPrepareOutcome {
    Ready(Box<PreparedMcpTrim>),
    PassThrough { result: Value, reason: String },
}

/// Production entry point. Any database or validation failure is fail-open and returns the exact
/// original result with a machine-readable reason.
pub fn prepare_mcp_trim(
    original: &CanonicalMcpResult,
    request: &McpApplyRequest<'_>,
) -> McpPrepareOutcome {
    let exact_original = original.raw().clone();
    let mut conn = match crate::db::open_db() {
        Ok(conn) => conn,
        Err(error) => {
            return McpPrepareOutcome::PassThrough {
                result: exact_original,
                reason: format!("recovery-store-unavailable: {error}"),
            }
        }
    };
    if let Err(error) = crate::db::ensure_schema(&conn) {
        return McpPrepareOutcome::PassThrough {
            result: exact_original,
            reason: format!("recovery-schema-unavailable: {error}"),
        };
    }
    match prepare_mcp_trim_in(&mut conn, original, request) {
        Ok(outcome) => outcome,
        Err(error) => McpPrepareOutcome::PassThrough {
            result: exact_original,
            reason: format!("apply-prepare-failed: {error}"),
        },
    }
}

/// Testable core of the prepare phase. The caller is responsible for ensuring the CTX schema.
pub fn prepare_mcp_trim_in(
    conn: &mut Connection,
    original: &CanonicalMcpResult,
    request: &McpApplyRequest<'_>,
) -> Result<McpPrepareOutcome> {
    if !request.authorized {
        return Ok(McpPrepareOutcome::PassThrough {
            result: original.raw().clone(),
            reason: "evidence-gate-not-authorized".into(),
        });
    }

    let (result, validated) = match render_validated_mcp_proposal(
        original,
        request.contract,
        Some(request.tool_input),
        request.manifest,
        request.proposal,
    ) {
        Ok(candidate) => candidate,
        Err(rejection) => {
            return Ok(McpPrepareOutcome::PassThrough {
                result: original.raw().clone(),
                reason: format!("proposal-rejected: {}", rejection.code()),
            })
        }
    };

    let original_json = serde_json::to_string(original.raw()).context("serialize original")?;
    let trimmed_json = serde_json::to_string(&result).context("serialize trimmed result")?;
    if trimmed_json.chars().count() >= original_json.chars().count() {
        return Ok(McpPrepareOutcome::PassThrough {
            result: original.raw().clone(),
            reason: "rendered-result-has-no-savings".into(),
        });
    }

    let rewind_id = rewind_id(
        request.server_id,
        request.tool_name,
        original_json.as_bytes(),
    );
    let prepared_at = chrono::Utc::now().to_rfc3339();
    let transaction = conn.transaction().context("start recovery transaction")?;
    crate::db::insert_rewind_checked(
        &transaction,
        &rewind_id,
        &prepared_at,
        request.session_id,
        request.tool_name,
        request.command_or_path,
        &original_json,
        &trimmed_json,
    )
    .context("persist exact original")?;
    transaction
        .commit()
        .context("commit recovery transaction")?;

    Ok(McpPrepareOutcome::Ready(Box::new(PreparedMcpTrim {
        result,
        rewind_id,
        validated,
        surface: request.surface.to_owned(),
        server_id: request.server_id.to_owned(),
        protocol_version: request.protocol_version.to_owned(),
        tool_name: request.tool_name.to_owned(),
        session_id: request.session_id.map(str::to_owned),
        command_or_path: request.command_or_path.to_owned(),
        strategy_id: request.manifest.id.to_owned(),
        strategy_version: request.manifest.version.to_owned(),
        chars_in: original_json.chars().count(),
        chars_out: trimmed_json.chars().count(),
        lines_total: original_json.lines().count(),
        lines_keep: trimmed_json.lines().count(),
        prepared_at,
    })))
}

/// Record an apply only after the adapter successfully wrote the shortened result to its native
/// transport. If this fails, recovery remains available and CTX under-counts rather than claiming
/// savings that were not delivered.
pub fn mark_mcp_trim_emitted(prepared: &PreparedMcpTrim) -> Result<()> {
    let mut conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    mark_mcp_trim_emitted_in(&mut conn, prepared)
}

fn mark_mcp_trim_emitted_in(conn: &mut Connection, prepared: &PreparedMcpTrim) -> Result<()> {
    let transaction = conn.transaction().context("start applied transaction")?;
    let features = json!({
        "adapter": "mcp-apply-v1",
        "server": prepared.server_id,
        "protocolVersion": prepared.protocol_version,
        "strategy": prepared.strategy_id,
        "strategyVersion": prepared.strategy_version,
        "replacements": prepared.validated.replacements,
        "structuredContentReplaced": prepared.validated.structured_content_replaced,
        "outputSchemaValidated": prepared.validated.output_schema_validated,
    });
    let features_json = serde_json::to_string(&features)?;
    let row = crate::db::CompressDecision {
        ts: &prepared.prepared_at,
        session_id: prepared.session_id.as_deref(),
        tool_name: &prepared.tool_name,
        server_prefix: Some(&prepared.server_id),
        kind: "mcp",
        task_mode: "gateway",
        lines_total: prepared.lines_total,
        lines_keep: prepared.lines_keep,
        lines_drop: prepared.lines_total.saturating_sub(prepared.lines_keep),
        chars_in: prepared.chars_in,
        would_chars_out: prepared.chars_out,
        features_json: &features_json,
        command_or_path: &prepared.command_or_path,
        applied: true,
        explore_arm: None,
        surface: Some(&prepared.surface),
    };
    crate::db::insert_compress_decision(&transaction, &row)?;
    let decision_id = transaction.last_insert_rowid();
    transaction.execute(
        "UPDATE compress_decisions SET rewind_id = ?2 WHERE id = ?1",
        params![decision_id, prepared.rewind_id],
    )?;
    crate::db::insert_compress_event(
        &transaction,
        &prepared.prepared_at,
        prepared.session_id.as_deref(),
        &prepared.tool_name,
        &format!("{}@{}", prepared.strategy_id, prepared.strategy_version),
        prepared.chars_in,
        prepared.chars_out,
        &prepared.command_or_path,
    )?;
    transaction.commit().context("commit applied transaction")?;
    Ok(())
}

fn rewind_id(server_id: &str, tool_name: &str, original: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ctx-mcp-rewind-v1\0");
    hash.update(server_id.as_bytes());
    hash.update([0]);
    hash.update(tool_name.as_bytes());
    hash.update([0]);
    hash.update(original);
    format!("mcp-{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_result::{parse_mcp_result, McpTextReplacement, McpTransformProposal};
    use serde_json::json;

    static MANIFEST: McpStrategyManifest = McpStrategyManifest {
        id: "test-text",
        version: "1",
        eligible_shape: "text",
        invariants: &["top-level-fields", "content-order", "non-text-blocks"],
        max_expansion_percent: 100,
    };

    fn schema(conn: &Connection) {
        crate::db::ensure_schema(conn).unwrap();
    }

    #[test]
    fn recovery_is_durable_before_adapter_can_emit() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let raw = json!({
            "content": [
                {"type":"text","text":"one\ntwo\nthree\nfour\nfive\nsix"},
                {"type":"image","data":"abc","mimeType":"image/png","vendor":7}
            ],
            "_meta": {"trace":"kept"}
        });
        let canonical = parse_mcp_result(&raw).unwrap();
        let proposal = McpTransformProposal {
            strategy_id: "test-text",
            strategy_version: "1",
            max_total_text_chars: 7,
            replacements: vec![McpTextReplacement {
                block_index: 0,
                expected_text: "one\ntwo\nthree\nfour\nfive\nsix".into(),
                replacement: "one\nsix".into(),
            }],
            structured_content: None,
        };
        let input = json!({});
        let request = McpApplyRequest {
            surface: "codex",
            server_id: "fixture",
            protocol_version: "2025-11-25",
            tool_name: "mcp__fixture__read",
            tool_input: &input,
            session_id: Some("session"),
            command_or_path: "fixture/read",
            contract: None,
            manifest: &MANIFEST,
            proposal: &proposal,
            authorized: true,
        };
        let McpPrepareOutcome::Ready(prepared) =
            prepare_mcp_trim_in(&mut conn, &canonical, &request).unwrap()
        else {
            panic!("expected ready trim")
        };
        let stored: String = conn
            .query_row(
                "SELECT original FROM rewind_store WHERE id=?1",
                params![prepared.rewind_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), raw);
        assert_eq!(prepared.result["content"][1], raw["content"][1]);
        let applied: i64 = conn
            .query_row("SELECT COUNT(*) FROM compress_decisions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied, 0, "prepare must not claim model-visible delivery");

        mark_mcp_trim_emitted_in(&mut conn, &prepared).unwrap();
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM compress_decisions WHERE applied=1 AND rewind_id=?1",
                params![prepared.rewind_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn unauthorized_apply_is_exact_pass_through_without_recovery_write() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let raw = json!({"content":[{"type":"text","text":"one two three"}]});
        let canonical = parse_mcp_result(&raw).unwrap();
        let proposal = McpTransformProposal {
            strategy_id: "test-text",
            strategy_version: "1",
            max_total_text_chars: 3,
            replacements: vec![McpTextReplacement {
                block_index: 0,
                expected_text: "one two three".into(),
                replacement: "one".into(),
            }],
            structured_content: None,
        };
        let input = json!({});
        let request = McpApplyRequest {
            surface: "codex",
            server_id: "fixture",
            protocol_version: "v",
            tool_name: "mcp__fixture__read",
            tool_input: &input,
            session_id: None,
            command_or_path: "fixture/read",
            contract: None,
            manifest: &MANIFEST,
            proposal: &proposal,
            authorized: false,
        };
        let McpPrepareOutcome::PassThrough { result, reason } =
            prepare_mcp_trim_in(&mut conn, &canonical, &request).unwrap()
        else {
            panic!("expected pass through")
        };
        assert_eq!(result, raw);
        assert_eq!(reason, "evidence-gate-not-authorized");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rewind_store", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
