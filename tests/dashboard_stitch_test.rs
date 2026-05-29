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
    let tab_ids = [
        "tab-savings",
        "tab-promptstats",
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

/// tab-promptstats must not appear inside tab-savings (regression: missing savings_tail close).
fn assert_tab_panels_are_siblings(html: &str) {
    let savings = html
        .find("id=\"tab-savings\"")
        .expect("tab-savings");
    let prompt = html
        .find("id=\"tab-promptstats\"")
        .expect("tab-promptstats");
    let between = &html[savings..prompt];
    assert_eq!(
        div_balance(between),
        0,
        "tab-savings is not closed before tab-promptstats; all other tabs break"
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
        "tab-savings",
        "savings-story",
        "story-chapter-4",
        "story-meters",
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
        "function buildSavingsStory(",
        "function renderSavingsStory(",
        "function wizNext(",
        "function finishOnboardingWizard(",
        "function loadExperimentTab(",
        "function loadSimulateTab(",
        "function hookTraceRow(",
    ] {
        assert!(html.contains(sym), "missing symbol {sym}");
    }
    let story_pos = html.find("<article id=\"savings-story\"").expect("savings-story");
    let wrap_pos = html.find("id=\"onboarding-wrap\"").expect("onboarding-wrap");
    assert!(
        story_pos < wrap_pos,
        "savings-story must appear before onboarding-wrap (story was nested inside display:none)"
    );
    assert!(
        html.contains(".story-meter-bar-track"),
        "missing savings story meter CSS in stitched dashboard"
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
        lines > 3000 && lines < 6000,
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
