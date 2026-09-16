use std::sync::Arc;
use std::time::Duration;

use super::fixture::{Fixture, REPO, REQUEST, claimed, prepared, start};
use crate::github_cloud::record_log::{Error, LEASE_ASSIGNMENT, Log, lease_key};
use crate::state::{LeaseOwner, Store};

#[test]
fn an_unclaimed_generation_cannot_create_a_log() {
    let fixture = Fixture::new();
    let owner = LeaseOwner::new(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        "github_cloud",
        &lease_key(REPO, REQUEST),
        REQUEST,
    )
    .expect("unclaimed owner");
    let result = Log::create(fixture.root(), start(), &[], &owner);
    assert_eq!(
        (
            matches!(result, Err(Error::Ownership)),
            fixture.dir().exists()
        ),
        (true, false)
    );
}

#[test]
fn a_live_unrelated_assignment_or_external_id_is_not_authority() {
    let fixture = Fixture::new();
    for (assignment, external) in [
        ("other-controls", lease_key(REPO, REQUEST)),
        (LEASE_ASSIGNMENT, lease_key(REPO, "another-request")),
    ] {
        let owner = claimed(Arc::clone(&fixture.store), assignment, &external);
        let result = Log::create(fixture.root(), start(), &[], &owner);
        assert_eq!(
            (
                matches!(result, Err(Error::Ownership)),
                fixture.dir().exists()
            ),
            (true, false)
        );
    }
}

#[test]
fn append_requires_the_receipts_own_live_lease() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let other = claimed(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        "owner/repository/other",
    );
    let before = fixture.raw();
    let result = log.append(&other, &prepared());
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, before)
    );
    log.close().expect("close");
}

#[test]
fn expired_ownership_cannot_append_or_resume() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let expired = fixture.owner.renew(Duration::ZERO).expect("expire lease");
    let append = log.append(&fixture.owner, &prepared());
    let open = Log::open(fixture.root(), REQUEST, &[], &fixture.owner);
    assert_eq!(
        (
            expired,
            matches!(append, Err(Error::Ownership)),
            matches!(open, Err(Error::Ownership))
        ),
        (true, true, true)
    );
    log.close().expect("close");
}

#[test]
fn a_replaced_generation_cannot_write_after_lease_loss() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    fixture.owner.release().expect("release old generation");
    let replacement = claimed(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        &lease_key(REPO, REQUEST),
    );
    let before = fixture.raw();
    let result = log.append(&fixture.owner, &prepared());
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, before)
    );
    log.close().expect("close");
    Log::open(fixture.root(), REQUEST, &[], &replacement)
        .expect("new owner opens")
        .close()
        .expect("close");
}

#[test]
fn wrong_owner_cannot_repair_a_torn_tail() {
    let fixture = Fixture::new();
    fixture.create(&[]).close().expect("close");
    let raw = format!("{}{{\"seq\":1", fixture.raw());
    fixture.write_raw(&raw);
    let other = claimed(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        "owner/repository/other",
    );
    let result = Log::open(fixture.root(), REQUEST, &[], &other);
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, raw)
    );
}

#[test]
fn lost_owner_cannot_repair_a_torn_tail() {
    let fixture = Fixture::new();
    fixture.create(&[]).close().expect("close");
    let raw = format!("{}{{\"seq\":1", fixture.raw());
    fixture.write_raw(&raw);
    fixture.owner.release().expect("release");
    let result = Log::open(fixture.root(), REQUEST, &[], &fixture.owner);
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, raw)
    );
}

#[test]
fn unprovable_ownership_is_a_typed_failure_before_append() {
    let fixture = Fixture::new();
    let path = fixture.root().join("state.db");
    let store = Arc::new(Store::open(&path).expect("disk store"));
    let owner = claimed(store, LEASE_ASSIGNMENT, &lease_key(REPO, REQUEST));
    let mut log = Log::create(fixture.root(), start(), &[], &owner).expect("log");
    let connection = rusqlite::Connection::open(path).expect("fixture connection");
    connection
        .execute("DROP TABLE leases", ())
        .expect("break ownership query");
    let before = fixture.raw();
    let result = log.append(&owner, &prepared());
    assert_eq!(
        (
            matches!(result, Err(Error::OwnershipCheck(_))),
            fixture.raw()
        ),
        (true, before)
    );
    log.close().expect("close");
}
