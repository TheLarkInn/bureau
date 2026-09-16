//! Config-refresh, recovery, projection, and reconcile orchestration.

use crate::cli::{factory_credentials, out};
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{ActivatedConfig, Config, ConfigManager, GitSource};
use bureau::engine::Engine;
use bureau::forge::{Forge, LabelForge};
use bureau::reconcile::Reconciler;
use bureau::runlog::ConfigSource;
use bureau::state::Store;

use super::active::Active;
use super::{ResolvedArgs, build};

mod deferred;
mod progress;
mod recovery;

#[cfg(test)]
mod tests;

struct Revision {
    config: Config,
    credentials: factory_credentials::Resolution,
    forges: BTreeMap<String, Arc<dyn Forge>>,
    label_forges: BTreeMap<String, Arc<dyn LabelForge>>,
    source: ConfigSource,
    direct_agents: BTreeMap<String, Vec<u8>>,
}

type RevisionBuilder = dyn Fn(ActivatedConfig, Option<&bureau::setup::Settings>) -> anyhow::Result<Revision>
    + Send
    + Sync;

fn revision(
    active: ActivatedConfig,
    settings: Option<&bureau::setup::Settings>,
) -> anyhow::Result<Revision> {
    let credentials = build::credentials(&active.config, settings)?;
    let forges = build::forges(&active.config, &credentials.values);
    let label_forges = build::label_forges(&active.config, &credentials.values);
    Ok(Revision {
        config: active.config,
        credentials,
        forges,
        label_forges,
        source: ConfigSource {
            remote: active.remote,
            reference: active.reference,
            commit: active.commit,
        },
        direct_agents: active.direct_agents,
    })
}

pub(super) struct Daemon {
    manager: ConfigManager,
    state: Arc<Store>,
    engine: Arc<Engine>,
    active: Active,
    _maintenance: Option<bureau::maintenance::Guard>,
    settings: Option<bureau::setup::Settings>,
    revision: Box<RevisionBuilder>,
    recovery_forge: Box<recovery::ForgeBuilder>,
}

impl Daemon {
    pub(super) async fn pass(&mut self) -> anyhow::Result<()> {
        let active = self.refresh().await?;
        self.project_finished()?;
        let deferred = self.resume_unfinished()?;
        let revision = (self.revision)(active, self.settings.as_ref())?;
        let reconciler = self.reconciler(&revision);
        let current = reconciler.reconcile_pass().await;
        self.complete_pass(current, deferred)
    }

    async fn refresh(&mut self) -> anyhow::Result<ActivatedConfig> {
        self.active.reap().await;
        let refresh = self.manager.refresh().await?;
        if let Some(warning) = refresh.warning {
            out::error(format_args!(
                "config refresh failed; using last-known-good: {warning}"
            ));
        }
        Ok(refresh.active)
    }

    pub(super) fn active_ids(&self) -> Vec<String> {
        self.active.ids()
    }

    fn project_finished(&self) -> anyhow::Result<()> {
        for record in self.engine.finished()? {
            bureau::state::project_terminal(&self.state, &record)?;
        }
        Ok(())
    }

    fn reconciler(&self, revision: &Revision) -> Reconciler {
        Reconciler {
            config: revision.config.clone(),
            state: self.state.clone(),
            forges: revision.forges.clone(),
            label_forges: revision.label_forges.clone(),
            engine: self.engine.clone(),
            credentials: revision.credentials.values.clone(),
            model_credential_errors: revision.credentials.errors.clone(),
            config_source: revision.source.clone(),
            direct_agents: revision.direct_agents.clone(),
        }
    }

    pub(super) async fn drain(self, signals: &mut super::active::Signals) {
        let _ = self.active.drain(signals).await;
    }
}

fn maintenance(args: &ResolvedArgs) -> anyhow::Result<Option<bureau::maintenance::Guard>> {
    if args.maintenance_guarded {
        Ok(None)
    } else {
        Ok(Some(bureau::maintenance::shared(&args.maintenance_root)?))
    }
}

/// Assembles the daemon from resolved arguments; a free constructor so
/// the state machine type carries no builder surface.
pub(super) fn new(args: &ResolvedArgs) -> anyhow::Result<Daemon> {
    let credential = build::config_credential(
        args.config_credential.as_deref(),
        args.config_forge,
        args.settings.as_ref(),
    )?;
    let maintenance = maintenance(args)?;
    let source = GitSource::new(
        args.config_remote.clone(),
        args.config_ref.clone(),
        args.config_subdir.clone(),
        &args.config_cache,
        credential,
    );
    Ok(Daemon {
        manager: ConfigManager::new(source),
        state: Arc::new(Store::open(&args.state).context("opening state database")?),
        engine: Arc::new(Engine::new(args.runs.clone(), args.cache.clone())),
        active: Active::new(args.runs.clone()),
        _maintenance: maintenance,
        settings: args.settings.clone(),
        revision: Box::new(revision),
        recovery_forge: Box::new(build::forge),
    })
}
