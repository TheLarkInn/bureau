use std::path::{Path, PathBuf};

use super::config;
use crate::cli::reconcile::{ForgeArg, ResolvedArgs};

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(root)
        .output()
        .expect("fixture git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(root: &Path) -> PathBuf {
    let source = root.join("source");
    std::fs::create_dir_all(&source).expect("source repo");
    git(&source, &["init", "--quiet", "-b", "main"]);
    config::write_config(&source, &source);
    git(&source, &["add", ".bureau"]);
    git(
        &source,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "--quiet",
            "-m",
            "offline daemon config",
        ],
    );
    source
}

pub(super) struct Files {
    pub(super) root: PathBuf,
    source: PathBuf,
}

impl Files {
    pub(super) fn new() -> Self {
        let id = bureau::engine::new_run_id("daemon-credentials").expect("fixture identity");
        let root = std::env::temp_dir().join(id);
        std::fs::create_dir(&root).expect("owned daemon fixture");
        std::fs::write(root.join("repo-credential"), "synthetic-repo-secret").expect("fake source");
        let source = repository(&root);
        Self { root, source }
    }

    pub(super) fn args(&self) -> ResolvedArgs {
        ResolvedArgs {
            maintenance_guarded: true,
            maintenance_root: self.root.clone(),
            config_remote: self.source.to_string_lossy().into_owned(),
            config_ref: "main".into(),
            config_subdir: ".bureau".into(),
            config_credential: None,
            config_forge: ForgeArg::Github,
            config_cache: self.root.join("config-cache"),
            runs: self.root.join("runs"),
            state: self.root.join("state.db"),
            cache: self.root.join("checkouts"),
            settings: Some(config::settings(&self.root)),
            interval: "1m".into(),
            now: true,
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        super::native_source::writable(&self.root);
        std::fs::remove_dir_all(&self.root).expect("remove owned daemon fixture");
    }
}
