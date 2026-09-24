//! Concurrent refreshes of one mirror: first-use clones, fetches while
//! the remote advances, and run branches cut between refreshes. Without
//! the mirror lock these race on the clone target and on
//! `refs/heads/main`, and a refresh prunes other runs' unpushed branches.

use std::time::Duration;

use tokio::task::JoinSet;

use super::{
    CheckoutCache, Path, PathBuf, TestDir, Worktree, commit_and_push, git_ok, make_source, mirrored,
};

/// Concurrent refreshes per round.
const RUNS: usize = 6;
/// Rounds per test; each round advances the remote first.
const ROUNDS: usize = 5;

/// Starts `RUNS` refreshes of `url` at once; returns their failures.
async fn refresh_all(cache: &CheckoutCache, url: &str) -> Vec<String> {
    let mut refreshes = JoinSet::new();
    for _ in 0..RUNS {
        let (cache, url) = (cache.clone(), url.to_owned());
        refreshes.spawn(async move { cache.mirror(&url, None).await.err() });
    }
    let results = refreshes.join_all().await;
    results
        .into_iter()
        .flatten()
        .map(|e| e.to_string())
        .collect()
}

/// A run's setup: refresh under the lock, cut the branch, release.
async fn cut(cache: CheckoutCache, url: String, dir: PathBuf, branch: String) -> Option<String> {
    let wait = Duration::from_secs(60);
    let (mirror, lock) = match cache.mirror_locked(&url, None, wait).await {
        Ok(locked) => locked,
        Err(error) => return Some(error.to_string()),
    };
    let created = Worktree::create(&mirror, &dir, &branch, false).await;
    drop(lock);
    created
        .map(|worktree| worktree.retain())
        .err()
        .map(|e| e.to_string())
}

/// Cuts `RUNS` run branches at once under `root`; returns failures.
async fn cut_all(cache: &CheckoutCache, url: &str, root: &Path, round: usize) -> Vec<String> {
    let mut cuts = JoinSet::new();
    for run in 0..RUNS {
        let dir = root.join(format!("wt-{round}-{run}"));
        let branch = format!("run/{round}-{run}");
        cuts.spawn(cut(cache.clone(), url.to_owned(), dir, branch));
    }
    cuts.join_all().await.into_iter().flatten().collect()
}

/// Advances the remote and cuts `RUNS` branches, `ROUNDS` times, then
/// refreshes once more; returns every failure.
async fn cut_rounds(cache: &CheckoutCache, source: &super::Source, root: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    for round in 0..ROUNDS {
        commit_and_push(&source.work, &format!("round {round}"));
        failures.extend(cut_all(cache, &source.url(), root, round).await);
    }
    failures.extend(refresh_all(cache, &source.url()).await);
    failures
}

/// Whether a second lock attempt times out while the first is held, and
/// whether a third succeeds once it is dropped.
async fn lock_states(cache: &CheckoutCache, url: &str) -> (bool, bool) {
    let held = cache.mirror_locked(url, None, Duration::ZERO).await;
    let busy = cache.mirror_locked(url, None, Duration::ZERO).await.err();
    let timed_out = busy.is_some_and(|error| match error {
        bureau::git::Error::Io(error) => error.kind() == std::io::ErrorKind::TimedOut,
        bureau::git::Error::Command { .. } => false,
    });
    drop(held.expect("first lock"));
    let freed = cache.mirror_locked(url, None, Duration::ZERO).await;
    (timed_out, freed.is_ok())
}

fn run_branches(mirror: &Path) -> usize {
    let listed = git_ok(
        mirror,
        &["for-each-ref", "--format=%(refname)", "refs/heads/run/"],
    );
    listed.lines().count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_use_clones_all_succeed() {
    let tmp = TestDir::new("first-use");
    let source = make_source(&tmp, "src");
    let mut failures = Vec::new();
    for round in 0..ROUNDS {
        let cache = CheckoutCache::new(tmp.path().join(format!("cache-{round}")));
        failures.extend(refresh_all(&cache, &source.url()).await);
    }
    assert_eq!(failures, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_fetches_while_the_remote_advances_all_succeed() {
    let tmp = TestDir::new("advance");
    let (cache, mirror, source) = mirrored(&tmp, "src").await;
    let (mut failures, mut current) = (Vec::new(), Vec::new());
    for round in 0..ROUNDS {
        let head = commit_and_push(&source.work, &format!("round {round}"));
        failures.extend(refresh_all(&cache, &source.url()).await);
        current.push(git_ok(&mirror, &["rev-parse", "refs/heads/main"]) == head);
    }
    assert_eq!((failures, current), (Vec::new(), vec![true; ROUNDS]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_branch_cuts_while_the_remote_advances_keep_every_branch() {
    let tmp = TestDir::new("cuts");
    let (cache, mirror, source) = mirrored(&tmp, "src").await;
    let failures = cut_rounds(&cache, &source, tmp.path()).await;
    assert_eq!(
        (failures, run_branches(&mirror)),
        (Vec::new(), RUNS * ROUNDS)
    );
}

#[tokio::test]
async fn a_refresh_prunes_only_branches_without_a_worktree() {
    let tmp = TestDir::new("prune");
    let (cache, mirror, source) = mirrored(&tmp, "src").await;
    let live = Worktree::create(&mirror, &tmp.path().join("live"), "run/live", false)
        .await
        .expect("live worktree");
    git_ok(&mirror, &["branch", "run/stale", "main"]);
    commit_and_push(&source.work, "two");
    cache.mirror(&source.url(), None).await.expect("refresh");
    let kept = ["run/live", "run/stale"].map(|branch| {
        let reference = format!("refs/heads/{branch}");
        git_ok(
            &mirror,
            &["for-each-ref", "--format=%(refname)", &reference],
        ) == reference
    });
    drop(live);
    assert_eq!(kept, [true, false]);
}

#[tokio::test]
async fn a_busy_mirror_lock_times_out_and_frees_on_drop() {
    let tmp = TestDir::new("busy");
    let (cache, _mirror, source) = mirrored(&tmp, "src").await;
    assert_eq!(lock_states(&cache, &source.url()).await, (true, true));
}
