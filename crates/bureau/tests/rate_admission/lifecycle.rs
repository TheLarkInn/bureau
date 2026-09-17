use std::sync::Arc;
use std::time::Duration;

use bureau::config::Limits;
use bureau::state::{FreshClaim, LeaseOwner, Store};
use rusqlite::Connection;

use super::support::{ASSIGNMENT, Fixture, TTL, TestDir};

fn historical_run(path: &std::path::Path) {
    let store = Arc::new(Store::open(path).expect("store"));
    let owner = LeaseOwner::new(store, ASSIGNMENT, "github", "item", "run").expect("owner");
    owner.claim(TTL).expect("first admission");
    Connection::open(path)
        .expect("time fixture")
        .execute("UPDATE run_admissions SET admitted_at_ms = 7", [])
        .expect("historical admission");
    owner.renew(TTL).expect("renew");
    owner.release().expect("release");
}

#[test]
fn release_renewal_restart_and_recovery_preserve_the_first_timestamp() {
    let directory = TestDir::new("recovery");
    let path = directory.path().join("state.db");
    historical_run(&path);
    let store = Store::open(&path).expect("reopen after all owners and connections closed");
    let resumed = store
        .resume_claim(ASSIGNMENT, "github", "item", "run", TTL)
        .expect("resume");
    let conn = Connection::open(&path).expect("accounting");
    let rows: (u32, i64) = conn
        .query_row(
            "SELECT COUNT(*), MIN(admitted_at_ms) FROM run_admissions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("persistent accounting");
    assert_eq!((resumed, rows), (true, (1, 7)));
}

#[test]
fn immediate_lease_expiry_does_not_erase_a_rate_charge() {
    let fixture = Fixture::new("expired");
    let first = fixture.owner(0, "item", "expired-run");
    first.claim(Duration::ZERO).expect("expired admission");
    let second = fixture.owner(1, "item", "new-run");
    let limits = Limits {
        max_runs_per_hour: Some(1),
        ..Limits::default()
    };
    let attempted = fixture.claim(&second, &limits);
    let budget = fixture.stores[0].budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (attempted, budget.live_leases, budget.runs_this_hour),
        (None, 0, 1)
    );
}

#[test]
fn losing_an_item_claim_does_not_charge_a_second_run() {
    let fixture = Fixture::new("busy");
    let first = fixture.owner(0, "item", "first");
    let second = fixture.owner(1, "item", "second");
    let claims = [
        fixture.claim(&first, &Limits::default()),
        fixture.claim(&second, &Limits::default()),
    ];
    let budget = fixture.stores[0].budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (claims, budget.runs_this_hour),
        ([Some(FreshClaim::Claimed), Some(FreshClaim::Busy)], 1)
    );
}

#[test]
fn refused_or_ignored_accounting_writes_roll_back_the_lease() {
    for failure in ["ABORT, 'injected failure'", "IGNORE"] {
        let fixture = Fixture::new("accounting-failure");
        fixture.execute(&format!(
            "CREATE TRIGGER fail_admission BEFORE INSERT ON run_admissions
             BEGIN SELECT RAISE({failure}); END;"
        ));
        let owner = fixture.owner(0, "item", "run");
        let result = owner.claim(TTL);
        let budget = fixture.stores[0].budget(ASSIGNMENT).expect("budget");
        assert_eq!(
            (
                result.is_err(),
                owner.owns().expect("ownership"),
                budget.runs_this_hour
            ),
            (true, false, 0),
            "injected {failure}"
        );
    }
}

#[test]
fn reopening_does_not_recreate_missing_accounting_history() {
    let directory = TestDir::new("lost-history");
    let path = directory.path().join("state.db");
    historical_run(&path);
    let conn = Connection::open(&path).expect("fault injection");
    conn.execute("DROP TABLE run_admissions", [])
        .expect("lost history");
    let refused = Store::open(&path).is_err();
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'run_admissions')",
            [],
            |row| row.get(0),
        )
        .expect("schema remains unchanged");
    assert_eq!((refused, exists), (true, false));
}
