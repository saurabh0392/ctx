use std::fs;
use std::path::{Path, PathBuf};

use ctx::agent::{AgentTransport, ClaudeCodeTransport};
use ctx::tool_result::{
    parse_mcp_result, render_mcp_result_or_original, CanonicalContentBlock, McpResultCoverage,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u32,
    fixtures: Vec<FixtureEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureEntry {
    id: String,
    path: String,
    source: String,
    source_url: Option<String>,
    platform: String,
    platform_version: String,
    os: String,
    observed_on: String,
    verification: String,
    result_selector: String,
    replacement_result: String,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoverageReport {
    schema_version: u32,
    fixture_count: usize,
    round_trip_identical: usize,
    content_blocks: usize,
    text_blocks: usize,
    image_blocks: usize,
    audio_blocks: usize,
    resource_link_blocks: usize,
    embedded_text_resource_blocks: usize,
    embedded_blob_resource_blocks: usize,
    unknown_blocks: usize,
    opaque_contract_fields: usize,
    structured_content_results: usize,
    metadata_results: usize,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tool_contracts")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read fixture")).expect("valid JSON")
}

fn manifest() -> FixtureManifest {
    serde_json::from_value(read_json(&fixture_dir().join("manifest.json")))
        .expect("valid fixture manifest")
}

fn selected_result(entry: &FixtureEntry) -> Value {
    let fixture = read_json(&fixture_dir().join(&entry.path));
    match entry.result_selector.as_str() {
        "root" => fixture,
        "tool_response" => fixture
            .get("tool_response")
            .cloned()
            .expect("fixture tool_response"),
        "tool_output_json" => serde_json::from_str(
            fixture
                .get("tool_output")
                .and_then(Value::as_str)
                .expect("fixture JSON-stringified tool_output"),
        )
        .expect("valid JSON-stringified tool output"),
        selector => panic!("unsupported fixture selector: {selector}"),
    }
}

#[test]
fn corpus_manifest_is_complete_and_every_result_round_trips() {
    let manifest = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.fixtures.len(), 4);

    let mut report = CoverageReport {
        schema_version: 1,
        fixture_count: manifest.fixtures.len(),
        ..Default::default()
    };

    for entry in &manifest.fixtures {
        assert!(!entry.id.is_empty());
        assert!(!entry.source.is_empty());
        assert!(!entry.platform.is_empty());
        assert!(!entry.platform_version.is_empty());
        assert!(!entry.os.is_empty());
        assert!(!entry.observed_on.is_empty());
        assert!(!entry.verification.is_empty());
        assert!(!entry.replacement_result.is_empty());
        if entry.platform == "MCP" {
            assert!(entry.source_url.as_deref().is_some_and(|url| {
                url.starts_with("https://modelcontextprotocol.io/specification/")
            }));
        }

        let raw = selected_result(entry);
        let parsed = parse_mcp_result(&raw)
            .unwrap_or_else(|error| panic!("{} failed to parse: {error}", entry.id));
        let rendered = parsed.render();
        assert_eq!(rendered, raw, "{} did not round-trip", entry.id);
        report.round_trip_identical += 1;
        add_coverage(&mut report, parsed.coverage());
    }

    let expected: CoverageReport =
        serde_json::from_value(read_json(&fixture_dir().join("coverage-report.json")))
            .expect("valid expected coverage report");
    assert_eq!(report, expected);
}

fn add_coverage(report: &mut CoverageReport, coverage: McpResultCoverage) {
    report.content_blocks += coverage.content_blocks;
    report.text_blocks += coverage.text_blocks;
    report.image_blocks += coverage.image_blocks;
    report.audio_blocks += coverage.audio_blocks;
    report.resource_link_blocks += coverage.resource_link_blocks;
    report.embedded_text_resource_blocks += coverage.embedded_text_resource_blocks;
    report.embedded_blob_resource_blocks += coverage.embedded_blob_resource_blocks;
    report.unknown_blocks += coverage.unknown_blocks;
    report.opaque_contract_fields += coverage.opaque_contract_fields;
    report.structured_content_results += usize::from(coverage.has_structured_content);
    report.metadata_results += usize::from(coverage.has_metadata);
}

