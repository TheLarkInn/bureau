use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use bureau::contract::StepOutcome;
use bureau::git::CheckoutCache;
use bureau::home::{Directory, Home, Layout};
use bureau::runlog::{self, EventKind, RunLog};
use bureau::setup::{
    ConfigSource, CredentialSource, MigrationSettings, PluginSettings, Settings, save_settings,
};
use bureau::state::Store;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub const REPO_URL: &str = "https://example.invalid/work.git";
pub const SECRET_TEXT: &str = "credential-value-must-not-appear";

pub struct Fixture {
    root: PathBuf,
    home: Home,
    pub bin: PathBuf,
}

impl Fixture {
    pub fn new(label: &str) -> Self {
        let next = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from("target/local-effects-tests")
            .join(format!("{label}-{}-{next}", std::process::id()));
        let _removed = fs::remove_dir_all(&root);
        let home = Home::new(root.join("home"));
        for directory in Directory::ALL {
            create_dir(home.layout().directory(directory));
        }
        let bin = root.join("bin");
        create_dir(&bin);
        Self { root, home, bin }
    }

    /// Fixed fixture layout.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        self.home.layout()
    }

    /// Writes settings, a committed config snapshot, and its checkout mirror.
    ///
    /// # Panics
    /// Panics when fixture files cannot be written.
    pub fn configure(&self, adapter: Option<&str>, credential: CredentialSource) {
        let settings = Settings {
            config: ConfigSource::SeparateRepository {
                remote: "https://example.invalid/config.git".to_owned(),
                reference: "main".to_owned(),
            },
            credentials: BTreeMap::from([("work".to_owned(), credential)]),
            plugin: PluginSettings::default(),
            migration: MigrationSettings::default(),
        };
        save_settings(self.layout().settings(), &settings).expect("save settings");
        self.write_config(adapter);
        self.create_repo_mirror();
    }

    /// Writes a deliberately unreadable credential value file.
    ///
    /// # Panics
    /// Panics when the fixture file cannot be written or restricted.
    #[must_use]
    pub fn credential_file(&self) -> PathBuf {
        let path = self.root.join("credential");
        fs::write(&path, SECRET_TEXT).expect("write credential");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("restrict credential");
        path
    }

    /// Creates one executable in the fixture search path.
    ///
    /// # Panics
    /// Panics when the executable cannot be written.
    pub fn executable(&self, name: &str) {
        let path = self.bin.join(name);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("make executable");
    }

    /// Opens the fixture state database.
    ///
    /// # Panics
    /// Panics when `SQLite` cannot open the database.
    #[must_use]
    pub fn state(&self) -> Store {
        Store::open(self.layout().state_db()).expect("state store")
    }

    /// Writes one run log and optionally its terminal event and derived cache.
    ///
    /// # Panics
    /// Panics when run files cannot be written or replayed.
    #[must_use]
    pub fn run(&self, run_id: &str, finished: bool, cache: bool) -> PathBuf {
        let mut log = RunLog::create(self.layout().runs(), run_id, &[]).expect("create run");
        log.append(
            EventKind::RunStarted,
            runlog::run_started(run_id, "assignment"),
        )
        .expect("start run");
        if finished {
            log.append(
                EventKind::RunFinished,
                runlog::run_finished(StepOutcome::Success),
            )
            .expect("finish run");
        }
        let directory = log.dir().to_path_buf();
        log.close().expect("close run");
        if cache {
            let state = runlog::replay_state(&directory).expect("replay run");
            runlog::write_state_cache(&directory, &state).expect("cache state");
        }
        directory
    }

    /// Writes a worktree with no durable event history.
    ///
    /// # Panics
    /// Panics when fixture files cannot be written.
    #[must_use]
    pub fn orphan_worktree(&self, run_id: &str) -> PathBuf {
        let path = self.layout().runs().join(run_id).join("wt");
        create_dir(&path);
        fs::write(path.join("change"), "orphan").expect("write orphan");
        path
    }

    /// Creates a real detached Git worktree with no durable run history.
    ///
    /// # Panics
    /// Panics when local Git setup fails.
    #[must_use]
    pub fn registered_worktree(&self, run_id: &str) -> (PathBuf, PathBuf) {
        let seed = self.root.join(format!("seed-{run_id}"));
        create_dir(&seed);
        git(&seed, &["init", "--quiet"]);
        fs::write(seed.join("tracked"), "content").expect("write tracked file");
        git(&seed, &["add", "tracked"]);
        git_commit(&seed);
        let mirror = self.layout().checkout_cache().join(format!("{run_id}.git"));
        git_clone(&self.root, &seed, &mirror);
        let worktree = self.layout().runs().join(run_id).join("wt");
        git_worktree(&mirror, &worktree);
        (mirror, worktree)
    }

    fn write_config(&self, adapter: Option<&str>) {
        let snapshot = self.layout().config_cache().join("snapshots/config-a");
        for directory in ["roles", "assignments", "pipelines"] {
            create_dir(&snapshot.join(directory));
        }
        fs::write(snapshot.join("repos.yaml"), repos_yaml()).expect("write repos");
        if let Some(adapter) = adapter {
            fs::write(snapshot.join("roles/reviewer.yaml"), role_yaml(adapter))
                .expect("write role");
        }
        self.write_active(&snapshot);
    }

    fn write_active(&self, snapshot: &Path) {
        let active = bureau::config::ActivatedConfig {
            config: bureau::config::Config::load(snapshot).expect("load config"),
            remote: "https://example.invalid/config.git".to_owned(),
            reference: "main".to_owned(),
            commit: "config-a".to_owned(),
            direct_agents: BTreeMap::new(),
        };
        let bytes = serde_json::to_vec_pretty(&active).expect("serialize active config");
        fs::write(self.layout().config_cache().join("active.json"), bytes)
            .expect("write active config");
    }

    fn create_repo_mirror(&self) {
        let cache = CheckoutCache::new(self.layout().checkout_cache().to_path_buf());
        create_dir(&cache.mirror_dir(REPO_URL));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::set_permissions(self.credential_path(), fs::Permissions::from_mode(0o600)).ok();
        fs::remove_dir_all(&self.root).expect("remove fixture");
    }
}

