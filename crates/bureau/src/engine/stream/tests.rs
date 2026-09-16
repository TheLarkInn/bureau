use std::fs;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use super::{LogSink, Shared, with_log};
use crate::runlog::{self, EventKind, RunLog};
use crate::state::{LeaseOwner, Store};

struct Fixture {
    root: PathBuf,
    store: Arc<Store>,
    competing: Connection,
}

impl Fixture {
    fn new() -> Self {
        let id = crate::identity::random_hex().expect("fixture identity");
        let root = std::env::temp_dir().join(format!("bureau-output-fence-{id}"));
        let database = root.join("state.db");
        let store = Arc::new(Store::open(&database).expect("store"));
        let competing = Connection::open(database).expect("second connection");
        competing
            .busy_timeout(Duration::ZERO)
            .expect("nonblocking takeover");
        Self {
            root,
            store,
            competing,
        }
    }

    fn owner(&self) -> LeaseOwner {
        let owner = LeaseOwner::new(self.store.clone(), "assignment", "github", "item", "run")
            .expect("owner");
        assert!(owner.claim(Duration::from_secs(60)).expect("claim"));
        owner
    }

    fn new_log(&self) -> Shared {
        Arc::new(Mutex::new(
            RunLog::create(&self.root, "run", &[]).expect("log"),
        ))
    }

    fn resumed_log(&self) -> Shared {
        let log = RunLog::resume(&self.root.join("run"), &[]).expect("resumed log");
        Arc::new(Mutex::new(log))
    }

    fn chunks(&self) -> Vec<(u64, String)> {
        runlog::read_events(&self.root.join("run"))
            .expect("events")
            .into_iter()
            .map(|event| {
                (
                    event.seq,
                    event.data["data"].as_str().expect("output").to_owned(),
                )
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove fixture");
    }
}

fn expire(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute("UPDATE leases SET expires_at_ms = 0", [])
}

fn racing_output(log: &mut RunLog, connection: &Connection) -> io::Result<Option<ErrorCode>> {
    let blocked = expire(connection)
        .err()
        .and_then(|error| error.sqlite_error_code());
    log.append(
        EventKind::Output,
        runlog::output(Some("step"), "combined", "first"),
    )?;
    Ok(blocked)
}

#[test]
fn output_append_blocks_takeover_and_stale_writer_cannot_corrupt_the_new_sequence() {
    let fixture = Fixture::new();
    let old = fixture.owner();
    let log = fixture.new_log();
    let mut stale = LogSink::new("step", &log, Some(old.clone()));
    let blocked = with_log(&log, Some(&old), |log| {
        racing_output(log, &fixture.competing)
    })
    .expect("fenced output");
    expire(&fixture.competing).expect("expiry after completed output");
    let current = fixture.owner();
    let mut sink = LogSink::new("step", &fixture.resumed_log(), Some(current));
    let error = stale
        .write_all(b"stale")
        .expect_err("old owner cannot append");
    sink.write_all(b"current").expect("current output");
    assert_eq!(
        (blocked, error.kind(), fixture.chunks()),
        (
            Some(ErrorCode::DatabaseBusy),
            io::ErrorKind::PermissionDenied,
            vec![(0, "first".into()), (1, "current".into())]
        ),
    );
}

#[test]
fn ordinary_output_without_a_supervisor_owner_still_appends() {
    let fixture = Fixture::new();
    let mut sink = LogSink::new("step", &fixture.new_log(), None);
    sink.write_all(b"ordinary").expect("output");
    sink.flush().expect("flush");
    assert_eq!(fixture.chunks(), vec![(0, "ordinary".into())]);
}
