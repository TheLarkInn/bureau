use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bureau::config::{Access, ForgeKind, Repo};
use bureau::forge::github::cloud::{Client, Error, Response, Transport};
use bureau::github_cloud::{
    Control, ExpectedIdentity, LEASE_ASSIGNMENT, Log, Record, Selection, SelectionRequest, Start,
    State, lease_key, select,
};
use bureau::process::Secret;
use bureau::runlog::ConfigSource;
use bureau::state::{LeaseOwner, Store};
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn definition() -> Value {
    json!({
        "id": "automation-1", "name": "Review", "description": "", "prompt": "Review",
        "created_at": "2026-01-01", "updated_at": "2026-01-01", "created_by": {},
    })
}

fn response(path: &str) -> Value {
    match path {
        "/user" => json!({"id": 17, "login": "runner"}),
        "/cmc_internal/api/agents/repos/example/project/automations/v2" => {
            json!({"automations": [definition()]})
        }
        "/cmc_internal/api/agents/automations/automation-1" => definition(),
        "/cmc_internal/api/agents/tasks/task-1" => json!({
            "id": "task-1", "automation_id": "automation-1",
            "state": "waiting_for_user", "created_at": "2026-01-01",
        }),
        _ => panic!("unexpected offline request: {path}"),
    }
}

struct Offline {
    posts: AtomicUsize,
}

#[async_trait]
impl Transport for Offline {
    async fn send(&self, request: reqwest::Request) -> Result<Response, Error> {
        self.posts.fetch_add(
            usize::from(request.method() == reqwest::Method::POST),
            Ordering::Relaxed,
        );
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&response(request.url().path())).expect("fixture JSON"),
        })
    }
}

fn selection_request() -> SelectionRequest {
    SelectionRequest {
        registry_name: "code".to_owned(),
        repo: Repo {
            url: "https://github.com/example/project".to_owned(),
            forge: ForgeKind::Github,
            access: Access::Push,
            credential: "github-main".to_owned(),
        },
        config_source: ConfigSource {
            remote: "https://github.com/example/config".to_owned(),
            reference: "main".to_owned(),
            commit: "config-commit".to_owned(),
        },
    }
}

fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bureau-cloud-cli-replay-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).expect("fixture directory");
    path
}

pub(super) struct Fixture {
    root: PathBuf,
    store: Arc<Store>,
    client: Client,
    selection: Selection,
    network: Arc<Offline>,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let root = directory();
        let network = Arc::new(Offline {
            posts: AtomicUsize::new(0),
        });
        let client =
            Client::with_transport(Secret::new("synthetic-cloud-cli-secret"), network.clone());
        let selection = select(
            &client,
            &selection_request(),
            &ExpectedIdentity::Login("runner".to_owned()),
        )
        .await
        .expect("verified selection");
        Self {
            store: Arc::new(Store::open(&root.join("state.db")).expect("store")),
            root,
            client,
            selection,
            network,
        }
    }

    pub(super) fn control(&self) -> Control<'_> {
        Control {
            client: &self.client,
            selection: &self.selection,
            store: self.store.clone(),
            root: &self.root,
            secrets: &[],
        }
    }

    fn owner(&self) -> LeaseOwner {
        let owner = LeaseOwner::new(
            self.store.clone(),
            LEASE_ASSIGNMENT,
            "github_cloud",
            &lease_key("example/project", "receipt"),
            "receipt",
        )
        .expect("owner");
        assert!(owner.claim(Duration::from_secs(90)).expect("claim"));
        owner
    }

    fn created(&self) -> (LeaseOwner, Log) {
        let owner = self.owner();
        let start = Start {
            request_id: "receipt".to_owned(),
            scope: self.selection.scope().clone(),
            automation_id: "automation-1".to_owned(),
            definition: definition(),
        };
        let log = Log::create(&self.root, start, &[], &owner).expect("receipt");
        (owner, log)
    }

    pub(super) fn record(&self, records: &[Record]) -> State {
        let (owner, mut log) = self.created();
        for record in records {
            log.append(&owner, record).expect("record");
        }
        let state = log.state().clone();
        log.close().expect("close");
        owner.release().expect("release");
        state
    }

    pub(super) fn posts(&self) -> usize {
        self.network.posts.load(Ordering::Relaxed)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("clean fixture");
    }
}
