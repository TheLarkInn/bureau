//! Detached worktrees pinned to exact commits.

use std::path::Path;

use super::{CheckoutCache, Credential, Error, Worktree, git};

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

/// Reads the blob at `<commit>:<path>` from a mirror, verbatim.
///
/// # Errors
/// Propagates Git failures; a path absent from the commit is `Ok(None)`.
pub async fn show_blob(mirror: &Path, commit: &str, path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let object = format!("{commit}:{}", path.display());
    let mut secrets = Vec::new();
    match git(&["show", &object], mirror, None, &mut secrets).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(Error::Command { ref detail, .. }) if detail.contains("does not exist") => Ok(None),
        Err(error) => Err(error),
    }
}

impl CheckoutCache {
    /// Mirrors `url` and resolves `reference` to its exact commit.
    ///
    /// # Errors
    /// Propagates mirror and ref-resolution failures.
    pub async fn resolve_reference(
        &self,
        url: &str,
        credential: Option<&Credential>,
        reference: &str,
    ) -> Result<(std::path::PathBuf, String), Error> {
        let mirror = self.mirror(url, credential).await?;
        let commit = resolve_commit(&mirror, reference).await?;
        Ok((mirror, commit))
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

async fn checkout(mirror: &Path, commit: &str, directory: &Path) -> Result<Worktree, Error> {
    prune(mirror).await?;
    let worktree = Worktree::create(mirror, directory, "snapshot", true).await?;
    worktree.reset(commit).await?;
    Ok(worktree)
}

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
        let (mirror, commit) = self.resolve_reference(url, credential, reference).await?;
        let worktree = checkout(&mirror, &commit, directory).await?;
        Ok((worktree, commit))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{CheckoutCache, show_blob};

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    fn seeded_remote(root: &Path) -> PathBuf {
        let work = root.join("work");
        git(root, &["init", "-b", "main", "work"]);
        std::fs::create_dir_all(work.join(".bureau")).expect("mkdir");
        std::fs::write(work.join(".bureau/repos.yaml"), b"repos: {}\n").expect("write");
        let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
        git(&work, &[&id[..], &["add", "-A"][..]].concat());
        git(&work, &[&id[..], &["commit", "-m", "init"][..]].concat());
        git(root, &["clone", "--bare", "work", "remote.git"]);
        root.join("remote.git")
    }

    #[tokio::test]
    async fn resolves_a_reference_and_reads_committed_blobs() {
        let root = std::env::temp_dir().join(format!("bureau-snapshot-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let url = seeded_remote(&root).to_string_lossy().into_owned();
        let cache = CheckoutCache::new(root.join("cache"));
        let resolved = cache
            .resolve_reference(&url, None, "main")
            .await
            .expect("resolve");
        let found = show_blob(&resolved.0, &resolved.1, Path::new(".bureau/repos.yaml")).await;
        let missing = show_blob(&resolved.0, &resolved.1, Path::new("absent.yaml")).await;
        std::fs::remove_dir_all(&root).expect("cleanup");
        let binding = found.expect("blob");
        let blobs = (binding.as_deref(), missing.expect("absent"));
        assert_eq!(blobs, (Some(&b"repos: {}\n"[..]), None));
    }
}
