use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use super::fixture::{Fixture, REPO, REQUEST, claimed, prepared, start};
use crate::github_cloud::record_log::{Error, LEASE_ASSIGNMENT, Log, envelope, lease_key, write};
use crate::github_cloud::records::Dispatch;
use crate::runlog::RunLog;
use crate::state::{LeaseOwner, Store};

fn disk_fixture() -> (Fixture, Connection) {
    let mut fixture = Fixture::new();
    let path = fixture.root().join("fence.db");
    fixture.store = Arc::new(Store::open(&path).expect("disk store"));
    fixture.owner = claimed(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        &lease_key(REPO, REQUEST),
    );
    let connection = Connection::open(path).expect("independent connection");
    connection
        .busy_timeout(Duration::ZERO)
        .expect("nonblocking probe");
    (fixture, connection)
}

fn blocked(connection: &Connection) -> bool {
    if let Err(error) = connection.execute_batch("BEGIN IMMEDIATE") {
        return error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy);
    }
    connection.execute_batch("ROLLBACK").expect("release probe");
    false
}

fn checkpoint(ready: &Sender<()>, resume: &Receiver<()>) -> io::Result<()> {
    ready.send(()).map_err(io::Error::other)?;
    resume
        .recv_timeout(Duration::from_secs(10))
        .map_err(io::Error::other)
}

fn probe(connection: &Connection, seen: &Receiver<()>, resume: &Sender<()>) -> bool {
    seen.recv_timeout(Duration::from_secs(10))
        .expect("filesystem checkpoint");
    let held = blocked(connection);
    resume.send(()).expect("continue filesystem operation");
    held
}

fn append_during_probe(
    mut log: Log,
    owner: &LeaseOwner,
    ready: &Sender<()>,
    resume: &Receiver<()>,
) -> Result<Log, Error> {
    log.append_with(owner, &prepared(), |writer, data| {
        checkpoint(ready, resume)?;
        let sequence = write::event(writer, data)?;
        checkpoint(ready, resume)?;
        Ok(sequence)
    })?;
    Ok(log)
}

fn append_fence(fixture: &Fixture, connection: &Connection) -> (Log, [bool; 2]) {
    let log = fixture.create(&[]);
    let (ready, seen) = mpsc::channel();
    let (resume, continue_write) = mpsc::channel();
    std::thread::scope(|scope| {
        let worker =
            scope.spawn(move || append_during_probe(log, &fixture.owner, &ready, &continue_write));
        let held = [
            probe(connection, &seen, &resume),
            probe(connection, &seen, &resume),
        ];
        let log = worker
            .join()
            .expect("writer thread")
            .expect("fenced append");
        (log, held)
    })
}

#[test]
fn append_and_fsync_hold_off_competing_sqlite_transactions() {
    let (fixture, connection) = disk_fixture();
    let (log, held) = append_fence(&fixture, &connection);
    assert_eq!(
        (held, blocked(&connection), log.state().dispatch),
        ([true, true], false, Dispatch::Prepared)
    );
    log.close().expect("close");
}

#[test]
fn the_constructor_fence_rejects_lost_ownership_before_filesystem_changes() {
    let fixture = Fixture::new();
    let data = envelope::created(start()).expect("created payload");
    fixture.owner.release().expect("lose ownership");
    let result = write::create(&fixture.owner, fixture.root(), REQUEST, &[], data);
    assert_eq!(
        (
            matches!(result, Err(Error::Ownership)),
            fixture.dir().exists()
        ),
        (true, false)
    );
}

#[test]
fn the_resume_fence_rejects_lost_ownership_before_torn_tail_repair() {
    let fixture = Fixture::new();
    fixture.create(&[]).close().expect("close");
    let torn = format!("{}{{\"seq\":1", fixture.raw());
    fixture.write_raw(&torn);
    fixture.owner.release().expect("lose ownership");
    let result = write::resume(&fixture.owner, &fixture.dir(), &[]);
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, torn)
    );
}

#[test]
fn the_raw_append_fence_rechecks_generation_after_takeover() {
    let fixture = Fixture::new();
    fixture.create(&[]).close().expect("close");
    let writer = RunLog::resume(&fixture.dir(), &[]).expect("previous writer");
    let data = envelope::record(&prepared()).expect("prepared payload");
    fixture
        .owner
        .release()
        .expect("release previous generation");
    let _replacement = claimed(
        Arc::clone(&fixture.store),
        LEASE_ASSIGNMENT,
        &lease_key(REPO, REQUEST),
    );
    let before = fixture.raw();
    let result = write::append(&fixture.owner, writer, REQUEST, 1, data, write::event);
    assert_eq!(
        (matches!(result, Err(Error::Ownership)), fixture.raw()),
        (true, before)
    );
}
