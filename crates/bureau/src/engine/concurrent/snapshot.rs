//! Internal Git snapshot without changing the run branch or real index.

use std::path::{Path, PathBuf};

use crate::engine::gitcmd;

struct Index {
    path: PathBuf,
}

impl Index {
    fn new(path: PathBuf) -> Self {
        let _ = std::fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn add(worktree: &Path, env: &[(&str, &str)]) -> Result<(), String> {
    let args = [
        "add",
        "-A",
        "--",
        ".",
        ":(exclude).ai/**",
        ":(exclude).github/copilot/settings.json",
    ];
    gitcmd::git(&args, worktree, env).await?;
    Ok(())
}

async fn write_tree(worktree: &Path, env: &[(&str, &str)]) -> Result<String, String> {
    gitcmd::git(&["read-tree", "HEAD"], worktree, env).await?;
    add(worktree, env).await?;
    gitcmd::git(&["write-tree"], worktree, env).await
}

async fn commit(
    worktree: &Path,
    tree: &str,
    index_env: &[(&str, &str)],
    group: &str,
) -> Result<String, String> {
    let head = gitcmd::git(&["rev-parse", "HEAD"], worktree, &[]).await?;
    let message = format!("bureau concurrent snapshot: {group}");
    let mut env = index_env.to_vec();
    env.extend(gitcmd::IDENTITY);
    gitcmd::git(
        &["commit-tree", tree, "-p", &head, "-m", &message],
        worktree,
        &env,
    )
    .await
}

pub(super) async fn create(worktree: &Path, run_dir: &Path, group: &str) -> Result<String, String> {
    let index = Index::new(run_dir.join(format!(".concurrent-index-{group}")));
    let index_text = index.path.to_string_lossy().into_owned();
    let env = [("GIT_INDEX_FILE", index_text.as_str())];
    let tree = write_tree(worktree, &env).await?;
    commit(worktree, &tree, &env, group).await
}
