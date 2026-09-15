use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::{Error, FreshClaim, LeaseOwner, Store};
use crate::runlog::{EventKind, RunLog, output, run_started};

mod authority;
mod progress;

struct Fixture {
    root: PathBuf,
    owner: LeaseOwner,
    log: RunLog,
}

impl Fixture {
    fn new() -> Self {
        let id = crate::identity::random_hex().expect("test identity");
        let root = std::env::temp_dir().join(format!("bureau-admission-progress-{id}"));
        std::fs::create_dir(&root).expect("fixture root");
        let store = Arc::new(Store::open(&root.join("state.db")).expect("writer database"));
        // A zero wait makes progress depend on actual ownership, not a timing threshold.
        store
            .lock()
            .busy_timeout(Duration::ZERO)
            .expect("nonblocking writer");
        let owner = LeaseOwner::new(store, "writer", "github", "1", "writer").expect("owner");
        owner.claim(Duration::from_secs(30)).expect("writer claim");
        let mut log = RunLog::create(&root.join("runs"), "writer", &[]).expect("writer log");
        log.append(EventKind::RunStarted, run_started("writer", "writer"))
            .expect("header");
        Self { root, owner, log }
    }

    fn close(self) {
        let Self { root, owner, log } = self;
        drop((owner, log));
        std::fs::remove_dir_all(root).expect("remove closed fixture");
    }
}

fn pause(ready: mpsc::Sender<()>, resume: mpsc::Receiver<()>) -> impl FnMut() {
    let mut channels = Some((ready, resume));
    move || {
        if let Some((ready, resume)) = channels.take() {
            ready.send(()).expect("replay suspended");
            resume
                .recv_timeout(Duration::from_secs(10))
                .expect("resume replay");
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Observe,
    Claim,
}

impl Operation {
    fn run(self, root: &Path, before_replay: impl FnMut()) -> Result<bool, Error> {
        let store = Arc::new(Store::open(&root.join("state.db"))?);
        let runs = root.join("runs");
        match self {
            Self::Observe => store
                .observe(&runs, "new", "ado", before_replay)
                .map(|work| work.is_empty()),
            Self::Claim => LeaseOwner::new(store, "new", "ado", "2", "candidate")?
                .claim_fresh_with(Duration::from_secs(30), &runs, before_replay)
                .map(|claim| claim == FreshClaim::Claimed),
        }
    }
}

fn progress(operation: Operation) -> (bool, bool) {
    let mut fixture = Fixture::new();
    let root = fixture.root.clone();
    let (ready, waiting) = mpsc::channel();
    let (resume, suspended) = mpsc::channel();
    let observer = std::thread::spawn(move || operation.run(&root, pause(ready, suspended)));
    waiting
        .recv_timeout(Duration::from_secs(10))
        .expect("replay reached");
    let written = fixture.owner.with_ownership(|| {
        fixture.log.append(
            EventKind::Output,
            output(Some("writer"), "combined", "progress"),
        )
    });
    resume.send(()).expect("release replay");
    let admitted = observer
        .join()
        .expect("observer thread")
        .expect("admission result");
    fixture.close();
    (written.is_ok(), admitted)
}

#[test]
fn owned_output_progresses_while_observation_replay_is_suspended() {
    assert_eq!(progress(Operation::Observe), (true, true));
}

#[test]
fn owned_output_progresses_while_fresh_claim_replay_is_suspended() {
    assert_eq!(progress(Operation::Claim), (true, true));
}
