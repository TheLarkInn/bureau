//! Binary-level local lifecycle command behavior.

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("dir");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bureau(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .env("BUREAU_HOME", home)
        .output()
        .expect("bureau")
}

fn bureau_input(home: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .env("BUREAU_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bureau");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write input");
    child.wait_with_output().expect("output")
}

fn settings(remote: &str) -> String {
    settings_with_plugin(remote, false)
}

fn settings_with_plugin(remote: &str, install: bool) -> String {
    format!(
        "config:\n  kind: single_repository\n  remote: {remote}\n  reference: main\ncredentials: {{}}\nplugin:\n  install_user_global: {install}\nmigration:\n  source: null\n"
    )
}

#[test]
fn setup_atomically_replaces_existing_settings() {
    let home = TestDir::new();
    let from = home.0.join("new.yaml");
    std::fs::write(home.0.join("settings.yaml"), settings("old")).expect("old");
    std::fs::write(&from, settings("new")).expect("new");
    let output = bureau(&home.0, &["setup", "--from", &from.to_string_lossy()]);
    let saved = std::fs::read_to_string(home.0.join("settings.yaml")).expect("saved");
    assert!(output.status.success() && saved.contains("remote: new"));
}

#[test]
fn setup_does_not_apply_settings_after_requested_plugin_failure() {
    let home = TestDir::new();
    let from = home.0.join("new.yaml");
    std::fs::write(home.0.join("settings.yaml"), settings("old")).expect("old");
    std::fs::write(&from, settings_with_plugin("new", true)).expect("new");
    let bin = home.0.join("bin");
    std::fs::create_dir(&bin).expect("bin");
    let copilot = bin.join("copilot");
    std::fs::write(&copilot, "#!/bin/sh\necho installer-failed >&2\nexit 7\n").expect("fake");
    std::fs::set_permissions(&copilot, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let output = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(["setup", "--from", &from.to_string_lossy()])
        .env("BUREAU_HOME", &home.0)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .expect("bureau");
    let saved = std::fs::read_to_string(home.0.join("settings.yaml")).expect("saved");
    assert!(!output.status.success() && saved.contains("remote: old"));
}

#[test]
fn doctor_json_is_read_only_and_structured() {
    let home = TestDir::new();
    let output = bureau(&home.0, &["doctor", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["diagnostics"].as_array().map(Vec::len), Some(8));
}

#[test]
fn repair_requires_confirmation_then_creates_layout() {
    let home = TestDir::new();
    let preview = bureau(&home.0, &["repair"]);
    let applied = bureau_input(&home.0, &["repair"], b"yes\n");
    let complete = ["credentials", "runs", "checkout-cache", "config-cache"]
        .iter()
        .all(|name| home.0.join(name).is_dir());
    assert_eq!(
        (preview.status.code(), applied.status.success(), complete),
        (Some(2), true, true)
    );
}

#[test]
fn repair_restores_a_crashed_running_log_without_a_live_lease() {
    let home = TestDir::new();
    let run = crashed_run(&home.0);
    let worktree = run.join("wt");
    std::fs::create_dir_all(&worktree).expect("worktree");
    let activation =
        bureau_plugin::activate_direct("reviewer.md", b"agent", &worktree, &run).expect("activate");
    std::mem::forget(activation);
    let output = bureau_input(&home.0, &["repair"], b"yes\n");
    let restored = !worktree.join(".github/agents/reviewer.agent.md").exists()
        && !run.join("activations").exists();
    assert!(output.status.success() && restored);
}

fn crashed_run(home: &Path) -> PathBuf {
    let runs = home.join("runs");
    let mut log = bureau::runlog::RunLog::create(&runs, "crashed", &[]).expect("run log");
    log.append(
        bureau::runlog::EventKind::RunStarted,
        bureau::runlog::run_started("crashed", "assignment"),
    )
    .expect("start run");
    let run = log.dir().to_path_buf();
    log.close().expect("close run");
    run
}

#[test]
fn init_uses_the_production_settings_guard() {
    let home = TestDir::new();
    let request = home.0.join("init.yaml");
    let yaml = format!(
        "settings:\n{}repositories:\n  code:\n    url: https://github.com/example/code\n    forge: github\n    access: pr\n    credential: work\nassignment:\n  name: first\n  work:\n    forge: github\n    source: example/code\n    filter: is:issue\n    abort_label: bureau:failed\n    escalate_label: bureau:needs-human\n  primary_repo: code\n  verify: cargo test\n  branch_prefix: bureau/\n  adapter: copilot\nfirst_pipeline:\n  kind: fixed\n",
        indent(&settings("repo"), 2)
    );
    std::fs::write(home.0.join("settings.yaml"), settings("existing")).expect("settings");
    std::fs::write(&request, yaml).expect("request");
    let output = bureau(&home.0, &["init", "--from", &request.to_string_lossy()]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success() && stderr.contains("bureau setup"));
}

fn indent(value: &str, spaces: usize) -> String {
    use std::fmt::Write as _;

    let prefix = " ".repeat(spaces);
    value.lines().fold(String::new(), |mut output, line| {
        let _ = writeln!(output, "{prefix}{line}");
        output
    })
}
