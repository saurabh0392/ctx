//! Shareable per-repo Context Report (CTX-56, product exit). `ctx report --repo <name>` writes a
//! single self-contained HTML file: no ctx install, no server, no external assets, so it opens in
//! any browser on any machine. It is a static snapshot of where a repo's agent context went, the
//! Context Bill someone can send a teammate. Report generation is local; sharing the exported file
//! is always an explicit user action.
//! the user chooses to share.

use anyhow::{bail, Result};

use crate::cli::{ReportFormat, ReportPrivacy};
use crate::db::{ContextBill, ContextBillTool};

#[derive(serde::Serialize)]
struct ContextReportV1 {
    schema: &'static str,
    generated_at: String,
    privacy: &'static str,
    repo: String,
    decisions: i64,
    tool_count: usize,
    sink_tokens: i64,
    eligible_tokens: i64,
    reclaimed_tokens: i64,
    eligibility_note: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ReportTool>>,
}

#[derive(serde::Serialize)]
struct ReportTool {
    name: String,
    decisions: i64,
    sink_tokens: i64,
    eligible_tokens: i64,
    reclaimed_tokens: i64,
    sources: Vec<ReportSource>,
}

#[derive(serde::Serialize)]
struct ReportSource {
    label: String,
    calls: i64,
    sink_tokens: i64,
}

/// Entry point for `ctx report`. With `list` or no `repo`, prints the repos ctx has data for. With a
/// `repo` (substring match), writes the report to `out` (default `ctx-report-<repo>.html` in the
/// current directory) and prints the path.
pub fn run(
    repo: Option<&str>,
    out: Option<&str>,
    list: bool,
    privacy: ReportPrivacy,
    format: ReportFormat,
) -> Result<()> {
    let conn = crate::db::open_db()?;
    let repos = crate::db::list_repos(&conn);

    if list || repo.is_none() {
        if repos.is_empty() {
            println!("No repos recorded yet. Run some agent sessions with ctx installed, then ctx ingest.");
            return Ok(());
        }
        println!("Repos ctx has context data for:\n");
        for r in &repos {
            println!(
                "  {:>7}  {:>6} decisions   {}",
                human_chars(r.sink_chars),
                r.decisions,
                r.repo_key
            );
        }
        println!("\nExport one:  ctx report --repo <name>   (name matches part of the path)");
        return Ok(());
    }

    let needle = repo.unwrap();
    let matches: Vec<&crate::db::RepoSummary> = repos
        .iter()
        .filter(|r| r.repo_key.to_lowercase().contains(&needle.to_lowercase()))
        .collect();
    let chosen = match matches.as_slice() {
        [] => bail!("No repo matches \"{needle}\". Run `ctx report --list` to see what ctx has."),
        [one] => *one,
        many => {
            let names: Vec<&str> = many.iter().map(|r| r.repo_key.as_str()).collect();
            bail!(
                "\"{needle}\" matches {} repos, narrow it:\n  {}",
                many.len(),
                names.join("\n  ")
            );
        }
    };

    let bill = crate::db::repo_bill(&conn, &chosen.repo_key);
    let payload = match format {
        ReportFormat::Html => render_html(&chosen.repo_key, &bill, privacy),
        ReportFormat::Json => {
            serde_json::to_string_pretty(&render_json(&chosen.repo_key, &bill, privacy))?
        }
    };

    let path = match out {
        Some(p) => p.to_string(),
        None => format!(
            "ctx-report-{}.{}",
            slug(&repo_display(&chosen.repo_key)),
            match format {
                ReportFormat::Html => "html",
                ReportFormat::Json => "json",
            }
        ),
    };
    std::fs::write(&path, payload)?;
    println!(
        "Wrote {} ({}, {} decisions) to {path}",
        repo_display(&chosen.repo_key),
        human_chars(bill.total_sink_chars),
        bill.decisions
    );
    if matches!(privacy, ReportPrivacy::Detailed) {
        println!(
            "Detailed privacy mode includes the tool, command, and path labels shown in the file. Review before sharing."
        );
    } else {
        println!(
            "Aggregate mode omits commands, paths, absolute repo paths, and tool/server names."
        );
    }
    crate::beta::record_event(
        "context_report_exported",
        "cli",
        Some(match privacy {
            ReportPrivacy::Aggregate => "aggregate",
            ReportPrivacy::Detailed => "detailed",
        }),
    );
    Ok(())
}

