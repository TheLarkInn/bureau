//! Exact committed configuration loading for one-shot execution.

use std::path::Path;

use anyhow::Context as _;

use bureau::config::CONFIG_CREDENTIAL;

use crate::cli::prepare::config_identity;

pub(super) struct Loaded {
    pub(super) config: bureau::config::Config,
    pub(super) settings: bureau::setup::Settings,
    pub(super) source: bureau::runlog::ConfigSource,
    pub(super) direct_agents: std::collections::BTreeMap<String, Vec<u8>>,
}

/// The reserved config credential, resolved once and — when it declares
/// an identity — proved to be that account before the fetch it signs
/// carries it anywhere.
async fn config_credential(
    settings: &bureau::setup::Settings,
) -> anyhow::Result<Option<bureau::git::Credential>> {
    if !settings.credentials.contains_key(CONFIG_CREDENTIAL) {
        return Ok(None);
    }
    let (forge, secret) = config_identity::verified_secret(settings).await?;
    Ok(Some(bureau::git::credential_for(forge, secret)))
}

pub(super) async fn load(settings_path: &Path, cache: &Path) -> anyhow::Result<Loaded> {
    let settings = bureau::setup::load_settings(settings_path).context("loading settings")?;
    let credential = config_credential(&settings).await?;
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
