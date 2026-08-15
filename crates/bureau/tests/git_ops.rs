//! Layer 6 git tests: offline, local bare repos over `file://` only
//! (DESIGN.md section 7). Setup uses git directly; the code under test
//! goes through the layer-0 process contract with an empty environment,
//! so these tests also prove git works without `HOME` or `PATH`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::ForgeKind;
use bureau::git::{CheckoutCache, Worktree, auth_args, credential_for};
use bureau::process::Secret;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-git-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Synchronous git for test setup; panics on failure, returns stdout.
fn git_ok(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "git {args:?} failed: {stderr}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare remote plus a working clone with one commit on `main`.
struct Source {
    remote: PathBuf,
    work: PathBuf,
}

impl Source {
    fn url(&self) -> String {
        format!("file://{}", self.remote.display())
    }
}

fn make_source(tmp: &TestDir, name: &str) -> Source {
    let remote = tmp.path().join(format!("{name}.git"));
    let work = tmp.path().join(name);
    let r = remote.to_string_lossy().into_owned();
    let w = work.to_string_lossy().into_owned();
    git_ok(
        tmp.path(),
        &["-c", "init.defaultBranch=main", "init", "--bare", &r],
    );
    git_ok(tmp.path(), &["clone", &r, &w]);
    std::fs::write(work.join("file.txt"), "one").expect("write");
    git_ok(&work, &["add", "file.txt"]);
    commit(&work, "one");
    git_ok(&work, &["push", "origin", "main"]);
    Source { remote, work }
}

fn commit(work: &Path, message: &str) {
    git_ok(
        work,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-m",
            message,
        ],
    );
}

/// Adds a second commit in the working clone, pushes, returns its hash.
fn commit_and_push(work: &Path, message: &str) -> String {
    std::fs::write(work.join("file.txt"), message).expect("write");
    git_ok(work, &["add", "file.txt"]);
    commit(work, message);
    git_ok(work, &["push", "origin", "main"]);
    git_ok(work, &["rev-parse", "HEAD"])
}

/// A mirror of a fresh source, plus the cache that made it.
async fn mirrored(tmp: &TestDir, tag: &str) -> (CheckoutCache, PathBuf, Source) {
    let source = make_source(tmp, tag);
    let cache = CheckoutCache::new(tmp.path().join("cache"));
    let mirror = cache.mirror(&source.url(), None).await.expect("mirror");
    (cache, mirror, source)
}

/// The auth argv carries the full header value; the scrub list holds
/// every form the credential can take in argv, on the wire, or echoed
/// by a reflected error page: raw secret, base64 pair, full header.
#[test]
fn auth_args_scrub_every_form_of_the_credential() {
    let credential = credential_for(ForgeKind::Github, Secret::new("s3cret"));
    let mut secrets = Vec::new();
    let argv = auth_args(&credential, &mut secrets);
    let pair = "eC1hY2Nlc3MtdG9rZW46czNjcmV0";
    let header = format!("AUTHORIZATION: Basic {pair}");
    let expected = vec!["-c".to_owned(), format!("http.extraheader={header}")];
    let scrubbed = [
        secrets.contains(&Secret::new("s3cret")),
        secrets.contains(&Secret::new(pair)),
        secrets.contains(&Secret::new(header.as_str())),
    ];
    assert_eq!((argv, scrubbed), (expected, [true, true, true]));
}

#[test]
fn mirror_dir_is_stable_and_url_keyed() {
    let cache = CheckoutCache::new(PathBuf::from("/cache"));
    let a = cache.mirror_dir("https://example.com/r.git");
    let b = cache.mirror_dir("https://example.com/r.git");
    let c = cache.mirror_dir("https://example.com/other.git");
    let keyed = (a == b, a != c, a.starts_with("/cache"));
    assert_eq!(keyed, (true, true, true));
}

#[tokio::test]
async fn mirror_creates_a_bare_mirror() {
    let tmp = TestDir::new("mirror");
    let (cache, mirror, source) = mirrored(&tmp, "src").await;
    let m = mirror.to_string_lossy().into_owned();
    let bare = git_ok(
        tmp.path(),
        &["--git-dir", &m, "rev-parse", "--is-bare-repository"],
    );
    let state = (
        mirror == cache.mirror_dir(&source.url()),
        bare,
        mirror.join("HEAD").exists(),
    );
    assert_eq!(state, (true, "true".to_owned(), true));
}

