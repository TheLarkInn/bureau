//! The bare-mirror checkout cache: one directory per remote URL.

use std::path::{Path, PathBuf};

use super::{Credential, Error, git};

/// Bare-mirror cache, one directory per remote URL.
#[derive(Debug, Clone)]
pub struct CheckoutCache {
    root: PathBuf,
}

impl CheckoutCache {
    /// A cache rooted at `root` (created lazily).
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The cache directory for a URL: a hash of the URL, so remotes with
    /// awkward names still map to one stable path.
    #[must_use]
    pub fn mirror_dir(&self, url: &str) -> PathBuf {
        use std::hash::{Hash, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        self.root.join(format!("{:016x}", hasher.finish()))
    }

    /// First use: `git clone --mirror` under the cache root.
    async fn clone_mirror(
        &self,
        url: &str,
        dir: &Path,
        credential: Option<&Credential>,
        clock: fn() -> u64,
    ) -> Result<(), Error> {
        tokio::fs::create_dir_all(&self.root).await?;
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut secrets = Vec::new();
        git(
            &["clone", "--mirror", url, &name],
            &self.root,
            credential,
            &mut secrets,
            clock,
        )
        .await?;
        Ok(())
    }

    /// Ensures an up-to-date bare mirror of `url` exists and returns
    /// its path: `git clone --mirror` on first use, `git fetch --prune` after.
    ///
    /// # Errors
    /// Propagates git and filesystem failures.
    pub async fn mirror(
        &self,
        url: &str,
        credential: Option<&Credential>,
        clock: fn() -> u64,
    ) -> Result<PathBuf, Error> {
        let dir = self.mirror_dir(url);
        if dir.exists() {
            let mut secrets = Vec::new();
            git(&["fetch", "--prune"], &dir, credential, &mut secrets, clock).await?;
        } else {
            self.clone_mirror(url, &dir, credential, clock).await?;
        }
        Ok(dir)
    }
}
