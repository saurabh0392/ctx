//! Act 2: reproducible agent-context benchmark.
//!
//! Honesty first. This harness reports outcome-first metrics for the arms we can measure
//! from the user's own collected labels (`off`, `ctx-heuristic`, `ctx-learned`). The
//! cross-system arms (`native-compaction`, a named competitor) require replaying a fixed
//! session corpus through those systems live; until that runs they are reported as
//! `not measured`, never as a win. Per the plan, ctx may only claim "beats native
//! compaction" once those arms have real data.

use anyhow::Result;
use serde::Serialize;

use crate::db::LabeledDecision;
use crate::learn::{load_model, score_decision, LEARNED_ACT_THRESHOLD};

#[derive(Serialize)]
pub struct ArmResult {
    pub arm: String,
    /// Whether this arm could be measured from collected labels.
    pub measured: bool,
    pub note: Option<String>,
    pub n_acted: usize,
    pub correction_rate: Option<f64>,
    pub reread_rate: Option<f64>,
}

#[derive(Serialize)]
pub struct BenchReport {
    pub total_labels: usize,
    pub arms: Vec<ArmResult>,
}

fn rate(rows: impl Iterator<Item = (i64, i64)>) -> (usize, f64, f64) {
    let mut n = 0usize;
    let mut corr = 0i64;
    let mut rr = 0i64;
    for (c, r) in rows {
        n += 1;
        corr += c;
        rr += r;
    }
    if n == 0 {
        (0, 0.0, 0.0)
    } else {
        (n, corr as f64 / n as f64, rr as f64 / n as f64)
    }
}

pub fn run_report() -> BenchReport {
    let conn = crate::db::open_db().ok();
    let rows: Vec<LabeledDecision> = match conn {
        Some(c) => {
            let _ = crate::db::ensure_schema(&c);
            crate::db::load_joined_decisions(&c)
        }
        None => Vec::new(),
    };

    let mut arms = Vec::new();

    // off: no compression. Baseline outcome rate across the whole corpus.
    let (n_off, cr_off, rr_off) = rate(rows.iter().map(|d| (d.correction, d.reread)));
    arms.push(ArmResult {
        arm: "off".into(),
        measured: !rows.is_empty(),
        note: Some("baseline: tool output untouched".into()),
        n_acted: n_off,
        correction_rate: (!rows.is_empty()).then_some(cr_off),
        reread_rate: (!rows.is_empty()).then_some(rr_off),
    });

    // ctx-heuristic: outcomes among decisions where the heuristic would drop lines.
    let (n_h, cr_h, rr_h) = rate(
        rows.iter()
            .filter(|d| d.lines_drop > 0)
            .map(|d| (d.correction, d.reread)),
    );
    arms.push(ArmResult {
        arm: "ctx-heuristic".into(),
        measured: n_h > 0,
        note: (n_h == 0).then(|| "no heuristic-trimmed decisions collected yet".into()),
        n_acted: n_h,
        correction_rate: (n_h > 0).then_some(cr_h),
        reread_rate: (n_h > 0).then_some(rr_h),
    });

    // ctx-learned: outcomes among decisions the model would act on (low predicted risk).
    match load_model().filter(|m| m.version > 0) {
        Some(model) => {
            let (n_l, cr_l, rr_l) = rate(
                rows.iter()
                    .filter(|d| {
                        d.lines_drop > 0 && score_decision(&model, d) < LEARNED_ACT_THRESHOLD
                    })
                    .map(|d| (d.correction, d.reread)),
            );
            arms.push(ArmResult {
                arm: "ctx-learned".into(),
                measured: n_l > 0,
                note: (n_l == 0)
                    .then(|| "model trained but no decisions cleared the risk bar yet".into()),
                n_acted: n_l,
                correction_rate: (n_l > 0).then_some(cr_l),
                reread_rate: (n_l > 0).then_some(rr_l),
            });
        }
        None => arms.push(ArmResult {
            arm: "ctx-learned".into(),
            measured: false,
            note: Some("no trained model yet (run `ctx context learn`)".into()),
            n_acted: 0,
            correction_rate: None,
            reread_rate: None,
        }),
    }

    // Cross-system arms require live replay through those systems; not yet measured.
    for (arm, note) in [
        (
            "native-compaction",
            "requires replaying the corpus through native compaction",
        ),
        (
            "competitor",
            "requires replaying the corpus through a named competitor",
        ),
    ] {
        arms.push(ArmResult {
            arm: arm.into(),
            measured: false,
            note: Some(note.into()),
            n_acted: 0,
            correction_rate: None,
            reread_rate: None,
        });
    }

    BenchReport {
        total_labels: rows.len(),
        arms,
    }
}

pub fn run(json: bool) -> Result<()> {
    let report = run_report();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("ctx context benchmark");
    println!("  labeled decisions: {}", report.total_labels);
    println!();
    println!(
        "  {:<20} {:>8} {:>14} {:>12}",
        "arm", "n", "correction", "re-read"
    );
    for a in &report.arms {
        match (a.correction_rate, a.reread_rate) {
            (Some(cr), Some(rr)) => println!(
                "  {:<20} {:>8} {:>13.1}% {:>11.1}%",
                a.arm,
                a.n_acted,
                cr * 100.0,
                rr * 100.0
            ),
            _ => println!(
                "  {:<20} {:>8} {:>14} {:>12}",
                a.arm, "-", "not measured", ""
            ),
        }
    }
    println!();
    println!("Honesty: cross-system arms need a live replay harness before any claim.");
    Ok(())
}
