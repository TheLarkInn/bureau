use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::ForgeKind;
use bureau::forge::Forge;
use bureau::forge::ado::AdoForge;
use bureau::forge::github::GitHubForge;
use bureau::git::{Credential, credential_for};
use bureau::process::Secret;

pub(super) struct Access {
    pub(super) forge: Arc<dyn Forge>,
    pub(super) git: Credential,
    pub(super) repo: String,
}

fn host(remote: &str) -> anyhow::Result<String> {
    if let Ok(url) = reqwest::Url::parse(remote) {
        let host = url
            .host_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow::anyhow!("config remote has no host"))?;
        return Ok(url
            .port()
            .map_or_else(|| host.clone(), |port| format!("{host}:{port}")));
    }
    let value = remote
        .split_once('@')
        .and_then(|(_, tail)| tail.split_once(':'))
        .map(|(host, _)| host.to_owned());
    value.ok_or_else(|| anyhow::anyhow!("unsupported config remote `{remote}`"))
}

fn github(remote: &str, secret: Secret) -> anyhow::Result<GitHubForge> {
    let host = host(remote)?;
    if host == "github.com" {
        return Ok(GitHubForge::new(secret));
    }
    let scheme = reqwest::Url::parse(remote).map_or("https", |url| {
        if url.scheme() == "http" {
            "http"
        } else {
            "https"
        }
    });
    let base = format!("{scheme}://{host}/api/v3");
    Ok(GitHubForge::new(secret).with_base_url(base))
}

fn ado_base(remote: &str) -> String {
    let head = remote.split("/_git/").next().unwrap_or(remote);
    head.rsplit_once('/')
        .map_or_else(|| head.to_owned(), |(base, _)| base.to_owned())
}

fn forge(kind: ForgeKind, remote: &str, secret: Secret) -> anyhow::Result<Arc<dyn Forge>> {
    Ok(match kind {
        ForgeKind::Github => Arc::new(github(remote, secret)?),
        ForgeKind::Ado => Arc::new(AdoForge::new(ado_base(remote), secret)),
    })
}

fn remote_path(remote: &str) -> anyhow::Result<String> {
    if let Ok(url) = reqwest::Url::parse(remote) {
        return Ok(url.path().to_owned());
    }
    remote
        .split_once(':')
        .map(|(_, path)| path.to_owned())
        .ok_or_else(|| anyhow::anyhow!("unsupported config remote `{remote}`"))
}

fn github_repo(segments: &[&str]) -> anyhow::Result<String> {
    let [.., owner, repo] = segments else {
        anyhow::bail!("GitHub config remote must identify owner/repository");
    };
    Ok(format!("{owner}/{}", repo.trim_end_matches(".git")))
}

fn ado_repo(segments: &[&str]) -> anyhow::Result<String> {
    let position = segments
        .iter()
        .position(|segment| *segment == "_git")
        .ok_or_else(|| anyhow::anyhow!("ADO config remote must contain `/_git/`"))?;
    let project = position
        .checked_sub(1)
        .and_then(|index| segments.get(index))
        .context("ADO config remote has no project")?;
    let repo = segments
        .get(position + 1)
        .context("ADO config remote has no repository")?;
    Ok(format!("{project}/{}", repo.trim_end_matches(".git")))
}

fn repo_name(kind: ForgeKind, remote: &str) -> anyhow::Result<String> {
    let path = remote_path(remote)?;
    let segments: Vec<_> = path.trim_matches('/').split('/').collect();
    match kind {
        ForgeKind::Github => github_repo(&segments),
        ForgeKind::Ado => ado_repo(&segments),
    }
}

pub(super) fn forge_kind(remote: &str) -> ForgeKind {
    if remote.contains("/_git/") {
        ForgeKind::Ado
    } else {
        ForgeKind::Github
    }
}

pub(super) fn resolve(settings: &bureau::setup::Settings) -> anyhow::Result<Access> {
    let remote = settings.config.remote();
    let kind = forge_kind(remote);
    let secret = bureau::credential::resolve(settings, "config")?;
    let forge = forge(kind, remote, secret.clone())?;
    Ok(Access {
        forge,
        git: credential_for(kind, secret),
        repo: repo_name(kind, remote)?,
    })
}

pub(super) fn credential(settings: &bureau::setup::Settings) -> anyhow::Result<Option<Credential>> {
    settings
        .credentials
        .contains_key("config")
        .then(|| resolve(settings).map(|access| access.git))
        .transpose()
}
