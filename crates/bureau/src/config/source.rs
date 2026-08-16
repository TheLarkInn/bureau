//! Committed Git configuration snapshots with last-known-good retention.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::git::{CheckoutCache, Credential};

use super::Config;

/// One validated committed config revision.
#[derive(Clone)]
pub struct Activated {
    /// Loaded configuration.
    pub config: Config,
    /// Git remote URL.
    pub remote: String,
    /// Configured branch/ref.
    pub reference: String,
    /// Exact activated commit.
    pub commit: String,
    /// Pinned direct-agent bytes keyed by role name.
    pub direct_agents: BTreeMap<String, Vec<u8>>,
}

/// Refresh result, optionally retaining an older revision after a failure.
pub struct Refresh {
    /// Revision new claims must use.
    pub active: Activated,
    /// Refresh/validation problem that caused last-known-good retention.
    pub warning: Option<String>,
}

/// A committed config repository and its disposable local cache.
pub struct GitSource {
    remote: String,
    reference: String,
    subdir: PathBuf,
    cache: CheckoutCache,
    snapshot_root: PathBuf,
    credential: Option<Credential>,
}

impl GitSource {
    /// Defines a committed source. `subdir` is `.` or `.bureau`.
    #[must_use]
    pub fn new(
        remote: String,
        reference: String,
        subdir: PathBuf,
        cache_root: &Path,
        credential: Option<Credential>,
    ) -> Self {
        Self {
            remote,
            reference,
            subdir,
            cache: CheckoutCache::new(cache_root.join("mirrors")),
            snapshot_root: cache_root.join("snapshots"),
            credential,
        }
    }

    /// Fetches, resolves, materializes, and validates one exact commit.
    ///
    /// # Errors
    /// Rejects unsafe subdirectories and propagates Git/config failures.
    pub async fn load(&self) -> Result<Activated, Error> {
        validate_subdir(&self.subdir)?;
        std::fs::create_dir_all(&self.snapshot_root)?;
        let directory = self.snapshot_directory()?;
        let (worktree, commit) = self
            .cache
            .snapshot(
                &self.remote,
                self.credential.as_ref(),
                &self.reference,
                &directory,
            )
            .await?;
        let (config, direct_agents) = Self::loaded(worktree.path(), &self.subdir)?;
        Ok(Activated {
            config,
            remote: self.remote.clone(),
            reference: self.reference.clone(),
            commit,
            direct_agents,
        })
    }

    fn snapshot_directory(&self) -> Result<PathBuf, Error> {
        Ok(self
            .snapshot_root
            .join(format!("config-{}", crate::identity::random_hex()?)))
    }

    fn loaded(
        snapshot: &Path,
        subdir: &Path,
    ) -> Result<(Config, BTreeMap<String, Vec<u8>>), Error> {
        let config_dir = snapshot.join(subdir);
        super::source_tree::validate(snapshot, &config_dir)?;
        let config =
            Config::load(&config_dir).map_err(|errors| Error::Validation(messages(&errors)))?;
        let agents = super::source_tree::load_agent_files(snapshot, &config_dir, &config.roles)?;
        Ok((config, agents))
    }
}

/// Retains the last valid committed revision across refresh failures.
pub struct Manager {
    source: GitSource,
    last: Option<Activated>,
}

impl Manager {
    /// Starts without a valid revision; the first failed refresh is fatal.
    #[must_use]
    pub const fn new(source: GitSource) -> Self {
        Self { source, last: None }
    }

    /// Activates a valid commit or returns last-known-good with a warning.
    ///
    /// # Errors
    /// Returns the source error when no valid revision has ever loaded.
    pub async fn refresh(&mut self) -> Result<Refresh, Error> {
        match self.source.load().await {
            Ok(active) => {
                self.last = Some(active.clone());
                Ok(Refresh {
                    active,
                    warning: None,
                })
            }
            Err(error) => match self.last.clone() {
                Some(active) => Ok(Refresh {
                    active,
                    warning: Some(error.to_string()),
                }),
                None => Err(error),
            },
        }
    }
}

/// Committed-source failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Git cache/snapshot failure.
    #[error(transparent)]
    Git(#[from] crate::git::Error),
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Loaded config failed validation.
    #[error("config revision is invalid:\n{0}")]
    Validation(String),
    /// Subdirectory escaped the config snapshot.
    #[error("config subdirectory must be relative and must not contain `..`")]
    UnsafeSubdir,
    /// A committed config path is a symlink, special file, or escape.
    #[error("committed config path is unsafe: {0}")]
    UnsafeConfig(String),
}

fn validate_subdir(path: &Path) -> Result<(), Error> {
    let unsafe_component = path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir));
    if unsafe_component {
        Err(Error::UnsafeSubdir)
    } else {
        Ok(())
    }
}

fn messages(errors: &[crate::ConfigError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
