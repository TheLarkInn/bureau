use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::adapters::AdapterKind;
use crate::config::Config;
use crate::git::CheckoutCache;
use crate::setup::CredentialSource;

use super::LocalEffects;
use crate::doctor::{Observation, Status};

fn repository_observation(total: usize, missing: usize) -> Observation {
    if missing == 0 {
        Observation::new(
            Status::Ok,
            "repositories_ok",
            format!("{total} registered repository checkout caches are available"),
        )
    } else {
        Observation::new(
            Status::Warning,
            "repository_cache_missing",
            format!("{missing} of {total} registered repository caches are absent"),
        )
    }
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn source_resolves(
    source: &CredentialSource,
    name: &str,
    environment_names: &BTreeSet<OsString>,
) -> bool {
    match source {
        CredentialSource::Environment { variable } => {
            environment_names.contains(&OsString::from(variable))
        }
        CredentialSource::Directory { path } => regular_file(&path.join(name)),
        CredentialSource::File { path } => regular_file(path),
    }
}

fn credential_resolves(
    name: &str,
    settings: &crate::setup::Settings,
    environment_names: &BTreeSet<OsString>,
) -> bool {
    settings
        .credentials
        .get(name)
        .is_some_and(|source| source_resolves(source, name, environment_names))
}

fn credential_references(
    config: Option<&Config>,
    settings: &crate::setup::Settings,
) -> BTreeSet<String> {
    config.map_or_else(
        || settings.credentials.keys().cloned().collect(),
        |config| {
            config
                .repos
                .values()
                .map(|repo| repo.credential.clone())
                .collect()
        },
    )
}

fn credential_observation(
    references: &BTreeSet<String>,
    unresolved: &[String],
    config_available: bool,
) -> Observation {
    if !unresolved.is_empty() {
        return Observation::new(
            Status::Error,
            "credential_references_unresolved",
            format!(
                "unresolved credential references: {}",
                unresolved.join(", ")
            ),
        );
    }
    let status = if config_available {
        Status::Ok
    } else {
        Status::Warning
    };
    Observation::new(
        status,
        "credential_references_ok",
        format!("{} credential references resolve", references.len()),
    )
}

const fn adapter_binary(adapter: AdapterKind) -> Option<&'static str> {
    match adapter {
        AdapterKind::Copilot => Some("copilot"),
        AdapterKind::Claude => Some("claude"),
        AdapterKind::Fake => None,
    }
}

fn adapter_binaries(config: Option<&Config>) -> BTreeSet<String> {
    let mut required = BTreeSet::from(["git".to_owned(), "unshare".to_owned()]);
    for adapter in config
        .into_iter()
        .flat_map(|config| config.roles.values().map(|role| role.adapter))
    {
        if let Some(binary) = adapter_binary(adapter) {
            required.insert(binary.to_owned());
        }
    }
    required
}

fn adapter_observation(
    required: &BTreeSet<String>,
    missing: &[String],
    config_available: bool,
) -> Observation {
    if !missing.is_empty() {
        return Observation::new(
            Status::Error,
            "adapter_binaries_missing",
            format!(
                "required executables are unavailable: {}",
                missing.join(", ")
            ),
        );
    }
    let status = if config_available {
        Status::Ok
    } else {
        Status::Warning
    };
    Observation::new(
        status,
        "adapters_ok",
        format!("{} required executables are available", required.len()),
    )
}

impl LocalEffects {
    pub(super) fn inspect_config_source(&self) -> Result<Observation, String> {
        let settings = self.settings()?;
        if settings.config.remote().trim().is_empty()
            || settings.config.reference().trim().is_empty()
        {
            return Ok(Observation::new(
                Status::Error,
                "config_source_invalid",
                "configured remote and ref must not be empty",
            ));
        }
        match self.cached_config()? {
            Some(_) => Ok(Observation::new(
                Status::Ok,
                "config_source_ok",
                "settings and a committed config snapshot are valid",
            )),
            None => Ok(Observation::new(
                Status::Warning,
                "config_snapshot_unavailable",
                "settings are valid but no committed config snapshot is cached",
            )),
        }
    }

    pub(super) fn inspect_repositories(&self) -> Result<Observation, String> {
        let Some(config) = self.cached_config()? else {
            return Ok(Observation::new(
                Status::Warning,
                "repositories_unavailable",
                "no committed config snapshot is cached",
            ));
        };
        let cache = CheckoutCache::new(self.layout.checkout_cache().to_path_buf());
        let missing = config
            .repos
            .values()
            .filter(|repo| !safe_directory(&cache.mirror_dir(&repo.url)))
            .count();
        Ok(repository_observation(config.repos.len(), missing))
    }

    pub(super) fn inspect_credentials(&self) -> Result<Observation, String> {
        let settings = self.settings()?;
        let config = self.cached_config()?;
        let references = credential_references(config.as_ref(), settings);
        let unresolved: Vec<_> = references
            .iter()
            .filter(|name| !credential_resolves(name, settings, &self.environment_names))
            .cloned()
            .collect();
        Ok(credential_observation(
            &references,
            &unresolved,
            config.is_some(),
        ))
    }

    pub(super) fn inspect_adapters(&self) -> Result<Observation, String> {
        let config = self.cached_config()?;
        let required = adapter_binaries(config.as_ref());
        let missing: Vec<_> = required
            .iter()
            .filter(|binary| !self.binary_available(binary))
            .cloned()
            .collect();
        Ok(adapter_observation(&required, &missing, config.is_some()))
    }

    pub(super) fn cached_config(&self) -> Result<Option<Config>, String> {
        let path = self.layout.config_cache().join("active.json");
        if !path.exists() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("active config record is not a safe file".to_owned());
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let active: crate::config::ActivatedConfig =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        Ok(Some(active.config))
    }
}
