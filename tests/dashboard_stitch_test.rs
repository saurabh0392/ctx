//! Dashboard fragment stitch: output contains required DOM contracts.

use std::path::PathBuf;
use std::process::Command;

fn ctx_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn stitched_dashboard_contains_required_dom_ids() {
    let html_path = ctx_root().join("src/dashboard.html");
    let html = std::fs::read_to_string(&html_path).expect("dashboard.html");
    for id in [
        "onboarding-wrap",
        "wiz-step-1",
        "wiz-step-5",
        "tab-savings",
        "tab-promptstats",
        "tab-profiles",
        "tab-trace",
        "tab-pipeline",
        "tab-settings",
        "tab-experiment",
        "tab-simulate",
    ] {
        assert!(html.contains(&format!("id=\"{id}\"")), "missing id={id}");
    }
    for sym in [
        "function wizNext(",
        "function finishOnboardingWizard(",
        "function loadExperimentTab(",
        "function loadSimulateTab(",
        "function hookTraceRow(",
    ] {
        assert!(html.contains(sym), "missing symbol {sym}");
    }
    let lines = html.lines().count();
    assert!(
        lines > 3000 && lines < 3700,
        "unexpected stitched line count: {lines}"
    );
}

#[test]
fn stitch_script_matches_committed_dashboard() {
    let root = ctx_root();
    let script = root.join("scripts/stitch-dashboard.sh");
    assert!(script.is_file(), "stitch-dashboard.sh missing");
    let out = Command::new("bash")
        .arg(&script)
        .current_dir(&root)
        .output()
        .expect("run stitch-dashboard.sh");
    assert!(
        out.status.success(),
        "stitch failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stitched = String::from_utf8(out.stdout).expect("utf8 stdout");
    let committed = std::fs::read_to_string(root.join("src/dashboard.html")).expect("dashboard.html");
    assert_eq!(
        stitched, committed,
        "src/dashboard.html is stale; run make dashboard"
    );
}
