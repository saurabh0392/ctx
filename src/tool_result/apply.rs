//! The only boundary allowed to turn a validated MCP proposal into model-visible output.
//!
//! Applying is deliberately two phase: `prepare_mcp_trim` durably stores the exact original
//! before returning a shortened value; the platform adapter emits that value and only then calls
//! `mark_mcp_trim_emitted`. This keeps recovery fail-closed and applied telemetry truthful.

use anyhow::{Context, Result};
#[cfg(test)]
use rusqlite::params;
use rusqlite::{Connection, OptionalExtension};
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
    /// Gateway round-trip latency at the point the response became available. Native hooks leave
    /// this `None`; the gateway uses it for product-proof receipts.
    pub transport_latency_ms: Option<u64>,
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
    transport_latency_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum McpPrepareOutcome {
    Ready(Box<PreparedMcpTrim>),
    PassThrough { result: Value, reason: String },
}

/// A transport-neutral text replacement request. Protocol adapters identify the exact leaf; this
/// boundary owns durable recovery and truthful accepted-delivery accounting.
pub struct TextApplyRequest<'a> {
    pub surface: &'a str,
    pub route_id: &'a str,
    pub protocol_version: &'a str,
    pub tool_name: &'a str,
    pub session_id: Option<&'a str>,
    pub command_or_path: &'a str,
    pub kind: &'a str,
    pub strategy: &'a str,
    pub original: &'a str,
    pub replacement: &'a str,
    pub authorized: bool,
    pub transport_latency_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PreparedTextTrim {
    pub replacement: String,
    pub rewind_id: String,
    surface: String,
    route_id: String,
    protocol_version: String,
    tool_name: String,
    session_id: Option<String>,
    command_or_path: String,
    kind: String,
    strategy: String,
    chars_in: usize,
    chars_out: usize,
    lines_total: usize,
    lines_keep: usize,
    prepared_at: String,
    transport_latency_ms: Option<u64>,
}

impl PreparedTextTrim {
    /// Content-free dimensions for an acceptance receipt. The original and replacement text stay
    /// private to the atomic apply path.
    pub fn character_receipt(&self) -> (usize, usize) {
        (self.chars_in, self.chars_out)
    }
}

