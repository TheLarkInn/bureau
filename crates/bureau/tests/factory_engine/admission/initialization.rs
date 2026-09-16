use std::collections::BTreeMap;
use std::time::Duration;

use bureau::state::{Error, FreshClaim, LeaseOwner};

use super::super::fixture::Fixture;
use super::{fresh, support};

fn unpublished(contents: Option<&str>) -> Fixture {
    let fixture = Fixture::create("success");
    std::fs::create_dir_all(fixture.directory()).expect("writer created the run directory");
    if let Some(contents) = contents {
        std::fs::write(fixture.directory().join("events.jsonl"), contents).expect("prefix");
    }
    fixture
}

fn observed(fixture: &Fixture) -> Result<BTreeMap<String, String>, Error> {
    support::store(fixture).preserved_factory_work(&fixture.engine.runs_dir, "offline", "github")
}

fn candidate(fixture: &Fixture) -> LeaseOwner {
    LeaseOwner::new(support::store(fixture), "offline", "github", "1", "new-run")
        .expect("fresh candidate")
}

fn blocked(fixture: &Fixture) {
    assert_eq!(
        (
            observed(fixture).is_err(),
            candidate(fixture)
                .claim_fresh(Duration::from_secs(30), &fixture.engine.runs_dir)
                .is_err(),
        ),
        (true, true),
    );
}

#[test]
fn live_unpublished_prefixes_are_busy_not_corrupt_and_do_not_block_other_assignments() {
    for prefix in [None, Some(""), Some("{\"seq\":")] {
        let fixture = unpublished(prefix);
        assert_eq!(
            (
                observed(&fixture).expect("live publisher").len(),
                fresh(&fixture, "offline", "github", "1"),
                fresh(&fixture, "another", "ado", "2"),
                bureau::runlog::preserved_factory_work(
                    &fixture.engine.runs_dir,
                    "offline",
                    "github",
                )
                .is_err(),
            ),
            (0, FreshClaim::Busy, FreshClaim::Claimed, true),
        );
    }
}

#[test]
fn an_expired_owner_cannot_hide_an_unpublished_run() {
    let fixture = unpublished(Some("{\"seq\":"));
    rusqlite::Connection::open(fixture.root.join("state.db"))
        .expect("database")
        .execute("UPDATE leases SET expires_at_ms = 0", [])
        .expect("expire owner");
    blocked(&fixture);
}

#[test]
fn a_released_owner_cannot_hide_a_missing_header() {
    let fixture = unpublished(None);
    fixture
        .plan
        .lease
        .as_ref()
        .expect("initial owner")
        .release()
        .expect("released owner");
    blocked(&fixture);
}

#[test]
fn live_ownership_does_not_hide_a_corrupt_complete_record() {
    let fixture = unpublished(Some("invalid event\n"));
    blocked(&fixture);
}

#[test]
fn a_live_first_header_can_end_inside_a_multibyte_character() {
    let fixture = unpublished(None);
    std::fs::write(
        fixture.directory().join("events.jsonl"),
        b"{\"run_id\":\"\xe2",
    )
    .expect("split UTF-8 header");
    assert_eq!(
        (
            observed(&fixture).expect("unpublished header").len(),
            fresh(&fixture, "offline", "github", "1"),
        ),
        (0, FreshClaim::Busy),
    );
}
