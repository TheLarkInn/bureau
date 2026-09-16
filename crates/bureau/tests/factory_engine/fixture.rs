use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bureau::engine::{Engine, RunPlan, new_run_id};
use bureau::runlog::copilot_factory::{Record, Records};
use bureau::state::{LeaseOwner, Store};
use serde_json::Value;

#[path = "source.rs"]
mod source;

pub use source::{commit, git, write};

pub fn lease(root: &Path, plan: &RunPlan) -> LeaseOwner {
    let store = Arc::new(Store::open(&root.join("state.db")).expect("state store"));
    let owner = LeaseOwner::new(
        store,
        &plan.assignment.name,
        "github",
        &plan.item.external_id,
        &plan.run_id,
    )
    .expect("owner");
    assert!(owner.claim(Duration::from_secs(120)).expect("claim"));
    owner
}

pub struct Fixture {
    pub root: PathBuf,
    pub plan: RunPlan,
    pub engine: Engine,
    cleanup: bool,
}

impl Fixture {
    pub fn at(root: PathBuf, mode: &str) -> Self {
        assert!(root.is_absolute(), "fixture directory must be absolute");
        std::fs::create_dir_all(root.parent().expect("fixture parent")).expect("fixture parent");
        std::fs::create_dir(&root).expect("new native fixture directory; refusing replacement");
        source::bureau_executable(&root);
        let repository = source::repository(&root);
        let runtime = source::runtime(&root, mode);
        let mut plan = super::plan::build(&repository, &runtime, mode);
        plan.lease = Some(lease(&root, &plan));
        let engine = Engine::new(root.join("runs"), root.join("cache"));
        Self {
            root,
            plan,
            engine,
            cleanup: true,
        }
    }

    pub fn create(mode: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(new_run_id("factory-engine").expect("fixture identity"));
        Self::at(root, mode)
    }

    pub const fn retain(&mut self) {
        self.cleanup = false;
    }

    pub fn directory(&self) -> PathBuf {
        self.engine.runs_dir.join(&self.plan.run_id)
    }

    pub fn events(&self) -> Vec<bureau::runlog::Event> {
        bureau::runlog::read_events_tolerant(&self.directory()).expect("events")
    }

    pub fn record(&self) -> Record {
        Records::replay(&self.events())
            .expect("strict replay")
            .0
            .into_values()
            .next()
            .expect("factory record")
    }

    pub fn trace(&self) -> Vec<Value> {
        std::fs::read_to_string(
            self.record()
                .intent
                .paths
                .storage
                .copilot_home
                .join("trace.jsonl"),
        )
        .expect("wire trace")
        .lines()
        .map(|line| serde_json::from_str(line).expect("trace record"))
        .collect()
    }

    pub fn count(&self, kind: bureau::runlog::EventKind) -> usize {
        self.events()
            .iter()
            .filter(|event| event.kind == kind)
            .count()
    }

    pub fn calls(&self, method: &str) -> usize {
        self.trace()
            .iter()
            .filter(|entry| entry["method"] == method)
            .count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.cleanup {
            return;
        }
        source::writable(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