#[test]
fn a_text_edit_preserves_every_non_text_block_and_top_level_field() {
    let raw = read_json(&fixture_dir().join("mcp-2025-11-25-mixed-result.json"));
    let mut parsed = parse_mcp_result(&raw).expect("parse mixed result");
    let CanonicalContentBlock::Text { text, .. } = &mut parsed.content[0] else {
        panic!("first fixture block must be text");
    };
    *text = "short typed candidate".into();

    let rendered = parsed.render();
    assert_eq!(
        rendered.pointer("/content/0/text"),
        Some(&json!("short typed candidate"))
    );
    assert_eq!(
        rendered.get("structuredContent"),
        raw.get("structuredContent")
    );
    assert_eq!(rendered.get("isError"), raw.get("isError"));
    assert_eq!(rendered.get("_meta"), raw.get("_meta"));
    assert_eq!(rendered.get("vendorTopLevel"), raw.get("vendorTopLevel"));
    assert_eq!(
        rendered
            .get("content")
            .and_then(Value::as_array)
            .map(|content| &content[1..]),
        raw.get("content")
            .and_then(Value::as_array)
            .map(|content| &content[1..])
    );
}

#[test]
fn typed_blocks_keep_only_extension_fields_in_their_preserved_maps() {
    let raw = read_json(&fixture_dir().join("mcp-2025-11-25-mixed-result.json"));
    let parsed = parse_mcp_result(&raw).expect("parse mixed result");

    for block in &parsed.content {
        let (preserved, typed_keys): (&Map<String, Value>, &[&str]) = match block {
            CanonicalContentBlock::Text { preserved, .. } => (preserved, &["type", "text"]),
            CanonicalContentBlock::Image { preserved, .. }
            | CanonicalContentBlock::Audio { preserved, .. } => {
                (preserved, &["type", "data", "mimeType"])
            }
            CanonicalContentBlock::ResourceLink { preserved, .. } => {
                (preserved, &["type", "name", "uri"])
            }
            CanonicalContentBlock::EmbeddedTextResource { preserved, .. } => {
                assert_embedded_payload_is_not_duplicated(preserved, "text");
                continue;
            }
            CanonicalContentBlock::EmbeddedBlobResource { preserved, .. } => {
                assert_embedded_payload_is_not_duplicated(preserved, "blob");
                continue;
            }
            CanonicalContentBlock::Unknown { .. } => continue,
        };
        for key in typed_keys {
            assert!(
                !preserved.contains_key(*key),
                "typed field {key} must not be duplicated in preserved extensions"
            );
        }
    }
}

#[test]
fn embedded_resource_renderer_overwrites_an_invalid_preserved_resource_field() {
    let raw = read_json(&fixture_dir().join("mcp-2025-11-25-mixed-result.json"));
    let mut parsed = parse_mcp_result(&raw).expect("parse mixed result");
    let block = parsed
        .content
        .iter_mut()
        .find(|block| matches!(block, CanonicalContentBlock::EmbeddedTextResource { .. }))
        .expect("embedded text resource fixture");
    let CanonicalContentBlock::EmbeddedTextResource {
        uri,
        text,
        preserved,
    } = block
    else {
        unreachable!("matching block selected")
    };
    let expected_uri = uri.clone();
    let expected_text = text.clone();
    preserved.insert("resource".into(), json!("invalid extension value"));

    let rendered = block.render();
    assert_eq!(
        rendered.pointer("/resource/uri"),
        Some(&Value::String(expected_uri))
    );
    assert_eq!(
        rendered.pointer("/resource/text"),
        Some(&Value::String(expected_text))
    );
}

