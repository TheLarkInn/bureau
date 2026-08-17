//! Shared fixtures for the `bureau watch` tests: a temp home with
//! hand-written run logs, a real state.db, and an adopted-config cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bureau::config::{ActivatedConfig, Config};
use bureau::contract::StepOutcome;
use bureau::runlog;
use bureau::state::Store;
use bureau::watch::Roots;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temp directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    /// Creates a unique temp directory.
    ///
    /// # Panics
    /// Panics when the directory cannot be created.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-watch-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// The watch roots inside this home.
    #[must_use]
    pub fn roots(&self) -> Roots {
        Roots {
            runs: self.0.join("runs"),
            state: self.0.join("state.db"),
            config_cache: self.0.join("config-cache"),
        }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One event line with a fixed clock, the way events.jsonl stores it.
#[must_use]
pub fn line(seq: u64, at_ms: u64, kind: &str, data: &serde_json::Value) -> String {
    serde_json::json!({"seq": seq, "at_ms": at_ms, "kind": kind, "data": data}).to_string()
}

/// Writes one run's events.jsonl.
///
/// # Panics
/// Panics when the fixture cannot be written.
pub fn write_log(runs: &Path, run_id: &str, lines: &[String]) {
    let dir = runs.join(run_id);
    std::fs::create_dir_all(&dir).expect("run dir");
    std::fs::write(dir.join("events.jsonl"), lines.join("\n") + "\n").expect("events");
}

/// A run that started and began step `work`; no state cache, so the
/// view-model must replay the log.
///
/// # Panics
/// Panics when the fixture cannot be written.
pub fn write_running_run(runs: &Path, run_id: &str, at_ms: u64) {
    let lines = [
        line(
            0,
            at_ms,
            "run_started",
            &runlog::run_started_for_item(run_id, "demo", "42"),
        ),
        line(
            1,
            at_ms + 100,
            "step_started",
            &runlog::step_started("work"),
        ),
    ];
    write_log(runs, run_id, &lines);
}

/// A run that settled; the derived cache is written the way the
/// engine's teardown writes it.
///
/// # Panics
/// Panics when the fixture cannot be written or replayed.
pub fn write_finished_run(runs: &Path, run_id: &str, at_ms: u64, cost_usd: f64) {
    let ok = StepOutcome::Success;
    let started = &runlog::run_started_for_item(run_id, "demo", "42");
    let step_start = &runlog::step_started("work");
    let step_done = &runlog::step_finished("work", ok);
    let finished = &runlog::run_finished_full(ok, "done", cost_usd, None, None);
    let lines = [
        line(0, at_ms, "run_started", started),
        line(1, at_ms + 100, "step_started", step_start),
        line(2, at_ms + 200, "step_finished", step_done),
        line(3, at_ms + 300, "run_finished", finished),
    ];
    write_log(runs, run_id, &lines);
    let dir = runs.join(run_id);
    let state = runlog::replay_state(&dir).expect("replay fixture");
    runlog::write_state_cache(&dir, &state).expect("cache fixture");
}

/// A run with six output events after its start, for detail tailing.
///
/// # Panics
/// Panics when the fixture cannot be written.
pub fn write_chatty_run(runs: &Path, run_id: &str, at_ms: u64) {
    let mut lines = vec![line(
        0,
        at_ms,
        "run_started",
        &runlog::run_started_for_item(run_id, "demo", "7"),
    )];
    for seq in 1..=6_u64 {
        let data = runlog::output(Some("work"), "combined", "chunk");
        lines.push(line(seq, at_ms + seq, "output", &data));
    }
    write_log(runs, run_id, &lines);
}

/// The adopted-config cache holding one assignment, `demo`.
///
/// # Panics
/// Panics when the fixture config is invalid or cannot be written.
pub fn write_active(home: &Path, commit: &str) {
    let config_dir = home.join("config-src");
    write(&config_dir, "repos.yaml", MINIMAL_REPO);
    write(&config_dir, "roles/worker.yaml", MINIMAL_ROLE);
    write(&config_dir, "assignments/demo.yaml", MINIMAL_ASSIGNMENT);
    write(
        &config_dir,
        "pipelines/fix-failing-test.yaml",
        MINIMAL_PIPELINE,
    );
    let config = Config::load(&config_dir).expect("fixture config loads");
    let active = ActivatedConfig {
        config,
        remote: "https://example.invalid/config".to_owned(),
        reference: "main".to_owned(),
        commit: commit.to_owned(),
        direct_agents: BTreeMap::new(),
    };
    let cache = home.join("config-cache");
    std::fs::create_dir_all(&cache).expect("config-cache");
    let bytes = serde_json::to_vec_pretty(&active).expect("serialize");
    std::fs::write(cache.join("active.json"), bytes).expect("active.json");
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

/// state.db: two recorded runs at $3 each and one live lease on `demo`.
///
/// # Panics
/// Panics when the store cannot be opened or written.
pub fn write_state_db(home: &Path) {
    let store = Store::open(&home.join("state.db")).expect("open");
    store.record_run("run-1", "demo", 3.0).expect("record one");
    store.record_run("run-2", "demo", 3.0).expect("record two");
    let claimed = store
        .try_claim("demo", "github", "42", Duration::from_secs(3600))
        .expect("claim");
    assert!(claimed);
}

const MINIMAL_REPO: &str = r"
repos:
  code:
    url: https://github.com/example/code
    forge: github
    access: push
    credential: gh-main
";

const MINIMAL_ROLE: &str = r"
name: worker
agent: agents/worker.md
adapter: fake
permissions: [repo:read, repo:write, pr:write]
min_trust: untrusted
";

const MINIMAL_PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const MINIMAL_ASSIGNMENT: &str = r#"
name: demo
work:
  forge: github
  source: "example/code"
  filter: "label:agent-eligible"
repos: [code]
pipeline: fix-failing-test
role: worker
verify: "make test"
branch_prefix: runner/
limits:
  max_concurrent: 2
  max_runs_per_hour: 4
  max_cost_per_day_usd: 25
"#;