fn render_json(repo_key: &str, bill: &ContextBill, privacy: ReportPrivacy) -> ContextReportV1 {
    let detailed = matches!(privacy, ReportPrivacy::Detailed);
    ContextReportV1 {
        schema: "ctx.context-report.v1",
        generated_at: chrono::Utc::now().to_rfc3339(),
        privacy: if detailed { "detailed" } else { "aggregate" },
        repo: repo_display(repo_key),
        decisions: bill.decisions,
        tool_count: bill.tools.len(),
        sink_tokens: approx_tokens(bill.total_sink_chars),
        eligible_tokens: approx_tokens(bill.total_reclaimable_chars),
        reclaimed_tokens: approx_tokens(bill.total_reclaimed_chars),
        eligibility_note: "Eligible under CTX's current transform; earned activation is evaluated separately from eligibility.",
        tools: detailed.then(|| {
            bill.tools
                .iter()
                .map(|t| ReportTool {
                    name: t.tool.clone(),
                    decisions: t.decisions,
                    sink_tokens: approx_tokens(t.sink_chars),
                    eligible_tokens: approx_tokens(t.reclaimable_chars),
                    reclaimed_tokens: approx_tokens(t.reclaimed_chars),
                    sources: t
                        .sources
                        .iter()
                        .map(|s| ReportSource {
                            label: s.label.clone(),
                            calls: s.calls,
                            sink_tokens: approx_tokens(s.sink_chars),
                        })
                        .collect(),
                })
                .collect()
        }),
    }
}

/// Approximate tokens from characters (the ~4-chars-per-token rule the dashboard uses).
fn approx_tokens(chars: i64) -> i64 {
    chars / 4
}

/// Compact char count: 1.2M, 340K, 900.
fn human_chars(chars: i64) -> String {
    let c = chars.max(0) as f64;
    if c >= 1_000_000.0 {
        format!("{:.1}M", c / 1_000_000.0)
    } else if c >= 1_000.0 {
        format!("{:.0}K", c / 1_000.0)
    } else {
        format!("{}", chars.max(0))
    }
}

fn human_tokens(chars: i64) -> String {
    human_chars(approx_tokens(chars))
}

/// The trailing path segment of a repo key, e.g. "ctx" from "/Users/me/Projects/ctx".
fn repo_display(repo_key: &str) -> String {
    repo_key
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(repo_key)
        .to_string()
}