fn assert_embedded_payload_is_not_duplicated(preserved: &Map<String, Value>, payload_key: &str) {
    assert!(!preserved.contains_key("type"));
    let resource = preserved
        .get("resource")
        .and_then(Value::as_object)
        .expect("preserved embedded-resource extensions");
    assert!(!resource.contains_key("uri"));
    assert!(!resource.contains_key(payload_key));
}

#[test]
fn native_platform_adapters_capture_the_same_lossless_result_in_shadow() {
    let dir = fixture_dir();

    let claude_payload = read_json(&dir.join("claude-code-2.1.153-post-tool-use-mcp.json"));
    let claude = ClaudeCodeTransport
        .extract(&claude_payload, true)
        .expect("Claude MCP result");
    assert_eq!(
        claude.canonical_mcp.expect("Claude canonical MCP").render(),
        claude_payload["tool_response"]
    );

    let cursor_payload = read_json(&dir.join("cursor-3.7.19-post-tool-use-mcp.json"));
    let cursor = ctx::cursor_hook::extract_cursor_tool_result(&cursor_payload, true)
        .expect("Cursor MCP result");
    let cursor_wire: Value = serde_json::from_str(cursor_payload["tool_output"].as_str().unwrap())
        .expect("Cursor wire JSON");
    assert_eq!(
        cursor.canonical_mcp.expect("Cursor canonical MCP").render(),
        cursor_wire
    );

    let codex_payload = read_json(&dir.join("codex-0.144.5-post-tool-use-mcp.json"));
    let codex =
        ctx::codex_hook::extract_tool_result(&codex_payload, true).expect("Codex MCP result");
    assert_eq!(
        codex.canonical_mcp.expect("Codex canonical MCP").render(),
        codex_payload["tool_response"]
    );
}

#[test]
fn native_platform_adapters_skip_canonical_capture_outside_shadow() {
    let dir = fixture_dir();

    let claude_payload = read_json(&dir.join("claude-code-2.1.153-post-tool-use-mcp.json"));
    let claude = ClaudeCodeTransport
        .extract(&claude_payload, false)
        .expect("Claude MCP result");
    assert!(claude.canonical_mcp.is_none());

    let cursor_payload = read_json(&dir.join("cursor-3.7.19-post-tool-use-mcp.json"));
    let cursor = ctx::cursor_hook::extract_cursor_tool_result(&cursor_payload, false)
        .expect("Cursor MCP result");
    assert!(cursor.canonical_mcp.is_none());

    let codex_payload = read_json(&dir.join("codex-0.144.5-post-tool-use-mcp.json"));
    let codex =
        ctx::codex_hook::extract_tool_result(&codex_payload, false).expect("Codex MCP result");
    assert!(codex.canonical_mcp.is_none());
}

#[test]
fn typed_mcp_compressor_is_observation_only() {
    let payload = read_json(&fixture_dir().join("claude-code-2.1.153-post-tool-use-mcp.json"));
    let tr = ClaudeCodeTransport
        .extract(&payload, true)
        .expect("Claude MCP result");
    let raw_output = tr.raw_output.clone();
    let cfg = ctx::config::Config {
        compress_enabled: true,
        compress_shadow_enabled: true,
        compress_preset: ctx::config::CompressPreset::Off,
        ..Default::default()
    };

    let decision = ctx::agent::decide(&cfg, &tr);
    assert!(!decision.apply, "typed T1 path must remain shadow-only");
    assert_eq!(tr.raw_output, raw_output, "shadow parse changed live text");
    let contract = decision
        .shadow
        .expect("shadow decision")
        .features
        .mcp_contract
        .expect("typed MCP evidence");
    assert!(contract.round_trip_identical);
    assert_eq!(contract.text_blocks, 1);
    assert_eq!(
        contract.eligible_strategy.as_deref(),
        Some("mcp-text-blocks")
    );
    assert_eq!(contract.eligible_strategy_version.as_deref(), Some("1"));
    assert_eq!(contract.proposal_validated, Some(true));
    assert_eq!(contract.proposal_replacements, Some(1));
    assert!(contract.proposal_rejection.is_none());
    assert!(contract.candidate_strategy.is_some());
}

