use std::collections::BTreeMap;
use std::sync::Arc;

use bureau::config::{Assignment, Repo};
use bureau::engine::rehydrate;
use bureau::forge::Forge;
use bureau::process::Secret;
use bureau::reconcile::Started;
use bureau::runlog::RunSnapshot;
use bureau::state::LeaseOwner;

use super::Daemon;
use super::deferred::Deferred;
use crate::cli::out;
use crate::cli::reconcile::build;

pub(super) type ForgeBuilder = dyn Fn(
        &Assignment,
        &BTreeMap<String, Repo>,
        &BTreeMap<String, Secret>,
    ) -> anyhow::Result<Arc<dyn Forge>>
    + Send
    + Sync;

pub(super) enum Recovered {
    Skipped,
    Started(Started),
    Deferred(Deferred),
}

impl Daemon {
    pub(super) fn owner(&self, snapshot: &RunSnapshot) -> anyhow::Result<LeaseOwner> {
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
    ) -> anyhow::Result<Recovered> {
        if snapshot.pipeline.factory_credential_refs().next().is_some() {
            owner.release()?;
            anyhow::bail!(
                "factory run {} remains preserved: {message}",
                snapshot.run_id
            );
        }
        let blocked = self.engine.block(snapshot, message);
        let projected =
            bureau::state::project_run(&self.state, &self.engine.runs_dir, &snapshot.run_id);
        let released = owner.release();
        blocked?;
        projected?;
        released?;
        Ok(Recovered::Skipped)
    }

    fn credential_failure(
        &self,
        snapshot: &RunSnapshot,
        owner: &LeaseOwner,
        error: &anyhow::Error,
    ) -> anyhow::Result<Recovered> {
        let Some(deferred) = Deferred::from_model_source(snapshot, error) else {
            return self.block(snapshot, owner, &error.to_string());
        };
        deferred.preserve(&self.engine.runs_dir, snapshot, owner)?;
        Ok(Recovered::Deferred(deferred))
    }

    pub(super) fn resume_owned(
        &self,
        snapshot: RunSnapshot,
        owner: LeaseOwner,
    ) -> anyhow::Result<Recovered> {
        let credentials = match build::credentials_for_snapshot(&snapshot, self.settings.as_ref()) {
            Ok(credentials) => credentials,
            Err(error) => return self.credential_failure(&snapshot, &owner, &error),
        };
        let forge = match (self.recovery_forge)(&snapshot.assignment, &snapshot.repos, &credentials)
        {
            Ok(forge) => forge,
            Err(error) => return self.block(&snapshot, &owner, &error.to_string()),
        };
        let mut plan = rehydrate(snapshot, forge, credentials);
        plan.lease = Some(owner);
        Ok(Recovered::Started(bureau::reconcile::resume(
            self.engine.clone(),
            self.state.clone(),
            plan,
        )))
    }

    fn resume_one(&self, snapshot: RunSnapshot) -> anyhow::Result<Recovered> {
        let owner = self.owner(&snapshot)?;
        if !owner.claim(bureau::supervise::LEASE_TTL)? {
            return Ok(Recovered::Skipped);
        }
        self.resume_owned(snapshot, owner)
    }

    fn resume_next(
        &mut self,
        snapshot: RunSnapshot,
        deferred: &mut Vec<Deferred>,
    ) -> anyhow::Result<()> {
        if self.active.contains(&snapshot.run_id) {
            return Ok(());
        }
        match self.resume_one(snapshot)? {
            Recovered::Skipped => {}
            Recovered::Started(started) => self.active.extend(vec![started]),
            Recovered::Deferred(error) => {
                out::error(format_args!("{error}"));
                deferred.push(error);
            }
        }
        Ok(())
    }

    pub(super) fn resume_unfinished(&mut self) -> anyhow::Result<Vec<Deferred>> {
        let mut deferred = Vec::new();
        for snapshot in self.engine.unfinished()? {
            self.resume_next(snapshot, &mut deferred)?;
        }
        Ok(deferred)
    }
}
