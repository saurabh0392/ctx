//! Integration tests for experiment plan tick and journal.

mod harness;

use chrono::NaiveDate;
use ctx::experiment_plan::{
    current_day, load_plan, load_state, plan_path, run_tick, save_plan, save_state,
    ExperimentPhase, ExperimentPlan, PhaseConfigPatch, PlanState,
};
use harness::CtxHarness;
use serial_test::serial;

fn write_plan(h: &CtxHarness, started: NaiveDate) {
    let plan = ExperimentPlan {
        started_at: started,
        corpus_path: "/tmp/ctx-test".to_string(),
        mode: "stress_test".to_string(),
        apply_recommendations_on_final_day: false,
        phases: vec![
            ExperimentPhase {
                name: "pre_ctx".to_string(),
                until_day: 2,
                config: PhaseConfigPatch {
                    hooks_enabled: Some(false),
                    active_profile: Some("all".into()),
                    ..Default::default()
                },
            },
            ExperimentPhase {
                name: "ctx_warmup".to_string(),
                until_day: 3,
                config: PhaseConfigPatch {
                    hooks_enabled: Some(true),
                    ..Default::default()
                },
            },
            ExperimentPhase {
                name: "profile_ab".to_string(),
                until_day: 15,
                config: PhaseConfigPatch {
                    ab_test: Some(ctx::config::AbTestConfig {
                        profile_pct: 50,
                        inject_pct: 100,
                        adaptive_pct: 100,
                        coaching_pct: 100,
                        compress_pct: 100,
                        compress_sgr_pct: 100,
                        tool_mix_pct: 100,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    save_plan(&plan).expect("save plan");
    let _ = (plan_path(), h);
}

#[test]
#[serial]
fn journey_experiment_tick_idempotent_phase() {
    let h = CtxHarness::new();
    h.write_config("active_profile = \"all\"\n");
    write_plan(&h, NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());

    save_state(&PlanState::default()).expect("state");

    run_tick(false).expect("first tick");
    let state1 = load_state();
    assert_eq!(state1.last_applied_phase.as_deref(), Some("profile_ab"));

    run_tick(false).expect("second tick");
    let state2 = load_state();
    assert_eq!(state2.last_applied_phase.as_deref(), Some("profile_ab"));

    let cfg = ctx::config::Config::load();
    assert_eq!(cfg.ab_test.as_ref().unwrap().profile_pct, 50);
}

#[test]
#[serial]
fn journey_plan_init_template_parses() {
    let _h = CtxHarness::new();
    ctx::experiment_plan::plan_init("/Users/test/ctx", "ctx").expect("init");
    let plan = load_plan().expect("load");
    assert_eq!(plan.corpus_path, "/Users/test/ctx");
    assert_eq!(plan.phases.len(), 10);
    let day = current_day(&plan, plan.started_at);
    assert_eq!(day, 1);
}
