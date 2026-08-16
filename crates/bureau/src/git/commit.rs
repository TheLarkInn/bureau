use super::{Error, Worktree, git};

impl Worktree {
    /// Commits every worktree change with bureau's deterministic identity.
    ///
    /// # Errors
    /// Propagates add, commit, and revision lookup failures.
    pub async fn commit_all(&self, message: &str) -> Result<String, Error> {
        let mut secrets = Vec::new();
        git(&["add", "-A"], &self.dir, None, &mut secrets).await?;
        let identity = [
            "-c",
            "user.name=Bureau",
            "-c",
            "user.email=bureau@localhost",
            "commit",
            "-m",
            message,
        ];
        git(&identity, &self.dir, None, &mut secrets).await?;
        let bytes = git(&["rev-parse", "HEAD"], &self.dir, None, &mut secrets).await?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    }
}
