//! Committed Git configuration snapshots with last-known-good retention.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::git::{CheckoutCache, Credential};

use super::{Config, SourceError};

fn validate_subdir(path: &Path) -> Result<(), SourceError> {
    let unsafe_component = path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir));
    if unsafe_component {
        Err(SourceError::UnsafeSubdir)
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

async fn create_dirs(path: &Path) -> Result<(), SourceError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(path))
        .await
        .map_err(std::io::Error::other)??;
    Ok(())
}

/// On-disk locations under the cache root: the git mirror cache, the
/// materialized snapshots, and the last-known-good pointer.
struct Cache {
    mirrors: CheckoutCache,
    snapshots: PathBuf,
    active: PathBuf,
}

impl Cache {
    fn new(root: &Path) -> Self {
        Self {
            mirrors: CheckoutCache::new(root.join("mirrors")),
            snapshots: root.join("snapshots"),
            active: root.join("active.json"),
        }
    }
}

/// One validated committed config revision.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
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
    cache: Cache,
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
            cache: Cache::new(cache_root),
            credential,
        }
    }

    /// Fetches, resolves, materializes, and validates one exact commit.
    ///
    /// # Errors
    /// Rejects unsafe subdirectories and propagates Git/config failures.
    pub async fn load(&self) -> Result<Activated, SourceError> {
        validate_subdir(&self.subdir)?;
        create_dirs(&self.cache.snapshots).await?;
        let directory = self.snapshot_directory()?;
        let (worktree, commit) = self
            .cache
            .mirrors
            .snapshot(
                &self.remote,
                self.credential.as_ref(),
                &self.reference,
                &directory,
            )
            .await?;
        let (config, direct_agents) = Self::loaded(worktree.path(), &self.subdir)?;
        let active = Activated {
            config,
            remote: self.remote.clone(),
            reference: self.reference.clone(),
            commit,
            direct_agents,
        };
        Self::persist_active(&self.cache.active, &active)?;
        Ok(active)
    }

    fn persist_active(path: &Path, active: &Activated) -> Result<(), SourceError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".active-{}.json.tmp",
            crate::identity::random_hex()?
        ));
        let bytes = serde_json::to_vec_pretty(active).map_err(std::io::Error::other)?;
        let result = Self::commit_active(&temporary, path, parent, &bytes);
        if result.is_err() {
            let _removed = std::fs::remove_file(&temporary);
        }
        result.map_err(SourceError::from)
    }

    fn write_active(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
        use std::io::Write as _;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn commit_active(
        temporary: &Path,
        path: &Path,
        parent: &Path,
        bytes: &[u8],
    ) -> Result<(), std::io::Error> {
        Self::write_active(temporary, bytes)?;
        std::fs::rename(temporary, path)?;
        std::fs::File::open(parent)?.sync_all()
    }

    fn snapshot_directory(&self) -> Result<PathBuf, SourceError> {
        Ok(self
            .cache
            .snapshots
            .join(format!("config-{}", crate::identity::random_hex()?)))
    }

    fn loaded(
        snapshot: &Path,
        subdir: &Path,
    ) -> Result<(Config, BTreeMap<String, Vec<u8>>), SourceError> {
        let config_dir = snapshot.join(subdir);
        super::source_tree::validate(snapshot, &config_dir)?;
        let config = Config::load(&config_dir)
            .map_err(|errors| SourceError::Validation(messages(&errors)))?;
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
    pub async fn refresh(&mut self) -> Result<Refresh, SourceError> {
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
