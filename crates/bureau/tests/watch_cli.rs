//! `bureau watch` at the binary boundary: piped (not a terminal), it
//! prints one plain-text snapshot and exits 0, even when the home
//! directory does not exist.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-watch-cli-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The Command's piped stdout is never a terminal, so the binary takes
/// its snapshot path.
fn bureau_watch(home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .arg("watch")
        .env("BUREAU_HOME", home)
        .output()
        .expect("run bureau watch")
}

/// One finished run's log, written by hand.
fn write_run(home: &Path) {
    let dir = home.join("runs").join("demo-1000-aa");
    std::fs::create_dir_all(&dir).expect("run dir");
    let started = serde_json::json!({"seq": 0, "at_ms": 1000, "kind": "run_started",
        "data": {"run_id": "demo-1000-aa", "assignment": "demo", "item": "42"}});
    let finished = serde_json::json!({"seq": 1, "at_ms": 1100, "kind": "run_finished",
        "data": {"outcome": "success", "message": "done", "cost_usd": 1.25}});
    std::fs::write(dir.join("events.jsonl"), format!("{started}\n{finished}\n")).expect("events");
}

#[test]
fn piped_watch_prints_one_snapshot_and_exits_zero() {
    let dir = TestDir::new("cli");
    write_run(dir.path());
    let output = bureau_watch(dir.path());
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let checks = (
        output.status.success(),
        text.contains("bureau watch · config none"),
        text.contains("demo-1000-aa")
            && text.contains("finished(success)")
            && text.contains("$1.25"),
    );
    assert_eq!(checks, (true, true, true), "{text}");
}

#[test]
fn piped_watch_tolerates_a_missing_home() {
    let dir = TestDir::new("cli-missing");
    let output = bureau_watch(&dir.path().join("does-not-exist"));
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let checks = (
        output.status.success(),
        text.contains("runs:"),
        text.contains("none"),
    );
    assert_eq!(checks, (true, true, true), "{text}");
}
