use std::sync::Arc;
use std::time::Duration;

use bureau::config::Limits;
use bureau::state::{LeaseOwner, Store};

use super::fresh;
use crate::cli::run::tests::{Fixture, assignment, item};

fn attempt(fixture: &Fixture, limits: Limits, id: &str, prs: usize) -> Option<LeaseOwner> {
    let mut assignment = assignment();
    assignment.limits = limits;
    fresh(
        fixture.store.clone(), &assignment, &item(id), &format!("run-{id}"),
        &fixture.runs(), prs,
    ).expect("explicit fresh admission")
}

fn occupied(fixture: &Fixture) {
    fixture.store.record_run("completed", "manual", 1.0).expect("recorded budget");
    assert!(fixture.store.try_claim("manual", "github", "held", Duration::from_secs(60))
        .expect("preexisting active work"));
}

fn configured_limits() -> [Limits; 5] {
    [
        "max_concurrent: 1",
        "max_runs_per_hour: 1",
        "max_runs_per_day: 1",
        "max_open_prs: 1",
        "max_cost_per_day_usd: 1",
    ].map(|text| serde_yaml_ng::from_str(text).expect("configured limit"))
}

#[test]
fn every_configured_limit_blocks_explicit_fresh_work() {
    let fixture = Fixture::new();
    occupied(&fixture);
    for limits in configured_limits() {
        let owner = attempt(&fixture, limits, "candidate", 1);
        assert_eq!(
            (owner.is_none(), fixture.store.active("manual").expect("leases").len(),
             fixture.runs().exists()),
            (true, 1, false),
            "blocked run/retry must not claim or create a run directory"
        );
    }
}

#[test]
fn omitted_limits_preserve_explicit_admission() {
    let fixture = Fixture::new();
    occupied(&fixture);
    let owner = attempt(&fixture, Limits::default(), "candidate", usize::MAX)
        .expect("omitted limits remain unlimited");
    owner.release().expect("release admitted work");
    assert_eq!(fixture.store.active("manual").expect("leases").len(), 1);
}

#[test]
fn explicit_run_and_retry_share_capacity_with_another_connection() {
    let fixture = Fixture::new();
    let limits = Limits { max_concurrent: Some(1), ..Limits::default() };
    let current = attempt(&fixture, limits.clone(), "running", 0).expect("first run");
    let other = Arc::new(Store::open(&fixture.root().join("state.db")).expect("other process"));
    let mut assignment = assignment();
    assignment.limits = limits.clone();
    let denied = fresh(other, &assignment, &item("retry"), "retried", &fixture.runs(), 0)
        .expect("retry admission");
    current.release().expect("completed current work");
    let retry = attempt(&fixture, limits, "retry", 0).expect("released capacity is reusable");
    assert_eq!(
        (denied.is_none(), retry.owns().expect("retry owner"),
         fixture.store.active("manual").expect("leases").len()),
        (true, true, 1)
    );
    retry.release().expect("release retry");
}

#[test]
fn explicit_budget_read_failure_does_not_create_a_claim() {
    let fixture = Fixture::new();
    rusqlite::Connection::open(fixture.root().join("state.db")).expect("failure injection")
        .execute("DROP TABLE runs", []).expect("unavailable counters");
    let result = fresh(fixture.store.clone(), &assignment(), &item("1"), "candidate",
        &fixture.runs(), 0);
    let error = result.err().expect("budget error must surface");
    assert_eq!(
        (format!("{error:#}").contains("no such table: runs"),
         fixture.store.active("manual").expect("leases").len(), fixture.runs().exists()),
        (true, 0, false)
    );
}