#[test]
fn typed_mcp_evidence_reports_a_broken_round_trip() {
    let payload = read_json(&fixture_dir().join("claude-code-2.1.153-post-tool-use-mcp.json"));
    let mut tr = ClaudeCodeTransport
        .extract(&payload, true)
        .expect("Claude MCP result");
    let canonical = tr.canonical_mcp.as_mut().expect("canonical MCP");
    let text = canonical
        .content
        .iter_mut()
        .find_map(|block| match block {
            CanonicalContentBlock::Text { text, .. } => Some(text),
            _ => None,
        })
        .expect("fixture text block");
    text.push_str(" mutated");

    let cfg = ctx::config::Config {
        compress_enabled: true,
        compress_shadow_enabled: true,
        compress_preset: ctx::config::CompressPreset::Off,
        ..Default::default()
    };
    let contract = ctx::agent::decide(&cfg, &tr)
        .shadow
        .expect("shadow decision")
        .features
        .mcp_contract
        .expect("typed MCP evidence");
    assert!(!contract.round_trip_identical);
}

#[test]
fn typed_mcp_evidence_does_no_work_when_shadow_collection_is_disabled() {
    let payload = read_json(&fixture_dir().join("claude-code-2.1.153-post-tool-use-mcp.json"));
    let tr = ClaudeCodeTransport
        .extract(&payload, false)
        .expect("Claude MCP result");
    assert!(
        tr.canonical_mcp.is_none(),
        "the adapter must not parse or retain canonical MCP when shadow is disabled"
    );
    let cfg = ctx::config::Config {
        compress_enabled: true,
        compress_shadow_enabled: false,
        compress_preset: ctx::config::CompressPreset::Off,
        ..Default::default()
    };

    let decision = ctx::agent::decide(&cfg, &tr);
    assert!(!decision.apply);
    assert!(
        decision
            .shadow
            .expect("controller still computes its existing decision")
            .features
            .mcp_contract
            .is_none(),
        "typed MCP evidence must be skipped when shadow collection is disabled"
    );
}

#[test]
fn generated_inputs_never_panic_or_partially_rebuild() {
    for seed in 0..512_u64 {
        let value = generated_value(seed, 0);
        assert_eq!(
            render_mcp_result_or_original(&value),
            value,
            "generated seed {seed} changed during a no-transform render"
        );
    }
}

fn generated_value(mut seed: u64, depth: usize) -> Value {
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    if depth >= 4 {
        return json!({"leaf": seed, "type": if seed.is_multiple_of(2) { "text" } else { "future" }});
    }
    match seed % 7 {
        0 => Value::Null,
        1 => Value::Bool(seed.is_multiple_of(2)),
        2 => Value::String(format!("generated-{seed}")),
        3 => Value::Array(vec![
            generated_value(seed.wrapping_add(1), depth + 1),
            generated_value(seed.wrapping_add(2), depth + 1),
        ]),
        4 => {
            let mut object = Map::new();
            object.insert("content".into(), Value::String("wrong-shape".into()));
            object.insert("unknown".into(), generated_value(seed + 1, depth + 1));
            Value::Object(object)
        }
        5 => json!({
            "content": [
                {"type": "text", "text": format!("seed-{seed}"), "future": seed},
                {"type": "future", "nested": generated_value(seed + 1, depth + 1)},
                seed
            ],
            "structuredContent": {"seed": seed},
            "isError": seed.is_multiple_of(2),
            "vendor": generated_value(seed + 2, depth + 1)
        }),
        _ => json!({
            "content": [
                {"type": "resource", "resource": {"uri": format!("test://{seed}"), "blob": "AA=="}},
                {"type": "image", "data": "AA==", "mimeType": "image/png"}
            ],
            "isError": {"future": true}
        }),
    }
}
