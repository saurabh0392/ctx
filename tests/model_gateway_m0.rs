use std::fs;
use std::path::{Path, PathBuf};

use ctx::model_gateway::capture::{sanitize, RawCapture};
use ctx::model_gateway::probe;
use ctx::model_gateway::route::{ProbeStatus, RouteDecision, WireProtocol};
use ctx::surface::SurfaceId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    path: String,
    surface: SurfaceId,
    protocol: WireProtocol,
    provenance: String,
    live_route_proof: bool,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/model_gateway")
}

#[test]
fn wave_one_capture_corpus_is_explicitly_synthetic_and_content_redacting() {
    let manifest: Manifest = serde_json::from_str(
        &fs::read_to_string(fixture_dir().join("manifest.json")).expect("read manifest"),
    )
    .expect("valid manifest");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.fixtures.len(), 3);

    for fixture in manifest.fixtures {
        assert_eq!(fixture.provenance, "synthetic-redaction-contract");
        assert!(!fixture.live_route_proof);
        assert_ne!(fixture.protocol, WireProtocol::Unknown);
        let raw_bytes = fs::read(fixture_dir().join(&fixture.path)).expect("read fixture");
        let capture: RawCapture = serde_json::from_slice(&raw_bytes).expect("parse fixture");
        assert_eq!(capture.surface, fixture.surface);

        let sanitized = sanitize(&capture);
        let output = serde_json::to_string_pretty(&sanitized).expect("serialize receipt");
        assert!(
            !output.contains("CTX_M0_SECRET"),
            "{} leaked content",
            fixture.path
        );
        assert!(sanitized.content_redacted);
        assert!(!sanitized.persisted_by_harness);
        assert_eq!(sanitized.signals.tool_call_count, 1);
        assert_eq!(sanitized.signals.tool_result_count, 1);
        assert_eq!(sanitized.signals.correlations.len(), 1);
        assert!(sanitized.signals.correlations[0].call_seen);
        assert!(sanitized.signals.correlations[0].result_seen);
    }
}

#[test]
fn probes_read_only_allowlisted_presence_and_do_not_change_profiles() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".codex")).unwrap();
    fs::write(
        home.path().join(".codex/config.toml"),
        "openai_base_url = \"https://CTX_M0_SECRET_UPSTREAM/v1\"\nforced_login_method = \"api\"\n",
    )
    .unwrap();
    fs::write(
        home.path().join(".codex/auth.json"),
        "{\"token\":\"CTX_M0_SECRET_AUTH\"}",
    )
    .unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(
        home.path().join(".claude/settings.json"),
        "{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://CTX_M0_SECRET_CLAUDE\",\"ANTHROPIC_API_KEY\":\"CTX_M0_SECRET_KEY\"}}",
    )
    .unwrap();
    fs::create_dir_all(home.path().join(".cursor")).unwrap();
    fs::write(home.path().join(".cursor/hooks.json"), "{\"version\":1}").unwrap();

    let before = snapshot(home.path());
    let receipts = [
        probe::inspect(SurfaceId::Codex, home.path(), Some("codex 1.0".into())),
        probe::inspect(
            SurfaceId::ClaudeCode,
            home.path(),
            Some("claude 1.0".into()),
        ),
        probe::inspect(SurfaceId::Cursor, home.path(), Some("cursor 1.0".into())),
    ];
    let after = snapshot(home.path());
    assert_eq!(before, after);

    assert_eq!(receipts[0].status, ProbeStatus::ReadyForCapture);
    assert_eq!(receipts[0].decision, RouteDecision::Narrow);
    assert_eq!(receipts[1].status, ProbeStatus::ReadyForCapture);
    assert_eq!(receipts[2].status, ProbeStatus::NeedsManualCapture);
    assert_eq!(receipts[2].decision, RouteDecision::Hold);
    for receipt in receipts {
        assert!(!receipt.client_process_executed);
        assert!(!receipt.client_state_mutation_possible);
        assert!(!receipt.ctx_state_mutated);
        assert!(!receipt.credential_contents_read);
        let output = serde_json::to_string(&receipt).unwrap();
        assert!(!output.contains("CTX_M0_SECRET"));
        assert!(!output.contains(home.path().to_string_lossy().as_ref()));
    }
}

#[test]
fn stale_configuration_is_not_reported_as_an_installed_client() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".codex")).unwrap();
    fs::write(
        home.path().join(".codex/config.toml"),
        "openai_base_url = \"http://127.0.0.1:9999/v1\"\n",
    )
    .unwrap();

    let receipt = probe::inspect(SurfaceId::Codex, home.path(), None);
    assert!(!receipt.installed);
    assert_eq!(receipt.status, ProbeStatus::NotInstalled);
    assert_eq!(receipt.decision, RouteDecision::Hold);
}

#[test]
fn claude_cloud_mode_cannot_activate_the_anthropic_messages_route() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(
        home.path().join(".claude/settings.json"),
        "{\"env\":{\"CLAUDE_CODE_USE_BEDROCK\":\"1\"}}",
    )
    .unwrap();

    let receipt = probe::inspect(
        SurfaceId::ClaudeCode,
        home.path(),
        Some("claude 1.0".into()),
    );
    assert_eq!(receipt.status, ProbeStatus::NeedsManualCapture);
    assert_eq!(receipt.decision, RouteDecision::Hold);
    assert_eq!(receipt.protocol, WireProtocol::Unknown);
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(current)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}
