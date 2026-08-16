//! Terminal projection distinguishes absent logs from corrupt durable state.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::engine::Engine;
use bureau::state::{Store, project_run};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-state-projection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("test dir");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn corrupt_log_is_an_error_not_an_absent_terminal() {
    let root = TestDir::new();
    let store = Store::open_in_memory().expect("store");
    let missing = project_run(&store, &root.0, "missing").expect("missing log");
    let run = bureau::runlog::run_dir(&root.0, "corrupt");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join(bureau::runlog::EVENTS_FILE), "not json\n").expect("events");
    let corrupt = project_run(&store, &root.0, "corrupt").is_err();
    assert_eq!((missing, corrupt), (false, true));
}

#[test]
fn empty_prestart_log_does_not_poison_recovery() {
    let root = TestDir::new();
    let run = bureau::runlog::run_dir(&root.0, "prestart");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join(bureau::runlog::EVENTS_FILE), []).expect("empty events");
    let engine = Engine::new(root.0.clone(), root.0.join("cache"));
    let recovered = (engine.unfinished(), engine.finished());
    assert!(matches!(recovered, (Ok(left), Ok(right)) if left.is_empty() && right.is_empty()));
}
