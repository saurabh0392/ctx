//! Provider-neutral model-path gateway contracts and M0 compatibility tooling.
//!
//! M0 is deliberately non-mutating: it can inspect documented configuration boundaries and
//! sanitize an offline capture, but it cannot listen, route, forward, or rewrite model traffic.

pub mod capture;
pub mod probe;
pub mod registry;
mod relay;
pub mod route;
mod service;

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

use crate::surface::SurfaceId;

const MAX_CAPTURE_BYTES: u64 = 8 * 1024 * 1024;

pub fn print_probe(surface: &str, run_client_version: bool, json: bool) -> Result<()> {
    let receipts = if surface == "all" {
        [SurfaceId::ClaudeCode, SurfaceId::Cursor, SurfaceId::Codex]
            .into_iter()
            .map(|surface| probe::probe(surface, run_client_version))
            .collect()
    } else {
        let parsed = SurfaceId::parse(surface).ok_or_else(|| {
            anyhow::anyhow!("unknown surface '{surface}' (use claude-code, cursor, codex, or all)")
        })?;
        vec![probe::probe(parsed, run_client_version)]
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&receipts)?);
        return Ok(());
    }

    for receipt in receipts {
        println!(
            "{}: {} ({})",
            receipt.surface.as_str(),
            receipt.status.as_str(),
            receipt.decision.as_str()
        );
        println!(
            "  client: {}",
            receipt
                .client_version
                .as_deref()
                .unwrap_or(if receipt.installed {
                    "detected (version not queried)"
                } else {
                    "not detected"
                })
        );
        println!("  boundary: {}", receipt.configuration_boundary.as_str());
        println!("  protocol: {}", receipt.protocol.as_str());
        println!("  auth: {}", receipt.authentication.as_str());
        if receipt.client_process_executed {
            println!("  version probe: client executed; startup maintenance was possible");
        }
        for reason in receipt.reasons {
            println!("  - {reason}");
        }
    }
    Ok(())
}

pub fn sanitize_capture_file(input: Option<&Path>) -> Result<()> {
    let raw = match input {
        Some(path) => {
            let file = std::fs::File::open(path)
                .with_context(|| format!("open capture input {}", path.display()))?;
            read_bounded(file).with_context(|| format!("read capture input {}", path.display()))?
        }
        None => read_bounded(std::io::stdin()).context("read capture input from stdin")?,
    };
    let capture: capture::RawCapture =
        serde_json::from_str(&raw).context("parse capture input JSON")?;
    let sanitized = capture::sanitize(&capture);
    println!("{}", serde_json::to_string_pretty(&sanitized)?);
    Ok(())
}

pub async fn serve(route_id: &str) -> Result<()> {
    service::serve(route_id).await
}

fn read_bounded(reader: impl Read) -> Result<String> {
    read_bounded_with_limit(reader, MAX_CAPTURE_BYTES)
}

fn read_bounded_with_limit(reader: impl Read, limit: u64) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        anyhow::bail!("capture input exceeds the 8 MiB M0 safety limit");
    }
    String::from_utf8(bytes).context("capture input must be UTF-8 JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_rejects_input_before_unbounded_allocation() {
        let error = read_bounded_with_limit(std::io::Cursor::new(b"1234"), 3).unwrap_err();
        assert!(error.to_string().contains("safety limit"));
    }
}
