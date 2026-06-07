//! Dashboard fragment stitch: output contains required DOM contracts.

use std::path::PathBuf;
use std::process::Command;

fn ctx_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Count `<div` opens and `</div>` closes in a slice (HTML fragments only).
fn div_balance(section: &str) -> i32 {
    let opens = section.matches("<div").count() as i32;
    let closes = section.matches("</div>").count() as i32;
    opens - closes
}

/// Each tab-panel section between consecutive tabs must fully close its divs.
fn assert_tab_sections_balanced(html: &str) {
    // New IA (ADR 0003): Home (context), Proof, Activity (trace) plus setup and dev tabs.
    let tab_ids = [
        "tab-context",
        "tab-proof",
        "tab-profiles",
        "tab-trace",
        "tab-pipeline",
        "tab-experiment",
        "tab-simulate",
        "tab-settings",
    ];
    for pair in tab_ids.windows(2) {
        let start = html
            .find(&format!("id=\"{}\"", pair[0]))
            .unwrap_or_else(|| panic!("missing {}", pair[0]));
        let end = html
            .find(&format!("id=\"{}\"", pair[1]))
            .unwrap_or_else(|| panic!("missing {}", pair[1]));
        let section = &html[start..end];
        let balance = div_balance(section);
        assert_eq!(
            balance, 0,
            "{} section has unbalanced divs (net {balance}); later tabs may be nested inside it",
            pair[0]
        );
    }
}

/// tab-proof must not be nested inside tab-context (regression: a tab failing to close).
fn assert_tab_panels_are_siblings(html: &str) {
    let context = html.find("id=\"tab-context\"").expect("tab-context");
    let proof = html.find("id=\"tab-proof\"").expect("tab-proof");
    let between = &html[context..proof];
    assert_eq!(
        div_balance(between),
        0,
        "tab-context is not closed before tab-proof; all other tabs break"
    );
}

#[test]
fn stitched_dashboard_contains_required_dom_ids() {
    let html_path = ctx_root().join("src/dashboard.html");
    let html = std::fs::read_to_string(&html_path).expect("dashboard.html");
    for id in [
        "onboarding-wrap",
        "wiz-step-1",
        "wiz-step-5",
        "tab-context",
        "ctx-home-proof",
        "tab-proof",
        "proof-list",
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
        "function loadProof(",
        "function renderProofList(",
        "function wizNext(",
        "function finishOnboardingWizard(",
        "function loadExperimentTab(",
        "function loadSimulateTab(",
        "function hookTraceRow(",
    ] {
        assert!(html.contains(sym), "missing symbol {sym}");
    }
    // Retirement of cost/budget pages (CTX-4) must hold: these are gone for good.
    for gone in [
        "id=\"tab-savings\"",
        "id=\"tab-promptstats\"",
        "id=\"budget-modal\"",
        "function loadSavings(",
        "function loadPromptStats(",
    ] {
        assert!(!html.contains(gone), "retired artifact still present: {gone}");
    }
    assert!(
        html.contains(".proof-tool"),
        "missing Proof page CSS in stitched dashboard"
    );
    assert!(
        html.contains(".exp-table"),
        "missing experiment compare table CSS in stitched dashboard"
    );
    assert!(
        html.contains(".pipe-feature-grid"),
        "missing pipeline feature grid CSS in stitched dashboard"
    );
    assert!(
        html.contains(".sim-hero"),
        "missing simulate tab CSS in stitched dashboard"
    );
    assert!(
        html.contains("What would ctx do?"),
        "missing redesigned simulate tab title in stitched dashboard"
    );
    assert!(
        html.contains("id=\"pipe-hero\""),
        "missing pipeline hero in stitched dashboard"
    );
    assert!(
        html.contains(".data-table"),
        "missing shared data-table CSS in stitched dashboard"
    );
    assert_tab_panels_are_siblings(&html);
    assert_tab_sections_balanced(&html);
    let lines = html.lines().count();
    assert!(
        lines > 3000 && lines < 8000,
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
