use crate::cli::out;
use std::path::PathBuf;

use bureau::forge::PrRequest;
use bureau::setup::{ConfigDraft, ConfigPullRequest, Settings};

use super::access;
use super::files;

pub(super) struct Proposal {
    pub(super) pull_request: ConfigPullRequest,
    pub(super) repo: String,
    pub(super) number: u64,
    pub(super) branch: String,
    pub(super) commit: String,
}

impl Proposal {
    /// The draft already sits at the tracked reference byte for byte: an
    /// interrupted run's pull request landed, so resume past proposing it.
    /// `number: 0` never collides — forge pull-request numbers start at 1.
    pub(super) fn premerged(access: &access::Access, reference: &str, commit: String) -> Self {
        Self {
            pull_request: ConfigPullRequest {
                id: reference.to_owned(),
            },
            repo: access.repo.clone(),
            number: 0,
            branch: String::new(),
            commit,
        }
    }

    /// True when the config was already merged and no pull request exists.
    pub(super) const fn is_premerged(&self) -> bool {
        self.number == 0
    }
}

/// Byte-compares the draft against the committed config at `commit`.
async fn already_merged(
    mirror: &std::path::Path,
    commit: &str,
    subdir: &std::path::Path,
    draft: &ConfigDraft,
) -> anyhow::Result<bool> {
    for (relative, bytes) in &draft.files {
        let path = if subdir == std::path::Path::new(".") {
            relative.clone()
        } else {
            subdir.join(relative)
        };
        let committed = bureau::git::snapshot::show_blob(mirror, commit, &path).await?;
        if committed.as_deref() != Some(bytes.as_slice()) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn branch() -> String {
    // The process clock boundary: bound once as a function pointer so
    // this stays the single read site.
    let now = std::time::SystemTime::now;
    let nanos = now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("bureau/init-{}-{nanos}", std::process::id())
}

fn worktree_path(layout: &bureau::home::Layout, branch: &str) -> anyhow::Result<PathBuf> {
    let root = layout.config_cache().join("init-worktrees");
    std::fs::create_dir_all(&root)?;
    Ok(root.join(branch.replace('/', "-")))
}

fn pull_request(settings: &Settings, repo: String, branch: String) -> PrRequest {
    PrRequest {
        repo,
        branch,
        base: settings.config.reference().to_owned(),
        title: "Add bureau configuration".to_owned(),
        body: "Adds the reviewed configuration for bureau initialization.".to_owned(),
        item_id: None,
    }
}

async fn checkout(
    layout: &bureau::home::Layout,
    settings: &Settings,
    access: &access::Access,
) -> anyhow::Result<(bureau::git::Worktree, String)> {
    let cache = bureau::git::CheckoutCache::new(layout.config_cache().join("init-mirrors"));
    let mirror = cache
        .mirror(settings.config.remote(), Some(&access.git))
        .await?;
    let branch = branch();
    let path = worktree_path(layout, &branch)?;
    let worktree = bureau::git::Worktree::create(&mirror, &path, &branch, false).await?;
    Ok((worktree, branch))
}

async fn publish(
    worktree: &bureau::git::Worktree,
    settings: &Settings,
    access: &access::Access,
) -> anyhow::Result<String> {
    let commit = worktree.commit_all("Add bureau configuration").await?;
    worktree
        .push(settings.config.remote(), Some(&access.git))
        .await?;
    Ok(commit)
}

async fn open(
    settings: &Settings,
    access: &access::Access,
    branch: String,
) -> anyhow::Result<bureau::forge::Pr> {
    let request = pull_request(settings, access.repo.clone(), branch);
    Ok(access.forge.create_pr(&request).await?)
}

async fn propose(
    layout: &bureau::home::Layout,
    settings: &Settings,
    access: &access::Access,
    draft: &ConfigDraft,
) -> anyhow::Result<Proposal> {
    let (worktree, branch) = checkout(layout, settings, access).await?;
    let root = worktree.path().join(settings.config.subdirectory());
    files::materialize(&root, draft)?;
    let commit = publish(&worktree, settings, access).await?;
    let created = open(settings, access, branch.clone()).await?;
    out::line(format_args!("config pull request: {}", created.url));
    Ok(Proposal {
        pull_request: ConfigPullRequest {
            id: created.url.clone(),
        },
        repo: created.repo,
        number: created.number,
        branch,
        commit,
    })
}

/// The remote's current head, when it already carries this exact draft.
async fn merged_head(
    layout: &bureau::home::Layout,
    settings: &Settings,
    access: &access::Access,
    draft: &ConfigDraft,
) -> anyhow::Result<Option<String>> {
    let cache = bureau::git::CheckoutCache::new(layout.config_cache().join("init-mirrors"));
    let (mirror, head) = cache
        .resolve_reference(
            settings.config.remote(),
            Some(&access.git),
            settings.config.reference(),
        )
        .await?;
    let merged = already_merged(&mirror, &head, settings.config.subdirectory(), draft).await?;
    Ok(merged.then_some(head))
}

pub(super) async fn create(
    layout: &bureau::home::Layout,
    settings: &Settings,
    draft: &ConfigDraft,
) -> anyhow::Result<Proposal> {
    let access = access::verified(settings).await?;
    if let Some(head) = merged_head(layout, settings, &access, draft).await? {
        return Ok(Proposal::premerged(
            &access,
            settings.config.reference(),
            head,
        ));
    }
    propose(layout, settings, &access, draft).await
}

pub(super) fn display(proposal: &Proposal) {
    out::line(format_args!(
        "waiting for {} at {} ({})",
        proposal.pull_request.id, proposal.branch, proposal.commit
    ));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use bureau::git::CheckoutCache;
    use bureau::setup::ConfigDraft;

    use super::already_merged;

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    fn draft(path: &str, bytes: &[u8]) -> ConfigDraft {
        ConfigDraft {
            files: BTreeMap::from([(PathBuf::from(path), bytes.to_vec())]),
        }
    }

    fn seeded_remote(root: &Path) -> PathBuf {
        let work = root.join("work");
        git(root, &["init", "-b", "main", "work"]);
        std::fs::create_dir_all(work.join(".bureau")).expect("mkdir");
        std::fs::write(work.join(".bureau/repos.yaml"), b"a: 1\n").expect("write");
        std::fs::write(work.join("repos.yaml"), b"b: 2\n").expect("write");
        let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
        git(&work, &[&id[..], &["add", "-A"][..]].concat());
        git(&work, &[&id[..], &["commit", "-m", "init"][..]].concat());
        git(root, &["clone", "--bare", "work", "remote.git"]);
        root.join("remote.git")
    }

    async fn mirror_of(root: &Path) -> (PathBuf, String) {
        let url = seeded_remote(root).to_string_lossy().into_owned();
        CheckoutCache::new(root.join("cache"))
            .resolve_reference(&url, None, "main")
            .await
            .expect("resolve")
    }

    #[tokio::test]
    async fn already_merged_matches_only_identical_committed_bytes() {
        let root = std::env::temp_dir().join(format!("bureau-premerged-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let (mirror, commit) = mirror_of(&root).await;
        let cases = [
            (draft("repos.yaml", b"a: 1\n"), ".bureau", true),
            (draft("repos.yaml", b"a: 2\n"), ".bureau", false),
            (draft("absent.yaml", b"x\n"), ".bureau", false),
            (draft("repos.yaml", b"b: 2\n"), ".", true),
        ];
        let mut found = Vec::new();
        for (draft, subdir, _) in &cases {
            found.push(
                already_merged(&mirror, &commit, Path::new(subdir), draft)
                    .await
                    .expect("x"),
            );
        }
        std::fs::remove_dir_all(&root).expect("cleanup");
        assert_eq!(found, [true, false, false, true]);
    }
}
