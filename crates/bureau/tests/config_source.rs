//! Committed config refresh and last-known-good behavior.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{ConfigManager, GitSource, SourceError};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Repo {
    home: PathBuf,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "bureau-config-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&home);
        let root = home.join("repo");
        std::fs::create_dir_all(&root).expect("repo dir");
        git(&root, &["init", "-b", "main"]);
        write_valid(&root);
        commit(&root, "valid");
        Self { home, root }
    }

    fn source(&self, subdir: &str) -> GitSource {
        GitSource::new(
            self.root.to_string_lossy().into_owned(),
            "main".to_owned(),
            subdir.into(),
            &self.home.join("cache"),
            None,
        )
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[tokio::test]
async fn committed_subdirectory_loads_at_exact_commit() {
    let repo = Repo::new();
    let mut manager = ConfigManager::new(repo.source(".bureau"));
    let refresh = manager.refresh().await.expect("refresh");
    assert_eq!(
        (
            refresh.active.config.assignments.len(),
            refresh.active.reference.as_str(),
            refresh.active.commit.len(),
            refresh.warning,
        ),
        (1, "main", 40, None)
    );
}

#[tokio::test]
async fn crash_leftover_snapshot_name_does_not_block_refresh() {
    let repo = Repo::new();
    let stale = repo
        .home
        .join("cache/snapshots")
        .join(format!("config-{}-0", std::process::id()));
    std::fs::create_dir_all(stale).expect("stale snapshot");
    let loaded = repo.source(".bureau").load().await.expect("load");
    assert_eq!(loaded.config.assignments.len(), 1);
}

#[tokio::test]
async fn committed_direct_agent_bytes_are_pinned() {
    let repo = Repo::new();
    let role = "name: worker\nagent: agents/worker.md\nadapter: copilot\npermissions: [repo:read]\nmin_trust: untrusted\n";
    std::fs::write(repo.root.join(".bureau/roles/worker.yaml"), role).expect("role");
    std::fs::create_dir_all(repo.root.join(".bureau/agents")).expect("agents");
    std::fs::write(repo.root.join(".bureau/agents/worker.md"), b"pinned agent").expect("agent");
    commit(&repo.root, "direct agent");
    let loaded = repo.source(".bureau").load().await.expect("load");
    assert_eq!(loaded.direct_agents["worker"], b"pinned agent");
}

#[tokio::test]
async fn invalid_new_commit_retains_last_known_good() {
    let repo = Repo::new();
    let mut manager = ConfigManager::new(repo.source(".bureau"));
    let first = manager.refresh().await.expect("first");
    std::fs::remove_file(repo.root.join(".bureau/repos.yaml")).expect("remove repos");
    commit(&repo.root, "invalid");
    let second = manager.refresh().await.expect("last known good");
    let first_commit = first.active.commit;
    assert_eq!(
        (
            second.active.commit,
            first_commit.clone(),
            second.warning.is_some(),
        ),
        (first_commit.clone(), first_commit, true)
    );
}

#[tokio::test]
async fn first_invalid_revision_is_fatal() {
    let repo = Repo::new();
    std::fs::remove_file(repo.root.join(".bureau/repos.yaml")).expect("remove repos");
    commit(&repo.root, "invalid");
    let mut manager = ConfigManager::new(repo.source(".bureau"));
    let error = manager.refresh().await.err().expect("first load fails");
    assert!(error.to_string().contains("missing repos.yaml"));
}

#[tokio::test]
async fn source_subdirectory_cannot_escape_snapshot() {
    let repo = Repo::new();
    let error = repo
        .source("../outside")
        .load()
        .await
        .err()
        .expect("unsafe subdir fails");
    assert!(matches!(error, SourceError::UnsafeSubdir));
}

#[cfg(unix)]
#[tokio::test]
async fn committed_config_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let repo = Repo::new();
    let outside = repo.home.join("outside.yaml");
    std::fs::write(&outside, "repos: {}\n").expect("outside");
    let repos = repo.root.join(".bureau/repos.yaml");
    std::fs::remove_file(&repos).expect("remove repos");
    symlink(&outside, &repos).expect("symlink");
    commit(&repo.root, "symlink");
    let error = repo.source(".bureau").load().await.err().expect("unsafe");
    assert!(matches!(error, SourceError::UnsafeConfig(_)));
}

#[cfg(unix)]
#[tokio::test]
async fn dangling_config_directory_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let repo = Repo::new();
    let roles = repo.root.join(".bureau/roles");
    std::fs::remove_dir_all(&roles).expect("remove roles");
    symlink(repo.home.join("missing"), &roles).expect("symlink");
    commit(&repo.root, "dangling symlink");
    let error = repo.source(".bureau").load().await.err().expect("unsafe");
    assert!(matches!(error, SourceError::UnsafeConfig(_)));
}

fn write_valid(root: &Path) {
    let config = root.join(".bureau");
    std::fs::create_dir_all(config.join("roles")).expect("roles");
    std::fs::create_dir_all(config.join("assignments")).expect("assignments");
    std::fs::create_dir_all(config.join("pipelines")).expect("pipelines");
    std::fs::write(
        config.join("repos.yaml"),
        "repos:\n  main:\n    url: https://github.com/acme/main\n    forge: github\n    access: push\n    credential: github-main\n",
    )
    .expect("repos");
    std::fs::write(
        config.join("roles/worker.yaml"),
        "name: worker\nagent: /bureau:implementer\nadapter: fake\npermissions: [repo:read]\nmin_trust: untrusted\n",
    )
    .expect("role");
    std::fs::write(
        config.join("assignments/work.yaml"),
        "name: work\nwork:\n  forge: github\n  source: acme/main\n  filter: is:issue\n  abort_label: bureau:failed\n  escalate_label: bureau:needs-human\nrepos: [main]\npipeline: work\nrole: worker\nverify: \"true\"\nbranch_prefix: bureau/\n",
    )
    .expect("assignment");
    std::fs::write(
        config.join("pipelines/work.yaml"),
        "name: work\nsteps:\n  - name: check\n    type: deterministic\n    run: \"true\"\n    next: done\n",
    )
    .expect("pipeline");
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "commit",
            "-m",
            message,
        ],
    );
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}
