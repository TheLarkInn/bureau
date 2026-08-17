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
) -> anyhow::Result<(access::Access, bureau::git::Worktree, String)> {
    let access = access::resolve(settings)?;
    let cache = bureau::git::CheckoutCache::new(layout.config_cache().join("init-mirrors"));
    let mirror = cache
        .mirror(settings.config.remote(), Some(&access.git))
        .await?;
    let branch = branch();
    let path = worktree_path(layout, &branch)?;
    let worktree = bureau::git::Worktree::create(&mirror, &path, &branch, false).await?;
    Ok((access, worktree, branch))
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

pub(super) async fn create(
    layout: &bureau::home::Layout,
    settings: &Settings,
    draft: &ConfigDraft,
) -> anyhow::Result<Proposal> {
    let (access, worktree, branch) = checkout(layout, settings).await?;
    let root = worktree.path().join(settings.config.subdirectory());
    files::materialize(&root, draft)?;
    let commit = publish(&worktree, settings, &access).await?;
    let created = open(settings, &access, branch.clone()).await?;
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

pub(super) fn display(proposal: &Proposal) {
    out::line(format_args!(
        "waiting for {} at {} ({})",
        proposal.pull_request.id, proposal.branch, proposal.commit
    ));
}
