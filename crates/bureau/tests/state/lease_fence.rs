use std::cell::Cell;
use std::sync::Arc;

use bureau::runlog::{EventKind, RunLog};
use bureau::state::{Error, LeaseOwner};

use super::{ASSIGNMENT, Duration, HOUR, Store, TestDir};

fn owner(store: Arc<Store>, run: &str, ttl: Duration) -> LeaseOwner {
    let owner = LeaseOwner::new(store, ASSIGNMENT, "github", "item", run).expect("owner");
    assert!(owner.claim(ttl).expect("claim"));
    owner
}

fn takeover(connection: &rusqlite::Connection) -> rusqlite::Result<usize> {
    connection.execute("UPDATE leases SET owner_id = 'replacement'", [])
}

fn append_with_competitor(
    log: &mut RunLog,
    competing: &rusqlite::Connection,
) -> std::io::Result<Option<rusqlite::ErrorCode>> {
    let blocked = takeover(competing)
        .err()
        .and_then(|error| error.sqlite_error_code());
    log.append(EventKind::Output, serde_json::json!({"message": "durable"}))?;
    Ok(blocked)
}

#[test]
fn live_owner_returns_the_durable_operation_result() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let owner = owner(store, "run", HOUR);
    assert_eq!(owner.with_ownership(|| Ok(42)).expect("fenced append"), 42);
}

#[test]
fn owner_exposes_its_exact_run_and_forge_identity() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let owner = owner(store, "specific-run", HOUR);
    assert_eq!(
        (
            owner.run_id(),
            owner.forge(),
            owner.external_id(),
            owner.assignment()
        ),
        ("specific-run", "github", "item", ASSIGNMENT),
    );
}

#[test]
fn expired_owner_cannot_execute_the_operation() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let owner = owner(store, "run", Duration::ZERO);
    let invoked = Cell::new(false);
    let result = owner.with_ownership(|| {
        invoked.set(true);
        Ok(())
    });
    assert_eq!(
        (matches!(result, Err(Error::LeaseLost(_))), invoked.get()),
        (true, false)
    );
}

#[test]
fn a_replaced_generation_cannot_append() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let old = owner(Arc::clone(&store), "run", Duration::ZERO);
    let current = owner(store, "run", HOUR);
    let old_result = old.with_ownership(|| Ok(()));
    assert_eq!(
        (
            matches!(old_result, Err(Error::LeaseLost(_))),
            current.owns().expect("current owner")
        ),
        (true, true)
    );
}

#[test]
fn write_errors_are_not_acknowledged_or_hidden() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let owner = owner(store, "run", HOUR);
    let result = owner.with_ownership::<()>(|| Err(std::io::Error::other("fsync failed")));
    assert_eq!(
        (
            result.expect_err("write failure").to_string(),
            owner.owns().expect("owner remains")
        ),
        ("fsync failed".to_owned(), true)
    );
}

#[test]
fn another_connection_cannot_take_over_during_checked_append_and_fsync() {
    let temporary = TestDir::new("lease-fence");
    let database = temporary.path().join("state.sqlite");
    let store = Arc::new(Store::open(&database).expect("store"));
    let owner = owner(store, "run", HOUR);
    let competing = rusqlite::Connection::open(&database).expect("other connection");
    competing.busy_timeout(Duration::ZERO).expect("nonblocking");
    let mut log = RunLog::create(&temporary.path().join("runs"), "run", &[]).expect("log");
    let blocked = owner
        .with_ownership(|| append_with_competitor(&mut log, &competing))
        .expect("fenced operation");
    let events = bureau::runlog::read_events(log.dir()).expect("durable append");
    let replaced = takeover(&competing).expect("unlocked");
    assert_eq!(
        (
            blocked,
            events.len(),
            replaced,
            owner.owns().expect("replaced owner")
        ),
        (Some(rusqlite::ErrorCode::DatabaseBusy), 1, 1, false)
    );
}
