//! Config-refresh, recovery, projection, and reconcile orchestration.

use crate::cli::out;
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{ActivatedConfig, Config, ConfigManager, GitSource};
use bureau::engine::{Engine, rehydrate};
use bureau::forge::{Forge, LabelForge};
use bureau::process::Secret;
use bureau::reconcile::Reconciler;
use bureau::runlog::{ConfigSource, RunSnapshot};
use bureau::state::{LeaseOwner, Store};

use super::active::Active;
use super::{ResolvedArgs, build};
use crate::cli::prepare;
use crate::cli::prepare::config_identity::ConfigRemote;

struct Revision {
    config: Config,
    credentials: BTreeMap<String, Secret>,
    identities: BTreeMap<String, String>,
    identity_forges: bureau::forge::identity::Authorizations,
    forges: BTreeMap<String, Arc<dyn Forge>>,
    label_forges: BTreeMap<String, Arc<dyn LabelForge>>,
    source: ConfigSource,
    direct_agents: BTreeMap<String, Vec<u8>>,
}

fn revision(active: ActivatedConfig, settings: Option<&bureau::setup::Settings>) -> Revision {
    let credentials = build::credentials(&active.config, settings);
    let forges = build::forges(&active.config, &credentials);
    let label_forges = build::label_forges(&active.config, &credentials);
    let identity_forges = prepare::authorizations(&active.config.repos, &credentials);
    Revision {
        config: active.config,
        credentials,
        identities: build::identities(settings),
        identity_forges,
        forges,
        label_forges,
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
            label_forges: revision.label_forges.clone(),
            engine: self.engine.clone(),
            credentials: revision.credentials.clone(),
            identities: revision.identities.clone(),
            identity_forges: revision.identity_forges.clone(),
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
        let identities = build::identities(self.settings.as_ref());
        let identity_forges = prepare::authorizations(&snapshot.repos, &credentials);
        let mut plan = rehydrate(snapshot, forge, credentials, identities, identity_forges);
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

/// The committed source the daemon refreshes from, signed with the
/// credential the caller already proved.
fn source(args: &ResolvedArgs, credential: Option<bureau::git::Credential>) -> GitSource {
    GitSource::new(
        args.config_remote.clone(),
        args.config_ref.clone(),
        args.config_subdir.clone(),
        &args.config_cache,
        credential,
    )
}

/// The config credential the daemon will fetch with, and the one host
/// authorized to answer for it.
fn config_remote(args: &ResolvedArgs) -> Option<ConfigRemote<'_>> {
    args.config_credential
        .as_deref()
        .map(|reference| ConfigRemote {
            reference,
            remote: &args.config_remote,
            forge: args.config_forge,
        })
}

/// Assembles the daemon from resolved arguments; a free constructor so
/// the state machine type carries no builder surface. The config
/// credential is resolved and proved here, once, before the first
/// refresh can fetch with it.
pub(super) async fn new(args: &ResolvedArgs) -> anyhow::Result<Daemon> {
    let maintenance = maintenance(args)?;
    let state = Arc::new(Store::open(&args.state).context("opening state database")?);
    let engine = Arc::new(Engine::new(args.runs.clone(), args.cache.clone()));
    let credential =
        build::config_credential(config_remote(args).as_ref(), args.settings.as_ref()).await?;
    Ok(Daemon {
        manager: ConfigManager::new(source(args, credential)),
        state,
        engine,
        active: Active::new(args.runs.clone()),
        _maintenance: maintenance,
        settings: args.settings.clone(),
    })
}
