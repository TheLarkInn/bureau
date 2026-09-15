use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::ForgeKind;
use bureau::forge::github::GitHubForge;
use bureau::forge::github::cloud::{Client, RepositoryRef};
use bureau::github_cloud::{Control, ExpectedIdentity, Selection, SelectionRequest, State, select};
use bureau::process::Secret;
use bureau::state::Store;

use super::args::NetworkArgs;
use crate::cli::{Paths, run::committed};

pub(super) fn runs_root(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    Ok(bureau::home::Home::discover()?
        .layout()
        .root()
        .join("github-cloud-runs"))
}

pub(super) fn paths(
    network: &NetworkArgs,
    runs: Option<&Path>,
    state: Option<PathBuf>,
) -> anyhow::Result<Paths> {
    let mut paths = crate::cli::paths(
        network.settings.clone(),
        network.config_cache.clone(),
        None,
        state,
        None,
    )?;
    paths.runs = runs_root(runs)?;
    Ok(paths)
}

fn expected(args: &NetworkArgs, previous: Option<&State>) -> anyhow::Result<ExpectedIdentity> {
    if let Some(login) = &args.expected_login {
        return Ok(ExpectedIdentity::Login(login.clone()));
    }
    let previous = previous.context("--expected-login is required for initial cloud selection")?;
    Ok(ExpectedIdentity::Id(previous.start.scope.principal_id))
}

fn selection_request(
    loaded: &committed::Loaded,
    args: &NetworkArgs,
    previous: Option<&State>,
) -> anyhow::Result<SelectionRequest> {
    let name = args
        .repo
        .as_deref()
        .or_else(|| previous.map(|state| state.start.scope.registry_name.as_str()))
        .context("--repo is required for cloud selection")?;
    let repo = loaded
        .config
        .repos
        .get(name)
        .context("selected repository is not in committed config")?;
    anyhow::ensure!(
        repo.forge == ForgeKind::Github,
        "cloud controls require a registered GitHub repository"
    );
    RepositoryRef::parse(&repo.url)?;
    Ok(SelectionRequest {
        registry_name: name.to_owned(),
        repo: repo.clone(),
        config_source: loaded.source.clone(),
    })
}

pub(super) struct Context {
    pub(super) client: Client,
    pub(super) selection: Selection,
    pub(super) paths: Paths,
    secrets: Vec<Secret>,
    _maintenance: bureau::maintenance::Guard,
}

impl Context {
    pub(super) fn control(&self) -> anyhow::Result<Control<'_>> {
        Ok(Control {
            client: &self.client,
            selection: &self.selection,
            store: Arc::new(Store::open(&self.paths.state)?),
            root: &self.paths.runs,
            secrets: &self.secrets,
        })
    }

    pub(super) fn value(&self, value: &impl serde::Serialize) -> anyhow::Result<serde_json::Value> {
        let mut value = serde_json::to_value(value)?;
        self.client.scrub(&mut value);
        Ok(value)
    }
}

pub(super) async fn connect(
    paths: Paths,
    args: &NetworkArgs,
    previous: Option<&State>,
) -> anyhow::Result<Context> {
    let identity = expected(args, previous)?;
    let maintenance = bureau::maintenance::shared(&paths.maintenance_root)?;
    let loaded = committed::load(&paths.settings, &paths.config_cache).await?;
    let request = selection_request(&loaded, args, previous)?;
    let token = bureau::credential::resolve(&loaded.settings, &request.repo.credential)?;
    let client = GitHubForge::new(token.clone()).cloud()?;
    let selection = select(&client, &request, &identity).await?;
    Ok(Context {
        client,
        selection,
        paths,
        secrets: vec![token],
        _maintenance: maintenance,
    })
}