#[derive(Debug, Clone)]
pub enum TextPrepareOutcome {
    Ready(Box<PreparedTextTrim>),
    PassThrough { reason: &'static str },
}

/// Production prepare entry point. Any persistence failure returns pass-through; it never exposes
/// a replacement whose original is not already durable.
pub fn prepare_text_trim(request: &TextApplyRequest<'_>) -> TextPrepareOutcome {
    let mut conn = match crate::db::open_db() {
        Ok(conn) => conn,
        Err(_) => {
            return TextPrepareOutcome::PassThrough {
                reason: "recovery-store-unavailable",
            }
        }
    };
    if crate::db::ensure_schema(&conn).is_err() {
        return TextPrepareOutcome::PassThrough {
            reason: "recovery-schema-unavailable",
        };
    }
    match prepare_text_trim_in(&mut conn, request) {
        Ok(outcome) => outcome,
        Err(_) => TextPrepareOutcome::PassThrough {
            reason: "apply-prepare-failed",
        },
    }
}

/// Testable core for a protocol-neutral prepare. Repeated identical input produces the same rewind
/// id and replacement bytes.
pub fn prepare_text_trim_in(
    conn: &mut Connection,
    request: &TextApplyRequest<'_>,
) -> Result<TextPrepareOutcome> {
    if !request.authorized {
        return Ok(TextPrepareOutcome::PassThrough {
            reason: "evidence-gate-not-authorized",
        });
    }
    if request
        .original
        .contains("[ctx trimmed this output to save context.")
    {
        return Ok(TextPrepareOutcome::PassThrough {
            reason: "already-shortened",
        });
    }
    if request.original.is_empty() || request.replacement.is_empty() {
        return Ok(TextPrepareOutcome::PassThrough {
            reason: "empty-text-replacement",
        });
    }

    let rewind_id = text_rewind_id(
        request.surface,
        request.route_id,
        request.protocol_version,
        request.tool_name,
        request.original.as_bytes(),
    );
    let replacement = format!(
        "{}{}",
        request.replacement,
        crate::compress::trim_marker(&rewind_id)
    );
    let chars_in = request.original.chars().count();
    let chars_out = replacement.chars().count();
    let lines_keep = replacement.lines().count();
    if chars_out >= chars_in {
        return Ok(TextPrepareOutcome::PassThrough {
            reason: "rendered-result-has-no-savings",
        });
    }

    let prepared_at = chrono::Utc::now().to_rfc3339();
    let transaction = conn
        .transaction()
        .context("start text recovery transaction")?;
    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT original, COALESCE(trimmed, '') FROM rewind_store WHERE id=?1",
            [&rewind_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match existing {
        Some((original, trimmed)) if original == request.original && trimmed == replacement => {}
        Some(_) => anyhow::bail!("deterministic rewind id collision"),
        None => crate::db::insert_rewind_checked(
            &transaction,
            &rewind_id,
            &prepared_at,
            request.session_id,
            request.tool_name,
            request.command_or_path,
            request.original,
            &replacement,
        )
        .context("persist exact text original")?,
    }
    transaction
        .commit()
        .context("commit text recovery transaction")?;

    Ok(TextPrepareOutcome::Ready(Box::new(PreparedTextTrim {
        replacement,
        rewind_id,
        surface: request.surface.into(),
        route_id: request.route_id.into(),
        protocol_version: request.protocol_version.into(),
        tool_name: request.tool_name.into(),
        session_id: request.session_id.map(str::to_owned),
        command_or_path: request.command_or_path.into(),
        kind: request.kind.into(),
        strategy: request.strategy.into(),
        chars_in,
        chars_out,
        lines_total: request.original.lines().count(),
        lines_keep,
        prepared_at,
        transport_latency_ms: request.transport_latency_ms,
    })))
}

/// Mark a prepared text replacement only after its transport has proof of upstream acceptance.
pub fn mark_text_trim_accepted(prepared: &PreparedTextTrim) -> Result<bool> {
    let mut conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    mark_text_trim_accepted_in(&mut conn, prepared)
}

fn mark_text_trim_accepted_in(conn: &mut Connection, prepared: &PreparedTextTrim) -> Result<bool> {
    let transaction = conn
        .transaction()
        .context("start accepted text transaction")?;
    let already_accepted: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM compress_decisions WHERE applied=1 AND rewind_id=?1)",
        [&prepared.rewind_id],
        |row| row.get(0),
    )?;
    if already_accepted {
        transaction.commit()?;
        return Ok(false);
    }
    let features = json!({
        "adapter": "transport-atomic-v1",
        "route": prepared.route_id,
        "protocolVersion": prepared.protocol_version,
        "strategy": prepared.strategy,
        "acceptance": "upstream",
    });
    let features_json = serde_json::to_string(&features)?;
    let row = crate::db::CompressDecision {
        ts: &prepared.prepared_at,
        session_id: prepared.session_id.as_deref(),
        tool_name: &prepared.tool_name,
        server_prefix: Some(&prepared.route_id),
        kind: &prepared.kind,
        task_mode: "model-gateway",
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
    crate::db::mark_decision_emitted(
        &transaction,
        decision_id,
        &prepared.rewind_id,
        prepared.chars_out,
    )?;
    crate::db::insert_compress_event(
        &transaction,
        &prepared.prepared_at,
        prepared.session_id.as_deref(),
        &prepared.tool_name,
        &prepared.strategy,
        prepared.chars_in,
        prepared.chars_out,
        &prepared.command_or_path,
    )?;
    transaction
        .commit()
        .context("commit accepted text transaction")?;
    if let Some(latency_ms) = prepared.transport_latency_ms {
        crate::db::record_gateway_runtime_event_best_effort(
            &prepared.surface,
            &prepared.route_id,
            "applied",
            Some(latency_ms),
            None,
        );
    }
    Ok(true)
}

fn text_rewind_id(
    surface: &str,
    route_id: &str,
    protocol_version: &str,
    tool_name: &str,
    original: &[u8],
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ctx-transport-rewind-v1\0");
    for part in [surface, route_id, protocol_version, tool_name] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.update(original);
    format!("model-{:x}", hash.finalize())
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
    let outcome = match prepare_mcp_trim_in(&mut conn, original, request) {
        Ok(outcome) => outcome,
        Err(error) => McpPrepareOutcome::PassThrough {
            result: exact_original,
            reason: format!("apply-prepare-failed: {error}"),
        },
    };
    if let (Some(latency_ms), McpPrepareOutcome::PassThrough { reason, .. }) =
        (request.transport_latency_ms, &outcome)
    {
        crate::db::record_gateway_runtime_event_best_effort(
            request.surface,
            request.server_id,
            "pass_through",
            Some(latency_ms),
            Some(reason),
        );
    }
    outcome
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
        transport_latency_ms: request.transport_latency_ms,
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
    crate::db::mark_decision_emitted(
        &transaction,
        decision_id,
        &prepared.rewind_id,
        prepared.chars_out,
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
    if let Some(latency_ms) = prepared.transport_latency_ms {
        crate::db::record_gateway_runtime_event_best_effort(
            &prepared.surface,
            &prepared.server_id,
            "applied",
            Some(latency_ms),
            None,
        );
    }
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
            transport_latency_ms: None,
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
            transport_latency_ms: None,
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

    fn text_request<'a>(
        original: &'a str,
        replacement: &'a str,
        authorized: bool,
    ) -> TextApplyRequest<'a> {
        TextApplyRequest {
            surface: "codex",
            route_id: "codex-testing",
            protocol_version: "responses-v1",
            tool_name: "Shell",
            session_id: Some("session"),
            command_or_path: "cargo test",
            kind: "test",
            strategy: "test-v1",
            original,
            replacement,
            authorized,
            transport_latency_ms: None,
        }
    }

