use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use super::super::{LeaseOwner, Store};

fn database() -> (PathBuf, Arc<Store>, Connection) {
    let name = crate::identity::random_hex().expect("test identity");
    let path = std::env::temp_dir().join(format!("bureau-admission-{name}.db"));
    let store = Arc::new(Store::open(&path).expect("scheduler database"));
    let contender = Connection::open(&path).expect("competing scheduler connection");
    contender
        .busy_timeout(Duration::ZERO)
        .expect("nonblocking contention");
    (path, store, contender)
}

#[test]
fn fresh_admission_predicate_runs_under_the_exclusive_database_fence() {
    let (path, store, contender) = database();
    let owner = LeaseOwner::new(store.clone(), "a", "github", "1", "r").expect("owner");
    let mut blocked = false;
    let claimed = store.claim_owner_if(&owner, Duration::from_secs(30), |_, _| {
        blocked = matches!(
            contender.execute("UPDATE leases SET expires_at_ms = 0", []),
            Err(rusqlite::Error::SqliteFailure(error, _)) if error.code == ErrorCode::DatabaseBusy
        );
        Ok(false)
    }).expect("declined admission");
    assert_eq!(
        (claimed, blocked, owner.owns().expect("ownership")),
        (false, true, false)
    );
    drop((owner, store, contender));
    std::fs::remove_file(path).expect("remove closed fixture database");
}
