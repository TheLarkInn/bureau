//! One-shot execution admits only committed configuration.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bureau-committed-run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".bureau")).expect("repo");
        write_config(&repo.join(".bureau"));
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["add", "-A"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@test",
                "commit",
                "-m",
                "config",
            ],
        );
        write_settings(&root, &repo);
        Self { root }
    }

    fn run(&self, pipeline: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bureau"))
            .args(["run", pipeline, "--item", "42"])
            .env("BUREAU_HOME", &self.root)
            .env_remove("BUREAU_TEST_MISSING")
            .output()
            .expect("bureau")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn unknown_pipeline_is_rejected_from_committed_config() {
    let output = Fixture::new().run("ghost");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        (output.status.code(), stderr.contains("ghost")),
        (Some(2), true)
    );
}

#[test]
fn declared_missing_credential_fails_before_state_creation() {
    let fixture = Fixture::new();
    let output = fixture.run("fix-failing-test");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code() == Some(2)
            && stderr.contains("gh-main")
            && !fixture.root.join("state.db").exists(),
        "{stderr}"
    );
}

fn write_settings(root: &Path, repo: &Path) {
    let value = format!(
        "config:\n  kind: single_repository\n  remote: '{}'\n  reference: main\ncredentials:\n  gh-main:\n    source: environment\n    variable: BUREAU_TEST_MISSING\n",
        repo.display()
    );
    std::fs::write(root.join("settings.yaml"), value).expect("settings");
}

fn write_config(root: &Path) {
    write(
        root,
        "repos.yaml",
        "repos:\n  code:\n    url: https://github.com/example/code\n    forge: github\n    access: push\n    credential: gh-main\n",
    );
    write(
        root,
        "roles/worker.yaml",
        "name: worker\nagent: agents/worker.md\nadapter: fake\npermissions: [repo:read]\nmin_trust: untrusted\n",
    );
    write(
        root,
        "assignments/demo.yaml",
        "name: demo\nwork:\n  forge: github\n  source: example/code\n  filter: is:issue\n  abort_label: bureau:failed\n  escalate_label: bureau:needs-human\nrepos: [code]\npipeline: fix-failing-test\nrole: worker\nverify: \"true\"\nbranch_prefix: bureau/\n",
    );
    write(
        root,
        "pipelines/fix-failing-test.yaml",
        "name: fix-failing-test\nsteps:\n  - name: check\n    type: deterministic\n    run: \"true\"\n    next: done\n",
    );
}

fn write(root: &Path, relative: &str, value: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, value).expect("write");
}

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .status()
        .expect("git");
    assert!(status.success(), "git {arguments:?}");
}
