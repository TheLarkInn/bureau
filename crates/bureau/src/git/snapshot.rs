//! Detached worktrees pinned to exact commits.

use std::path::Path;

use super::{CheckoutCache, Credential, Error, Worktree, git};

impl CheckoutCache {
    /// Creates a detached worktree at an exact commit resolved from `reference`.
    ///
    /// # Errors
    /// Propagates mirror, ref-resolution, worktree, and reset failures.
    pub async fn snapshot(
        &self,
        url: &str,
        credential: Option<&Credential>,
        reference: &str,
        directory: &Path,
    ) -> Result<(Worktree, String), Error> {
        let (mirror, commit) = resolve(self, url, credential, reference).await?;
        let worktree = checkout(&mirror, &commit, directory).await?;
        Ok((worktree, commit))
    }
}

impl Worktree {
    /// Resets this worktree to an exact commit.
    ///
    /// # Errors
    /// Propagates Git failures.
    pub async fn reset(&self, commit: &str) -> Result<(), Error> {
        let mut secrets = Vec::new();
        git(&["reset", "--hard", commit], &self.dir, None, &mut secrets).await?;
        Ok(())
    }
}

async fn resolve(
    cache: &CheckoutCache,
    url: &str,
    credential: Option<&Credential>,
    reference: &str,
) -> Result<(std::path::PathBuf, String), Error> {
    let mirror = cache.mirror(url, credential).await?;
    let commit = resolve_commit(&mirror, reference).await?;
    Ok((mirror, commit))
}

async fn checkout(mirror: &Path, commit: &str, directory: &Path) -> Result<Worktree, Error> {
    prune(mirror).await?;
    let worktree = Worktree::create(mirror, directory, "snapshot", true).await?;
    worktree.reset(commit).await?;
    Ok(worktree)
}

async fn prune(mirror: &Path) -> Result<(), Error> {
    let mut secrets = Vec::new();
    git(&["worktree", "prune"], mirror, None, &mut secrets).await?;
    Ok(())
}

async fn resolve_commit(mirror: &Path, reference: &str) -> Result<String, Error> {
    let expression = format!("{reference}^{{commit}}");
    let mut secrets = Vec::new();
    let bytes = git(&["rev-parse", &expression], mirror, None, &mut secrets).await?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}
