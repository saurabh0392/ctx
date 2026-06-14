//! Historical MCP tool usage analysis for vector-powered tool filtering experiments.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ToolUsageRow {
    pub tool_name: String,
    pub display: String,
    pub invocations: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolUsageAnalysis {
    pub sessions: u64,
    pub turns: u64,
    pub spend_usd: f64,
    pub tool_invocations: u64,
    pub distinct_tools: u64,
    pub embedded_sessions: u64,
    pub avg_tools_per_session: f64,
    pub tools_for_80pct_invocations: u64,
    pub keep_tools_candidate_count: u64,
    pub top_tools: Vec<ToolUsageRow>,
    pub recommendation: String,
}

pub fn run(conn: &Connection) -> Result<ToolUsageAnalysis> {
    let (sessions, turns, spend): (u64, u64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(turn_count), 0), COALESCE(SUM(total_usd), 0.0) FROM sessions",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    let tool_invocations: u64 =
        conn.query_row("SELECT COUNT(*) FROM tool_invocations", [], |r| r.get(0))?;

    let distinct_tools: u64 = conn.query_row(
        "SELECT COUNT(DISTINCT tool_name) FROM tool_invocations",
        [],
        |r| r.get(0),
    )?;

    let embedded_sessions: u64 = conn.query_row(
        "SELECT COUNT(DISTINCT se.session_id) FROM session_embeddings se
         JOIN tool_invocations ti ON ti.session_id = se.session_id",
        [],
        |r| r.get(0),
    )?;

    let avg_tools_per_session: f64 = conn.query_row(
        "SELECT AVG(tc) FROM (
            SELECT COUNT(DISTINCT tool_name) AS tc FROM tool_invocations GROUP BY session_id
         )",
        [],
        |r| r.get(0),
    )?;

    let tools_for_80pct: u64 = conn.query_row(
        "SELECT COUNT(*) FROM (
            WITH ranked AS (
              SELECT tool_name, COUNT(*) AS n,
                SUM(COUNT(*)) OVER (ORDER BY COUNT(*) DESC) AS cum,
                SUM(COUNT(*)) OVER () AS total
              FROM tool_invocations GROUP BY tool_name
            )
            SELECT * FROM ranked WHERE cum <= total * 0.8
         )",
        [],
        |r| r.get(0),
    )?;

    let keep_tools_candidate_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT tool_name FROM tool_invocations
            WHERE ts >= datetime('now', '-30 days')
            GROUP BY tool_name HAVING COUNT(*) >= 3
         )",
        [],
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT tool_name, COUNT(*) AS n, COUNT(DISTINCT session_id) AS s
         FROM tool_invocations GROUP BY tool_name ORDER BY n DESC LIMIT 15",
    )?;
    let top_tools: Vec<ToolUsageRow> = stmt
        .query_map([], |r| {
            let tool_name: String = r.get(0)?;
            Ok(ToolUsageRow {
                display: crate::semantic_tools::display_name_for_target(&tool_name),
                invocations: r.get(1)?,
                sessions: r.get(2)?,
                tool_name,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let recommendation = build_recommendation(
        embedded_sessions,
        distinct_tools,
        tools_for_80pct,
        keep_tools_candidate_count,
        avg_tools_per_session,
    );

    Ok(ToolUsageAnalysis {
        sessions,
        turns,
        spend_usd: spend,
        tool_invocations,
        distinct_tools,
        embedded_sessions,
        avg_tools_per_session,
        tools_for_80pct_invocations: tools_for_80pct,
        keep_tools_candidate_count,
        top_tools,
        recommendation,
    })
}

fn build_recommendation(
    embedded_sessions: u64,
    distinct_tools: u64,
    tools_for_80pct: u64,
    keep_candidates: u64,
    avg_tools_per_session: f64,
) -> String {
    if embedded_sessions < 2 {
        return "Need at least 2 embedded sessions before semantic tool mix can vote. Run ctx ingest on your corpus.".into();
    }
    let strip_headroom = distinct_tools.saturating_sub(tools_for_80pct);
    if strip_headroom < 3 {
        return format!(
            "Only {distinct_tools} distinct MCP tools indexed. Static personal keep_tools already covers most usage. Semantic mix will help on cold prompts that need rare tools, not on bulk savings."
        );
    }
    format!(
        "Vector tool mix is viable: {embedded_sessions} embedded sessions, {keep_candidates} tools qualify for keep_tools (≥3 uses/30d), {tools_for_80pct} tools cover 80% of invocations (avg {avg_tools_per_session:.1} tools/session). Run tool_mix_ab: static tool-level deny as baseline, semantic overlay on treatment arm."
    )
}

pub fn print_human(a: &ToolUsageAnalysis) {
    println!("MCP tool usage analysis");
    println!("────────────────────────────────────────");
    println!(
        "  {} sessions · {} turns · ${:.2} total spend",
        a.sessions, a.turns, a.spend_usd
    );
    println!(
        "  {} tool invocations · {} distinct tools · {} embedded sessions",
        a.tool_invocations, a.distinct_tools, a.embedded_sessions
    );
    println!(
        "  {:.1} tools/session avg · {} tools cover 80% of invocations",
        a.avg_tools_per_session, a.tools_for_80pct_invocations
    );
    println!(
        "  {} tools qualify for keep_tools (≥3 invocations in last 30 days)",
        a.keep_tools_candidate_count
    );
    println!();
    println!("Top tools:");
    for t in &a.top_tools {
        println!(
            "  {:>4} calls · {:>2} sessions · {}",
            t.invocations, t.sessions, t.display
        );
    }
    println!();
    println!("Recommendation");
    println!("  {}", a.recommendation);
}
