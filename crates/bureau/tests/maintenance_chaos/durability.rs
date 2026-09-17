use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bureau::runlog::{self, EventKind, RunLog, output, run_started};
use bureau::state::{Error, LeaseOwner, Store};

use super::fixture::TestDir;

const TTL: Duration = Duration::from_secs(60);

fn owner(store: &Arc<Store>, item: &str, run: &str) -> LeaseOwner {
    LeaseOwner::new(store.clone(), "durability", "github", item, run).expect("lease owner")
}

fn expire(database: &Path) {
    rusqlite::Connection::open(database)
        .expect("failure-injection connection")
        .execute("UPDATE leases SET expires_at_ms = 0", [])
        .expect("inject expiry without wall-clock sleeps");
}

fn rejected_append(old: &LeaseOwner) -> bool {
    let called = Cell::new(false);
    let result = old.with_ownership(|| {
        called.set(true);
        Ok(())
    });
    matches!(result, Err(Error::LeaseLost(_))) && !called.get()
}

fn stale_actions(old: &LeaseOwner, seed: u32) {
    let mut actions = [0, 1, 2];
    actions.rotate_left(usize::try_from(seed % 3).expect("small rotation"));
    for action in actions {
        let rejected = match action {
            0 => !old.renew(TTL).expect("stale renewal"),
            1 => old.release().is_ok(),
            2 => rejected_append(old),
            _ => unreachable!("fixed action list"),
        };
        assert!(rejected, "state={seed} stale action={action}");
    }
}

pub(super) fn lease_takeover(seed: u32) {
    let directory = TestDir::new(seed);
    let database = directory.path().join("state.db");
    let first = Arc::new(Store::open(&database).expect("first connection"));
    let old = owner(&first, "1", "same-run");
    assert!(old.claim(TTL).expect("original claim"));
    expire(&database);
    let reopened = Arc::new(Store::open(&database).expect("restarted connection"));
    let current = owner(&reopened, "1", "same-run");
    assert!(current.claim(TTL).expect("replacement generation"));
    stale_actions(&old, seed);
    let failed_write = current.with_ownership::<()>(|| Err(io::Error::other("injected fsync")));
    assert_eq!(
        (
            failed_write.expect_err("write failure").to_string(),
            current.owns().expect("replacement ownership"),
            reopened.active("durability").expect("live claims").len(),
        ),
        ("injected fsync".to_owned(), true, 1),
        "state={seed}: stale release and failed writes must not free the replacement"
    );
}

fn write_log(runs: &Path, seed: u32) -> std::path::PathBuf {
    let mut log = RunLog::create(runs, "logged", &[]).expect("run log");
    log.append(EventKind::RunStarted, run_started("logged", "durability"))
        .expect("run start");
    for index in 0..1 + seed % 4 {
        log.append(
            EventKind::Output,
            output(None, "stdout", &format!("record-{index}")),
        )
        .expect("output");
    }
    let directory = log.dir().to_path_buf();
    log.close().expect("close log");
    directory
}

fn append(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new().append(true).open(path).expect("append");
    file.write_all(bytes).expect("injected bytes");
    file.sync_all().expect("durable injected bytes");
}

fn cache_and_torn_tail(directory: &Path, seed: u32) {
    let before = runlog::replay_state(directory).expect("intact replay");
    fs::write(directory.join("state.json"), b"invalid disposable cache").expect("stale cache");
    let tail = br#"{"seq":99,"at_ms":0,"kind":"output","data":{}}"#;
    let cut = 1 + usize::try_from(seed).expect("seed fits usize") % (tail.len() - 1);
    append(&directory.join("events.jsonl"), &tail[..cut]);
    let replayed = runlog::replay_state(directory).expect("ignore only the unframed tail");
    fs::remove_file(directory.join("state.json")).expect("lost cache");
    assert_eq!(
        (replayed, runlog::replay_state(directory).expect("cache-free replay")),
        (before.clone(), before),
        "state={seed} torn-tail cut={cut}"
    );
}

fn resume_tail(directory: &Path, seed: u32) {
    let before = runlog::read_events(directory).expect("committed prefix");
    let mut log = RunLog::resume(directory, &[]).expect("resume truncated tail");
    let sequence = log
        .append(EventKind::Output, output(None, "stdout", "resumed"))
        .expect("append after restart");
    log.close().expect("close resumed log");
    let after = runlog::read_events(directory).expect("resumed events");
    assert_eq!(
        (
            usize::try_from(sequence).expect("small sequence"),
            after.len(),
            &after[..before.len()]
        ),
        (before.len(), before.len() + 1, before.as_slice()),
        "state={seed}: restart must keep the exact committed prefix"
    );
}

fn corrupt_record(seed: u32) -> &'static [u8] {
    match seed % 3 {
        0 => b"{not-json}\n",
        1 => b"{\"seq\":999,\"at_ms\":0,\"kind\":\"output\",\"data\":{}}\n",
        _ => b"{\"seq\":0,\"at_ms\":0,\"kind\":\"unknown\",\"data\":{}}\n",
    }
}

fn rejected_corruption(root: &Path, directory: &Path, seed: u32) {
    let events = directory.join("events.jsonl");
    append(&events, corrupt_record(seed));
    let before = fs::read(&events).expect("corrupt authoritative bytes");
    let store = Arc::new(Store::open(&root.join("state.db")).expect("fresh admission"));
    let candidate = owner(&store, "new", "candidate");
    let admission = candidate.claim_fresh(TTL, &root.join("runs"));
    assert_eq!(
        (
            runlog::replay_state(directory).expect_err("corrupt record").kind(),
            admission.is_err(),
            candidate.owns().expect("no claim after corruption"),
            fs::read(&events).expect("preserved evidence")
        ),
        (io::ErrorKind::InvalidData, true, false, before),
        "state={seed}: framed corruption must block replay and fresh admission"
    );
}

pub(super) fn replay_restart(seed: u32) {
    let root = TestDir::new(seed);
    let directory = write_log(&root.path().join("runs"), seed);
    cache_and_torn_tail(&directory, seed);
    resume_tail(&directory, seed);
    rejected_corruption(root.path(), &directory, seed);
}
