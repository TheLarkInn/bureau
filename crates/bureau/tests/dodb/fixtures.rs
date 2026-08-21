//! Shared fixtures for the section 13 definition-of-done tests: a local
//! git repo, config builders, and a temp dir. Self-contained so other
//! suites' fixture edits cannot break these proofs. Offline only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{
    Access, Assignment, Config, ForgeKind, Limits, Pipeline, Repo, StepDef, StepKind, WorkSource,
};
use bureau::contract::Trust;
use bureau::forge::Item;

/// The one assignment every fixture uses.
pub const ASSIGNMENT: &str = "fix-tests";

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    /// A fresh temp dir named for `tag`.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-dod-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs git for fixture setup, panicking on failure.
fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// A local repo with one committed file; the returned URL is its path,
/// which clone and push both accept offline.
pub fn make_repo(parent: &Path) -> String {
    let dir = parent.join("repo");
    std::fs::create_dir_all(&dir).expect("repo dir");
    git(&dir, &["init", "-b", "main"]);
    std::fs::write(dir.join("file.txt"), "start\n").expect("seed file");
    git(&dir, &["add", "-A"]);
    let commit = [
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@test",
        "commit",
        "-m",
        "init",
    ];
    git(&dir, &commit);
    dir.to_string_lossy().into_owned()
}

/// A work item with id-derived content.
pub fn item(id: &str) -> Item {
    Item {
        external_id: id.to_owned(),
        title: format!("Fix {id}"),
        body: format!("{id} is broken"),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

/// Budget limits that never gate a test unless overridden.
pub const fn generous() -> Limits {
    Limits {
        max_concurrent: Some(5),
        max_runs_per_hour: Some(10),
        max_runs_per_day: Some(20),
        max_open_prs: Some(5),
        max_cost_per_day_usd: Some(50.0),
        max_run_hours: None,
    }
}

/// A deterministic step running `run`, then the `done` terminal.
pub fn det_step(run: &str) -> StepDef {
    StepDef {
        name: "check".to_owned(),
        kind: StepKind::Deterministic,
        run: Some(run.to_owned()),
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: Some("done".to_owned()),
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// The registry's one repo entry.
pub fn repo(url: &str) -> Repo {
    Repo {
        url: url.to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

/// The fixture assignment: one repo, the `fix` pipeline.
pub fn assignment(limits: Limits) -> Assignment {
    Assignment {
        name: ASSIGNMENT.to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
            abort_label: "bureau:failed".to_owned(),
            escalate_label: "bureau:needs-human".to_owned(),
        },
        repos: vec!["main".to_owned()],
        pipeline: "fix".to_owned(),
        role: "worker".to_owned(),
        verify: "true".to_owned(),
        branch_prefix: "bureau/".to_owned(),
        limits,
    }
}

/// The fixture config: one repo, no roles, one one-step pipeline.
pub fn config(url: &str, run: &str, limits: Limits) -> Config {
    let pipeline = Pipeline {
        name: "fix".to_owned(),
        steps: vec![det_step(run)],
    };
    Config {
        repos: BTreeMap::from([("main".to_owned(), repo(url))]),
        roles: BTreeMap::new(),
        assignments: BTreeMap::from([(ASSIGNMENT.to_owned(), assignment(limits))]),
        pipelines: BTreeMap::from([("fix".to_owned(), pipeline)]),
    }
}