/// A filesystem-safe slug for the default output filename.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Minimal HTML escaping for values that land in text nodes (tool names, command/path labels).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_html(repo_key: &str, bill: &ContextBill, privacy: ReportPrivacy) -> String {
    let name = esc(&repo_display(repo_key));
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let sink_tok = human_tokens(bill.total_sink_chars);
    let reclaimable_tok = human_tokens(bill.total_reclaimable_chars);
    let reclaimed_tok = human_tokens(bill.total_reclaimed_chars);
    let reclaimable_pct = if bill.total_sink_chars > 0 {
        (bill.total_reclaimable_chars as f64 / bill.total_sink_chars as f64 * 100.0).round() as i64
    } else {
        0
    };

    let max_sink = bill
        .tools
        .iter()
        .map(|t| t.sink_chars)
        .max()
        .unwrap_or(1)
        .max(1);
    let detailed = matches!(privacy, ReportPrivacy::Detailed);
    let rows: String = if detailed {
        bill.tools.iter().map(|t| tool_row(t, max_sink)).collect()
    } else {
        String::new()
    };

    let body = if bill.tools.is_empty() {
        "<div class=\"empty\">ctx has not recorded any tool output for this repo yet.</div>"
            .to_string()
    } else if !detailed {
        format!(
            "<div class=\"empty\">{} tool categories measured. Aggregate privacy mode intentionally omits tool names, commands, paths, and source labels.</div>",
            bill.tools.len()
        )
    } else {
        format!("<div class=\"tools\">{rows}</div>")
    };
    let privacy_label = if detailed { "Detailed" } else { "Aggregate" };
    let privacy_warning = if detailed {
        "Detailed mode: this file contains the tool, command, and path labels visible below. Review it before sharing."
    } else {
        "Aggregate mode: command text, paths, absolute repo paths, and tool/server names are omitted."
    };

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Context Report: {name}</title>
<style>
  :root {{ --bg:#0a0e17; --surface:#121826; --surface2:#0f1420; --border:#1e2636; --t1:#e6ebf5; --t2:#aab3c5; --t3:#7a8499; --t4:#556072; --primary:#31c48d; --primary-lt:#5fd6a6; }}
  * {{ box-sizing: border-box; }}
  body {{ margin:0; background:var(--bg); color:var(--t1); font:15px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif; }}
  .wrap {{ max-width: 820px; margin: 0 auto; padding: 48px 24px 80px; }}
  .eyebrow {{ font-size:12px; font-weight:700; letter-spacing:.08em; text-transform:uppercase; color:var(--t4); }}
  h1 {{ font-size:32px; font-weight:800; letter-spacing:-0.5px; margin:6px 0 4px; }}
  .sub {{ color:var(--t3); font-size:14px; }}
  .hero {{ background:linear-gradient(155deg,#0f2920 0%,var(--surface) 72%); border:1px solid #1e3b30; border-radius:16px; padding:26px 28px; margin:26px 0; }}
  .hero-big {{ font-size:40px; font-weight:800; letter-spacing:-1px; line-height:1.05; }}
  .hero-cap {{ color:var(--t2); font-size:14px; margin-top:8px; max-width:560px; }}
  .stats {{ display:grid; grid-template-columns:repeat(3,1fr); gap:12px; margin:20px 0 8px; }}
  .stat {{ background:var(--surface); border:1px solid var(--border); border-radius:12px; padding:16px 18px; }}
  .stat-k {{ font-size:11px; color:var(--t4); text-transform:uppercase; letter-spacing:.05em; }}
  .stat-v {{ font-size:24px; font-weight:800; margin-top:4px; }}
  .stat-s {{ font-size:12px; color:var(--t3); margin-top:2px; }}
  .sec-h {{ font-size:12px; font-weight:700; letter-spacing:.08em; text-transform:uppercase; color:var(--t4); margin:34px 0 6px; }}
  .sec-note {{ color:var(--t3); font-size:13px; margin:0 0 16px; }}
  .tools {{ display:flex; flex-direction:column; gap:10px; }}
  .tool {{ background:var(--surface); border:1px solid var(--border); border-radius:12px; padding:15px 17px; }}
  .tool-head {{ display:flex; align-items:baseline; justify-content:space-between; gap:12px; }}
  .tool-name {{ font-weight:700; font-size:15px; }}
  .tool-tok {{ font-variant-numeric:tabular-nums; color:var(--t2); font-size:13px; white-space:nowrap; }}
  .bar {{ height:6px; border-radius:999px; background:var(--surface2); margin:10px 0 8px; overflow:hidden; }}
  .bar-fill {{ height:100%; background:linear-gradient(90deg,var(--primary),var(--primary-lt)); }}
  .tool-meta {{ font-size:12px; color:var(--t3); }}
  .sources {{ margin-top:10px; border-top:1px solid var(--border); padding-top:10px; display:flex; flex-direction:column; gap:5px; }}
  .src {{ display:flex; justify-content:space-between; gap:12px; font-size:12px; color:var(--t3); }}
  .src code {{ font-family:"SF Mono",ui-monospace,monospace; color:var(--t2); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .src-n {{ white-space:nowrap; font-variant-numeric:tabular-nums; }}
  .empty {{ color:var(--t3); border:1px dashed var(--border); border-radius:12px; padding:20px; }}
  .foot {{ margin-top:44px; padding-top:20px; border-top:1px solid var(--border); color:var(--t4); font-size:12px; line-height:1.7; }}
  @media (max-width:640px) {{ .stats {{ grid-template-columns:1fr; }} }}
</style></head>
<body><div class="wrap">
  <div class="eyebrow">Context report</div>
  <h1>{name}</h1>
  <div class="sub">Where this repo's coding-agent context went. Generated locally by ctx on {date}. Single machine, early data. {privacy_label} privacy.</div>

  <div class="hero">
    <div class="hero-big">{sink_tok} tokens</div>
    <div class="hero-cap">of tool output entered the agent's context window on this repo. About {reclaimable_pct}% of it ({reclaimable_tok} tokens) is eligible under CTX's current transform. Eligibility is not an earned-safety verdict; activation is evaluated separately from observed re-read and re-edit outcomes.</div>
  </div>

  <div class="stats">
    <div class="stat"><div class="stat-k">Eligible</div><div class="stat-v">{reclaimable_tok}</div><div class="stat-s">tokens under the current transform</div></div>
    <div class="stat"><div class="stat-k">Reclaimed</div><div class="stat-v">{reclaimed_tok}</div><div class="stat-s">tokens ctx already removed</div></div>
    <div class="stat"><div class="stat-k">Decisions</div><div class="stat-v">{decisions}</div><div class="stat-s">tool results watched</div></div>
  </div>

  <div class="sec-h">Where it went, by tool</div>
  <p class="sec-note">{privacy_warning}</p>
  {body}

  <div class="foot">
    Generated by ctx, a local context observability and control tool. Every number here comes from one machine; exporting this file made no network request. Token counts are approximate (about four characters per token). {privacy_warning}
  </div>
</div></body></html>
"#,
        decisions = bill.decisions,
    )
}

fn tool_row(t: &ContextBillTool, max_sink: i64) -> String {
    let pct = ((t.sink_chars as f64 / max_sink as f64) * 100.0).clamp(2.0, 100.0);
    let reclaim = if t.sink_chars > 0 {
        (t.reclaimable_chars as f64 / t.sink_chars as f64 * 100.0).round() as i64
    } else {
        0
    };
    let sources: String = t
        .sources
        .iter()
        .take(5)
        .map(|s| {
            format!(
                "<div class=\"src\"><code>{}</code><span class=\"src-n\">{} tok, {}&times;</span></div>",
                esc(&truncate(&s.label, 90)),
                human_tokens(s.sink_chars),
                s.calls
            )
        })
        .collect();
    let sources_block = if sources.is_empty() {
        String::new()
    } else {
        format!("<div class=\"sources\">{sources}</div>")
    };
    format!(
        r#"<div class="tool">
  <div class="tool-head"><span class="tool-name">{name}</span><span class="tool-tok">{tok} tokens</span></div>
  <div class="bar"><div class="bar-fill" style="width:{pct:.0}%"></div></div>
  <div class="tool-meta">{calls} calls, {reclaim}% trimmable</div>
  {sources_block}
</div>"#,
        name = esc(&pretty_tool(&t.tool)),
        tok = human_tokens(t.sink_chars),
        calls = t.decisions,
    )
}

/// Human-friendly tool label: strip the `mcp__server__tool` wrapper to `server tool`.
fn pretty_tool(tool: &str) -> String {
    if let Some(rest) = tool.strip_prefix("mcp__") {
        return rest.replacen("__", " ", 1).replace('_', " ");
    }
    tool.to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ContextBillSource, ContextBillTool};

    fn bill() -> ContextBill {
        ContextBill {
            tools: vec![ContextBillTool {
                tool: "mcp__linear__list_projects".into(),
                decisions: 4,
                sink_chars: 112_732,
                reclaimable_chars: 110_000,
                reclaimed_chars: 0,
                sources: vec![ContextBillSource {
                    label: "list_projects".into(),
                    calls: 4,
                    sink_chars: 112_732,
                }],
                rewinds: Vec::new(),
                trims: Vec::new(),
            }],
            total_sink_chars: 112_732,
            total_reclaimable_chars: 110_000,
            total_reclaimed_chars: 0,
            decisions: 4,
            since: None,
            trend: Vec::new(),
        }
    }

    #[test]
    fn report_is_self_contained_and_has_the_numbers() {
        let html = render_html("/Users/me/Projects/ctx", &bill(), ReportPrivacy::Detailed);
        assert!(html.contains("<!doctype html>"));
        // No external assets: no http(s) links, no src= fetches.
        assert!(!html.contains("http://") && !html.contains("https://"));
        assert!(!html.contains("<script"));
        assert!(html.contains("Context Report: ctx"));
        // MCP tool label is humanized, not the raw wire name.
        assert!(html.contains("linear list projects"));
        assert!(!html.contains("mcp__linear__"));
        // The headline sink number renders in tokens (~28K).
        assert!(html.contains("28K tokens"));
    }

    #[test]
    fn empty_bill_states_it_plainly() {
        let empty = ContextBill::default();
        let html = render_html("/x/y/thing", &empty, ReportPrivacy::Aggregate);
        assert!(html.contains("has not recorded any tool output"));
    }

    #[test]
    fn aggregate_report_omits_sensitive_labels() {
        let html = render_html(
            "/Users/me/Projects/secret-repo",
            &bill(),
            ReportPrivacy::Aggregate,
        );
        assert!(!html.contains("mcp__linear__"));
        assert!(!html.contains("linear list projects"));
        assert!(!html.contains("list_projects"));
        assert!(!html.contains("/Users/me/Projects"));
        let json = serde_json::to_value(render_json(
            "/Users/me/Projects/secret-repo",
            &bill(),
            ReportPrivacy::Aggregate,
        ))
        .unwrap();
        assert!(json.get("tools").is_none());
        assert_eq!(json["schema"], "ctx.context-report.v1");
    }

    #[test]
    fn repo_display_and_slug() {
        assert_eq!(repo_display("/Users/me/Projects/ctx"), "ctx");
        assert_eq!(repo_display("/Users/me/Projects/ctx/"), "ctx");
        assert_eq!(slug("the-gaffer"), "the-gaffer");
        assert_eq!(slug("a/b c"), "a-b-c");
    }
}
