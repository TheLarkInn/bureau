use super::{PathBuf, TestDir, Worktree, git_ok, mirrored};

async fn retained(tmp: &TestDir) -> (PathBuf, PathBuf) {
    let (_cache, mirror, _source) = mirrored(tmp, "source").await;
    let directory = tmp.path().join("retained");
    let worktree = Worktree::create(&mirror, &directory, "run/retained", false)
        .await
        .expect("create worktree");
    std::fs::write(directory.join("untracked"), "journal-dependent bytes").expect("write");
    worktree.retain();
    drop(worktree);
    (mirror, directory)
}

#[tokio::test]
async fn dropping_retained_worktree_preserves_files_and_registration() {
    let tmp = TestDir::new("retain");
    let (mirror, directory) = retained(&tmp).await;
    let listed = git_ok(&mirror, &["worktree", "list", "--porcelain"]);
    let content = std::fs::read_to_string(directory.join("untracked")).expect("retained file");
    assert_eq!(
        (listed.matches("worktree ").count(), content.as_str()),
        (2, "journal-dependent bytes")
    );
}

#[tokio::test]
async fn reopening_retained_worktree_preserves_dirty_content() {
    let tmp = TestDir::new("resume-retained");
    let (mirror, directory) = retained(&tmp).await;
    let worktree = Worktree::resume(&mirror, &directory, "run/retained")
        .await
        .expect("reopen without resetting");
    let content = std::fs::read_to_string(directory.join("untracked")).expect("retained file");
    assert_eq!(
        (worktree.is_retained(), content.as_str(), worktree.branch()),
        (true, "journal-dependent bytes", "run/retained")
    );
}

#[tokio::test]
async fn resuming_another_branch_fails_without_removing_the_worktree() {
    let tmp = TestDir::new("resume-wrong-branch");
    let (mirror, directory) = retained(&tmp).await;
    let result = Worktree::resume(&mirror, &directory, "run/other").await;
    assert_eq!(
        (result.is_err(), directory.join("untracked").exists()),
        (true, true)
    );
}

#[tokio::test]
async fn resuming_another_repository_fails_without_removing_the_worktree() {
    let tmp = TestDir::new("resume-wrong-repo");
    let (_mirror, directory) = retained(&tmp).await;
    let result = Worktree::resume(tmp.path(), &directory, "run/retained").await;
    assert_eq!(
        (result.is_err(), directory.join("untracked").exists()),
        (true, true)
    );
}

#[tokio::test]
async fn acknowledged_cleanup_removes_a_retained_worktree() {
    let tmp = TestDir::new("retained-cleanup");
    let (mirror, directory) = retained(&tmp).await;
    let worktree = Worktree::resume(&mirror, &directory, "run/retained")
        .await
        .expect("reopen retained worktree");
    worktree.allow_cleanup();
    drop(worktree);
    assert!(!directory.exists());
}