    #[test]
    fn text_prepare_is_durable_deterministic_and_acceptance_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let original = (0..100)
            .map(|index| format!("test {index} passed"))
            .collect::<Vec<_>>()
            .join("\n");
        let request = text_request(&original, "test 0 passed\ntest 99 passed", true);
        let TextPrepareOutcome::Ready(first) = prepare_text_trim_in(&mut conn, &request).unwrap()
        else {
            panic!("expected prepared text")
        };
        let TextPrepareOutcome::Ready(replayed) =
            prepare_text_trim_in(&mut conn, &request).unwrap()
        else {
            panic!("expected deterministic replay")
        };
        assert_eq!(replayed.rewind_id, first.rewind_id);
        assert_eq!(replayed.replacement, first.replacement);
        let stored = crate::db::get_rewind(&conn, &first.rewind_id).unwrap();
        assert_eq!(stored.original, original);
        assert_eq!(stored.trimmed, first.replacement);
        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM compress_decisions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(before, 0, "prepare/crash window cannot count as applied");

        mark_text_trim_accepted_in(&mut conn, &first).unwrap();
        mark_text_trim_accepted_in(&mut conn, &replayed).unwrap();
        let after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM compress_decisions WHERE applied=1 AND rewind_id=?1",
                [&first.rewind_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, 1, "acceptance retries cannot double count");
    }

    #[test]
    fn text_rejections_never_write_recovery_or_applied_receipts() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let large = "original line\n".repeat(100);
        let already =
            format!("{large}[ctx trimmed this output to save context. Full original id: prior]");
        let cases = [
            text_request(&large, "short", false),
            text_request("tiny", "not smaller", true),
            text_request(&already, "short", true),
        ];
        for request in cases {
            assert!(matches!(
                prepare_text_trim_in(&mut conn, &request).unwrap(),
                TextPrepareOutcome::PassThrough { .. }
            ));
        }
        let rewinds: i64 = conn
            .query_row("SELECT COUNT(*) FROM rewind_store", [], |row| row.get(0))
            .unwrap();
        let applied: i64 = conn
            .query_row("SELECT COUNT(*) FROM compress_decisions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((rewinds, applied), (0, 0));
    }
}
