//! Config-refresh, recovery, projection, and reconcile orchestration.

use crate::cli::out;
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{ActivatedConfig, Config, ConfigManager, GitSource};
use bureau::engine::{Engine, rehydrate};
use bureau::forge::Forge;
use bureau::process::Secret;
use bureau::reconcile::Reconciler;
use bureau::runlog::{ConfigSource, RunSnapshot};
use bureau::state::{LeaseOwner, Store};

use super::active::Active;
use super::{ResolvedArgs, build};

struct Revision {
    config: Config,
    credentials: BTreeMap<String, Secret>,
    forges: BTreeMap<String, Arc<dyn Forge>>,
    source: ConfigSource,
    direct_agents: BTreeMap<String, Vec<u8>>,
}

fn revision(active: ActivatedConfig, settings: Option<&bureau::setup::Settings>) -> Revision {
    let credentials = build::credentials(&active.config, settings);
    let forges = build::forges(&active.config, &credentials);
    Revision {
        config: active.config,
        credentials,
        forges,
        source: ConfigSource {
            remote: active.remote,
            reference: active.reference,
            commit: active.commit,
        },
        direct_agents: active.direct_agents,
    }
}

pub(super) struct Daemon {
    manager: ConfigManager,
    state: Arc<Store>,
    engine: Arc<Engine>,
    active: Active,
    _maintenance: Option<bureau::maintenance::Guard>,
    settings: Option<bureau::setup::Settings>,
}

impl Daemon {
    pub(super) async fn pass(&mut self) -> anyhow::Result<()> {
        let active = self.refresh().await?;
        self.project_finished()?;
        self.resume_unfinished()?;
        let revision = revision(active, self.settings.as_ref());
        let reconciler = self.reconciler(&revision);
        let started = reconciler.reconcile_once().await?;
        self.active.extend(started);
        Ok(())
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
            engine: self.engine.clone(),
            credentials: revision.credentials.clone(),
            config_source: revision.source.clone(),
            direct_agents: revision.direct_agents.clone(),
        }
    }

    fn resume_unfinished(&mut self) -> anyhow::Result<()> {
        for snapshot in self.engine.unfinished()? {
            if self.active.contains(&snapshot.run_id) {
                continue;
            }
            if let Some(started) = self.resume_one(snapshot)? {
                self.active.extend(vec![started]);
            }
        }
        Ok(())
    }

    fn resume_one(
        &self,
        snapshot: RunSnapshot,
    ) -> anyhow::Result<Option<bureau::reconcile::Started>> {
        let owner = self.owner(&snapshot)?;
        if !owner.claim(bureau::supervise::LEASE_TTL)? {
            return Ok(None);
        }
        self.resume_owned(snapshot, owner)
    }

    fn resume_owned(
        &self,
        snapshot: RunSnapshot,
        owner: LeaseOwner,
    ) -> anyhow::Result<Option<bureau::reconcile::Started>> {
        let credentials =
            match build::credentials_for_repos(&snapshot.repos, self.settings.as_ref()) {
                Ok(credentials) => credentials,
                Err(error) => return self.block(&snapshot, &owner, &error.to_string()),
            };
        let forge = match build::forge(&snapshot.assignment, &snapshot.repos, &credentials) {
            Ok(forge) => forge,
            Err(error) => return self.block(&snapshot, &owner, &error.to_string()),
        };
        let mut plan = rehydrate(snapshot, forge, credentials);
        plan.lease = Some(owner);
        Ok(Some(bureau::reconcile::resume(
            self.engine.clone(),
            self.state.clone(),
            plan,
        )))
    }

    fn owner(&self, snapshot: &RunSnapshot) -> anyhow::Result<LeaseOwner> {
        let forge = match snapshot.assignment.work.forge {
            bureau::config::ForgeKind::Ado => "ado",
            bureau::config::ForgeKind::Github => "github",
        };
        Ok(LeaseOwner::new(
            self.state.clone(),
            &snapshot.assignment.name,
            forge,
            &snapshot.item.external_id,
            &snapshot.run_id,
        )?)
    }

    fn block(
        &self,
        snapshot: &RunSnapshot,
        owner: &LeaseOwner,
        message: &str,
    ) -> anyhow::Result<Option<bureau::reconcile::Started>> {
        let blocked = self.engine.block(snapshot, message);
        let projected =
            bureau::state::project_run(&self.state, &self.engine.runs_dir, &snapshot.run_id);
        let released = owner.release();
        blocked?;
        projected?;
        released?;
        Ok(None)
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
    let state = Arc::new(Store::open(&args.state).context("opening state database")?);
    let engine = Arc::new(Engine::new(args.runs.clone(), args.cache.clone()));
    Ok(Daemon {
        manager: ConfigManager::new(source),
        state,
        engine,
        active: Active::new(args.runs.clone()),
        _maintenance: maintenance,
        settings: args.settings.clone(),
    })
}