#[tokio::test]
async fn mirror_fetches_new_commits_and_reuses_the_clone() {
    let tmp = TestDir::new("fetch");
    let (cache, mirror, source) = mirrored(&tmp, "src").await;
    std::fs::write(mirror.join("SENTINEL"), "keep").expect("sentinel");
    let new_head = commit_and_push(&source.work, "two");
    let url = source.url();
    let again = cache.mirror(&url, None).await.expect("second mirror");
    let m = mirror.to_string_lossy().into_owned();
    let head = git_ok(
        tmp.path(),
        &["--git-dir", &m, "rev-parse", "refs/heads/main"],
    );
    let state = (again == mirror, mirror.join("SENTINEL").exists(), head);
    assert_eq!(state, (true, true, new_head));
}

#[tokio::test]
async fn worktree_has_repo_content_on_its_branch() {
    let tmp = TestDir::new("wt");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("run");
    let wt = Worktree::create(&mirror, &dir, "run/fix-1", false)
        .await
        .expect("create");
    let d = dir.to_string_lossy().into_owned();
    let shown = git_ok(tmp.path(), &["-C", &d, "branch", "--show-current"]);
    let content = std::fs::read_to_string(dir.join("file.txt")).expect("content");
    let state = (shown, content, wt.branch(), wt.path() == dir.as_path());
    assert_eq!(
        state,
        ("run/fix-1".to_owned(), "one".to_owned(), "run/fix-1", true)
    );
}

#[tokio::test]
async fn two_worktrees_cannot_share_a_branch() {
    let tmp = TestDir::new("dup");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let first_dir = tmp.path().join("a");
    let second_dir = tmp.path().join("b");
    let first = Worktree::create(&mirror, &first_dir, "run/dup", false)
        .await
        .expect("first");
    let second = Worktree::create(&mirror, &second_dir, "run/dup", false).await;
    let clean = (second.is_err(), second_dir.exists(), first_dir.exists());
    assert_eq!(clean, (true, false, true));
    drop(first);
}

#[tokio::test]
async fn detach_mode_checks_out_no_branch() {
    let tmp = TestDir::new("detach");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("ro");
    let _wt = Worktree::create(&mirror, &dir, "run/ro", true)
        .await
        .expect("create");
    let d = dir.to_string_lossy().into_owned();
    let m = mirror.to_string_lossy().into_owned();
    let shown = git_ok(tmp.path(), &["-C", &d, "branch", "--show-current"]);
    let listed = git_ok(tmp.path(), &["--git-dir", &m, "branch", "--list", "run/ro"]);
    let state = (shown, listed, dir.join("file.txt").exists());
    assert_eq!(state, (String::new(), String::new(), true));
}

#[tokio::test]
async fn push_lands_the_branch_on_a_second_remote() {
    let tmp = TestDir::new("push");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("run");
    let wt = Worktree::create(&mirror, &dir, "run/push", false)
        .await
        .expect("create");
    let target = tmp.path().join("target.git");
    let t = target.to_string_lossy().into_owned();
    git_ok(
        tmp.path(),
        &["-c", "init.defaultBranch=main", "init", "--bare", &t],
    );
    wt.push(&format!("file://{t}"), None).await.expect("push");
    let branches = git_ok(
        tmp.path(),
        &["--git-dir", &t, "branch", "--list", "run/push"],
    );
    assert_eq!(branches, "run/push");
}

#[tokio::test]
async fn drop_removes_the_worktree_and_its_registration() {
    let tmp = TestDir::new("drop");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("run");
    let wt = Worktree::create(&mirror, &dir, "run/drop", false)
        .await
        .expect("create");
    drop(wt);
    let m = mirror.to_string_lossy().into_owned();
    let listed = git_ok(
        tmp.path(),
        &["--git-dir", &m, "worktree", "list", "--porcelain"],
    );
    let gone = (dir.exists(), listed.matches("worktree ").count());
    assert_eq!(gone, (false, 1));
}

#[tokio::test]
async fn drop_after_external_removal_is_safe() {
    let tmp = TestDir::new("gone");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("run");
    let wt = Worktree::create(&mirror, &dir, "run/gone", false)
        .await
        .expect("create");
    std::fs::remove_dir_all(&dir).expect("external removal");
    drop(wt);
    assert!(!dir.exists());
}

#[tokio::test]
async fn drop_when_the_mirror_is_gone_still_removes_the_dir() {
    let tmp = TestDir::new("nomirror");
    let (_cache, mirror, _source) = mirrored(&tmp, "src").await;
    let dir = tmp.path().join("run");
    let wt = Worktree::create(&mirror, &dir, "run/solo", false)
        .await
        .expect("create");
    std::fs::remove_dir_all(&mirror).expect("remove mirror");
    drop(wt);
    assert!(!dir.exists());
}
