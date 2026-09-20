use std::path::{Path, PathBuf};
use std::sync::Arc;

use bureau::config::{Assignment, Config};
use bureau::contract::Trust;
use bureau::forge::Item;
use bureau::state::Store;

const CONFIG: &str = "
repos:
  local: {url: 'fake://repo', forge: github, access: read, credential: unused}
roles: {}
assignments:
  manual:
    name: manual
    work:
      forge: github
      source: items
      filter: '*'
      abort_label: 'bureau:failed'
      escalate_label: 'bureau:needs-human'
    repos: [local]
    pipeline: check
    role: unused
    verify: 'true'
    branch_prefix: bureau/
pipelines:
  check:
    name: check
    steps: [{name: check, type: deterministic, run: 'true', next: done}]
";

pub(super) fn config() -> Config {
    serde_yaml_ng::from_str(CONFIG).expect("offline manual config")
}

pub(super) fn assignment() -> Assignment {
    config().assignments.remove("manual").expect("assignment")
}

pub(super) fn item(id: &str) -> Item {
    Item {
        external_id: id.to_owned(),
        title: format!("Item {id}"),
        body: "Offline item".to_owned(),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let id = bureau::engine::new_run_id("manual-claim-test").expect("fixture identity");
        let path = std::env::temp_dir().join(id);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(&path).expect("private fixture directory");
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("fixture cleanup failed for {}: {error}", self.0.display());
            assert!(std::thread::panicking(), "fixture cleanup must succeed");
        }
    }
}

pub(super) struct Fixture {
    pub(super) store: Arc<Store>,
    directory: Directory,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = Directory::new();
        let store = Arc::new(Store::open(&directory.0.join("state.db")).expect("database"));
        Self { store, directory }
    }

    pub(super) fn root(&self) -> &Path {
        &self.directory.0
    }

    pub(super) fn runs(&self) -> PathBuf {
        self.root().join("runs")
    }
}