impl Fixture {
    fn credential_path(&self) -> PathBuf {
        self.root.join("credential")
    }
}

fn create_dir(path: &Path) {
    fs::create_dir_all(path).expect("create directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("set directory mode");
}

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("run git");
    assert!(status.success(), "git command failed");
}

fn git_commit(seed: &Path) {
    git(
        seed,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    );
}

fn git_clone(root: &Path, seed: &Path, mirror: &Path) {
    let seed = std::path::absolute(seed).expect("absolute seed");
    let mirror = std::path::absolute(mirror).expect("absolute mirror");
    let args = ["clone", "--quiet", "--bare"];
    let status = Command::new("git")
        .args(args)
        .arg(seed)
        .arg(mirror)
        .current_dir(root)
        .status()
        .expect("clone mirror");
    assert!(status.success(), "git clone failed");
}

fn git_worktree(mirror: &Path, worktree: &Path) {
    let worktree = std::path::absolute(worktree).expect("absolute worktree");
    let status = Command::new("git")
        .args(["worktree", "add", "--quiet", "--detach"])
        .arg(worktree)
        .arg("HEAD")
        .current_dir(mirror)
        .status()
        .expect("create worktree");
    assert!(status.success(), "git worktree failed");
}

fn repos_yaml() -> String {
    format!(
        "repos:\n  work:\n    url: {REPO_URL}\n    forge: github\n    access: read\n    credential: work\n"
    )
}

fn role_yaml(adapter: &str) -> String {
    format!(
        "name: reviewer\nagent: /bureau:reviewer\nadapter: {adapter}\npermissions: [model:invoke]\nmin_trust: trusted\n"
    )
}
