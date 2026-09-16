use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bureau::config::{Access, ForgeKind, Repo};
use bureau::forge::github::cloud::{Client, Error, Response};
use bureau::github_cloud::{
    Control, ExpectedIdentity, LEASE_ASSIGNMENT, Log, Selection, SelectionRequest, Start,
    lease_key, select,
};
use bureau::runlog::ConfigSource;
use bureau::state::{LeaseOwner, Store};
use serde_json::json;

use super::support::{self, Fake};

static NEXT: AtomicU32 = AtomicU32::new(0);

pub fn request(access: Access) -> SelectionRequest {
    SelectionRequest {
        registry_name: "code".to_owned(),
        repo: Repo {
            url: "https://github.com/example/project".to_owned(),
            forge: ForgeKind::Github,
            access,
            credential: "github-main".to_owned(),
        },
        config_source: ConfigSource {
            remote: "https://github.com/example/config".to_owned(),
            reference: "main".to_owned(),
            commit: "config-commit".to_owned(),
        },
    }
}

fn new_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bureau-cloud-control-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).expect("fixture directory");
    path
}

pub struct Fixture {
    directory: PathBuf,
    pub root: PathBuf,
    pub store: Arc<Store>,
    pub client: Client,
    pub fake: Arc<Fake>,
    pub selection: Selection,
}

impl Fixture {
    pub async fn with_access(replies: Vec<Result<Response, Error>>, access: Access) -> Self {
        let directory = new_directory();
        let store = Arc::new(Store::open(&directory.join("state.db")).expect("store"));
        let mut script = vec![support::response(
            200,
            &json!({"id": 17, "login": "runner"}),
        )];
        script.extend(replies);
        let (client, fake) = Fake::client(script);
        let expected = ExpectedIdentity::Login("runner".to_owned());
        let selection = select(&client, &request(access), &expected)
            .await
            .expect("selection");
        Self {
            root: directory.join("cloud-runs"),
            directory,
            store,
            client,
            fake,
            selection,
        }
    }

    pub async fn new(replies: Vec<Result<Response, Error>>) -> Self {
        Self::with_access(replies, Access::Push).await
    }

    pub fn control(&self) -> Control<'_> {
        Control {
            client: &self.client,
            selection: &self.selection,
            store: self.store.clone(),
            root: &self.root,
            secrets: &[],
        }
    }

    pub fn owner(&self, key: &str) -> LeaseOwner {
        let external = lease_key(&self.selection.scope().repo, key);
        LeaseOwner::new(
            self.store.clone(),
            LEASE_ASSIGNMENT,
            "github_cloud",
            &external,
            key,
        )
        .expect("owner")
    }

    pub fn log(&self, key: &str) -> (LeaseOwner, Log) {
        let owner = self.owner(key);
        owner.claim(Duration::from_secs(90)).expect("claim");
        let start = Start {
            request_id: key.to_owned(),
            scope: self.selection.scope().clone(),
            automation_id: "automation-1".to_owned(),
            definition: support::definition(),
        };
        let log = Log::create(&self.root, start, &[], &owner).expect("log");
        (owner, log)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).expect("clean fixture");
    }
}

pub fn accepted_replies() -> Vec<Result<Response, Error>> {
    let mut replies = support::definition_replies();
    replies.push(support::response(
        202,
        &json!({"task_id": "not-a-verified-contract"}),
    ));
    replies
}

pub fn task_replies() -> Vec<Result<Response, Error>> {
    let mut replies = support::definition_replies();
    replies.push(support::response(200, &support::task()));
    replies
}

pub fn posts(requests: &[reqwest::Request]) -> usize {
    requests
        .iter()
        .filter(|request| request.method() == reqwest::Method::POST)
        .count()
}
