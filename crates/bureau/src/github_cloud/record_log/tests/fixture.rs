use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::github_cloud::record_log::{LEASE_ASSIGNMENT, Log, lease_key};
use crate::github_cloud::records::{Record, Scope, Start};
use crate::process::Secret;
use crate::runlog::{self, EVENTS_FILE, Event, EventKind};
use crate::state::{LeaseOwner, Store};

pub(super) const REQUEST: &str = "request-1";
pub(super) const REPO: &str = "owner/repository";
pub(super) const AUTOMATION: &str = "automation:one";
pub(super) const TASK: &str = "task:one";
static NEXT: AtomicU32 = AtomicU32::new(0);

fn scope() -> Scope {
    Scope {
        repo: REPO.to_owned(),
        registry_name: "work".to_owned(),
        credential_reference: "github-work".to_owned(),
        principal_id: 42,
        principal_login: "reviewer".to_owned(),
        config_source: runlog::ConfigSource {
            remote: "https://github.com/owner/config".to_owned(),
            reference: "refs/heads/main".to_owned(),
            commit: "1111111111111111111111111111111111111111".to_owned(),
        },
    }
}

pub(super) fn start() -> Start {
    Start {
        request_id: REQUEST.to_owned(),
        scope: scope(),
        automation_id: AUTOMATION.to_owned(),
        definition: json!({"id": AUTOMATION, "triggers": {}, "disabled": false}),
    }
}

pub(super) fn task() -> Value {
    json!({
        "id": TASK,
        "automation_id": AUTOMATION,
        "state": "future_remote_state",
        "status": "waiting_for_permission",
        "sessions": [
            {"id": "session:one", "task_id": TASK, "state": "future_session_state"},
            {"id": "session:two", "task_id": TASK, "state": "unknown"}
        ]
    })
}

pub(super) fn selected() -> Record {
    Record::TaskSelected {
        task_id: TASK.to_owned(),
    }
}

pub(super) fn prepared() -> Record {
    Record::Prepared {
        event: "manual".to_owned(),
    }
}

pub(super) fn observation(task: Value, events: Option<Vec<Value>>) -> Record {
    Record::Observed {
        task,
        events,
        reported_total: None,
    }
}

pub(super) fn event(seq: u64, data: Value) -> Event {
    Event {
        seq,
        at_ms: seq.saturating_add(1) * 100,
        kind: EventKind::GitHubCloud,
        data,
    }
}

pub(super) fn claimed(store: Arc<Store>, assignment: &str, external_id: &str) -> LeaseOwner {
    let owner = LeaseOwner::new(store, assignment, "github_cloud", external_id, REQUEST)
        .expect("lease owner");
    assert!(owner.claim(Duration::from_secs(600)).expect("claim lease"));
    owner
}

pub(super) struct Fixture {
    root: PathBuf,
    pub(super) store: Arc<Store>,
    pub(super) owner: LeaseOwner,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "cloud-records-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let store = Arc::new(Store::open_in_memory().expect("memory store"));
        let owner = claimed(
            Arc::clone(&store),
            LEASE_ASSIGNMENT,
            &lease_key(REPO, REQUEST),
        );
        Self { root, store, owner }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn dir(&self) -> PathBuf {
        self.root.join(REQUEST)
    }

    pub(super) fn create(&self, secrets: &[Secret]) -> Log {
        Log::create(&self.root, start(), secrets, &self.owner).expect("create cloud log")
    }

    pub(super) fn raw(&self) -> String {
        std::fs::read_to_string(self.dir().join(EVENTS_FILE)).expect("raw cloud log")
    }

    pub(super) fn events(&self) -> Vec<Event> {
        runlog::read_events_tolerant(&self.dir()).expect("outer events")
    }

    pub(super) fn write_raw(&self, text: &str) {
        std::fs::write(self.dir().join(EVENTS_FILE), text).expect("fixture log");
    }

    pub(super) fn write_events(&self, events: &[Event]) {
        let lines: Vec<_> = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("event JSON"))
            .collect();
        self.write_raw(&format!("{}\n", lines.join("\n")));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
