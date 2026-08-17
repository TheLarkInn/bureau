//! Exact committed configuration loading for one-shot execution.

use std::path::Path;

use anyhow::Context as _;

pub(super) struct Loaded {
    pub(super) config: bureau::config::Config,
    pub(super) settings: bureau::setup::Settings,
    pub(super) source: bureau::runlog::ConfigSource,
    pub(super) direct_agents: std::collections::BTreeMap<String, Vec<u8>>,
}

fn infer_forge(remote: &str) -> bureau::config::ForgeKind {
    if remote.contains("dev.azure.com") || remote.contains("/_git/") {
        bureau::config::ForgeKind::Ado
    } else {
        bureau::config::ForgeKind::Github
    }
}

fn config_credential(
    settings: &bureau::setup::Settings,
) -> anyhow::Result<Option<bureau::git::Credential>> {
    let Some(_) = settings.credentials.get("config") else {
        return Ok(None);
    };
    let secret = bureau::credential::resolve(settings, "config")?;
    let forge = infer_forge(settings.config.remote());
    Ok(Some(bureau::git::credential_for(forge, secret)))
}

pub(super) async fn load(settings_path: &Path, cache: &Path) -> anyhow::Result<Loaded> {
    let settings = bureau::setup::load_settings(settings_path).context("loading settings")?;
    let credential = config_credential(&settings)?;
    let source = &settings.config;
    let git = bureau::config::GitSource::new(
        source.remote().to_owned(),
        source.reference().to_owned(),
        source.subdirectory().to_path_buf(),
        cache,
        credential,
    );
    let active = git.load().await.context("loading committed config")?;
    Ok(Loaded {
        config: active.config,
        source: bureau::runlog::ConfigSource {
            remote: active.remote,
            reference: active.reference,
            commit: active.commit,
        },
        direct_agents: active.direct_agents,
        settings,
    })
}
