//! Subagent cost grouping via GET /api/task-costs.

mod harness;

use harness::CtxHarness;
use serial_test::serial;

#[test]
#[serial]
fn journey_subagent_cost_grouping() {
    let h = CtxHarness::new();
    for _ in 0..3 {
        h.seed_hook_trace("parent-1", None, None, 0.10, true);
    }
    for _ in 0..4 {
        h.seed_hook_trace("child-1a", Some("parent-1"), None, 0.12, true);
    }
    for _ in 0..2 {
        h.seed_hook_trace("child-1b", Some("parent-1"), None, 0.08, true);
    }
    for i in 0..3 {
        h.seed_hook_trace(&format!("solo-{i}"), None, None, 0.05, true);
    }

    let conn = h.open();
    let groups = ctx::db::load_task_costs(&conn).unwrap();
    let parent = groups
        .iter()
        .find(|g| g.parent_session == "parent-1")
        .expect("parent group");
    assert_eq!(parent.total_requests, 9);
    assert!((parent.total_cost_usd - 0.94).abs() < 0.01);
    assert_eq!(parent.children.len(), 3);

    let solo_groups: Vec<_> = groups
        .iter()
        .filter(|g| g.parent_session.starts_with("solo-"))
        .collect();
    assert_eq!(solo_groups.len(), 3);
}
