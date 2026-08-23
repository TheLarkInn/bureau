//! The reconcile loop (DESIGN.md section 8) replaces the scheduler.

mod observe;
mod start;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::task::JoinHandle;

use crate::config::{Assignment, Config, ForgeKind, Repo};
use crate::engine::{Engine, RunOutcome, RunPlan, new_run_id};
use crate::forge::{Forge, Item, LabelForge};
use crate::process::Secret;
use crate::state::{LeaseOwner, Store};

use observe::Observed;

/// Reconcile-pass failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Durable-state failure.
    #[error(transparent)]
    State(#[from] crate::state::Error),
    /// Forge failure.
    #[error(transparent)]
    Forge(#[from] crate::forge::Error),
    /// Run identity generation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Label-rule observation, claim, audit, or mutation failure.
    #[error(transparent)]
    LabelRule(#[from] crate::label_reconcile::Error),
}

impl Error {
    const fn is_rate_limited(&self) -> bool {
        match self {
            Self::Forge(error) => error.is_rate_limited(),
            Self::LabelRule(error) => error.is_rate_limited(),
            Self::State(_) | Self::Io(_) => false,
        }
    }
}

/// A claimed, started run.
pub struct Started {
    /// The run id.
    pub run_id: String,
    /// The run's task; joining it yields the outcome.
    pub handle: JoinHandle<RunOutcome>,
    /// Fenced lease generation to release after forced task abortion.
    pub owner: Option<crate::state::LeaseOwner>,
}

/// The pass result: the started runs, or the first failure when nothing started.
fn settle(
    failed: Vec<Error>,
    started: Vec<Started>,
    labels_applied: usize,
) -> Result<Vec<Started>, Error> {
    match (
        failed.into_iter().next(),
        started.is_empty() && labels_applied == 0,
    ) {
        (Some(first), true) => Err(first),
        _ => Ok(started),
    }
}

fn pending<'a>(observed: &'a Observed<'a>) -> impl Iterator<Item = Item> + 'a {
    let prs = &observed.open_prs;
    let open: Vec<&str> = prs.iter().filter_map(|pr| pr.item_id.as_deref()).collect();
    let live = &observed.inflight;
    let leased: Vec<&str> = live.iter().map(|l| l.external_id.as_str()).collect();
    let excluded = move |item: &&Item| {
        let id = item.external_id.as_str();
        !open.contains(&id) && !leased.contains(&id)
    };
    observed.desired.iter().filter(excluded).cloned()
}

/// The lease's forge key: the work forge's lowercase name.
const fn forge_key(forge: ForgeKind) -> &'static str {
    match forge {
        ForgeKind::Ado => "ado",
        ForgeKind::Github => "github",
    }
}

/// The interval ± 25%, derived from the process clock's nanoseconds.
/// The clock read is the boundary: bound once as a function pointer so
/// this stays the single site naming the process clock.
fn jittered(interval: Duration) -> Duration {
    let now = SystemTime::now;
    let nanos = now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let base = interval.as_nanos();
    let spread = (base / 4).max(1);
    let shifted = base.saturating_sub(spread) + u128::from(nanos) % (2 * spread + 1);
    Duration::from_nanos(u64::try_from(shifted).unwrap_or(u64::MAX))
}

/// Sleeps a jittered interval, returning early on a wake; a closed channel degrades to plain sleeps.
async fn wait(interval: Duration, wake: &mut tokio::sync::mpsc::Receiver<()>) {
    let Ok(woken) = tokio::time::timeout(jittered(interval), wake.recv()).await else {
        return; // the jittered interval elapsed
    };
    if woken.is_none() {
        tokio::time::sleep(interval).await;
    }
}

/// Restarts one already-owned durable plan through the standard run wrapper.
#[must_use]
pub fn resume(engine: Arc<Engine>, state: Arc<Store>, plan: RunPlan) -> Started {
    start::spawn(engine, state, plan)
}

/// Applies approval-label admission and trust promotion before a claim.
#[must_use]
pub fn approved_item(assignment: &Assignment, mut item: Item) -> Option<Item> {
    let Some(label) = assignment.work.approval_label.as_deref() else {
        return Some(item);
    };
    if !item.labels.iter().any(|item_label| item_label == label) {
        return None;
    }
    item.trust = item.trust.max(crate::contract::Trust::Maintainer);
    Some(item)
}

/// Compares desired and observed state, closing the gap.
///
/// Drain semantics: an assignment removed from the config is never
/// claimed again; in-flight runs finish and release their leases.
pub struct Reconciler {
    /// The loaded runner configuration.
    pub config: Config,
    /// Leases, budget, and dedup.
    pub state: Arc<Store>,
    /// Forge client per assignment; credentials and ADO organizations differ.
    pub forges: BTreeMap<String, Arc<dyn Forge>>,
    /// GitHub issue clients per deterministic label rule.
    pub label_forges: BTreeMap<String, Arc<dyn LabelForge>>,
    /// The pipeline engine.
    pub engine: Arc<Engine>,
    /// Credentials keyed by registry credential name, resolved once at
    /// startup from the daemon's environment.
    pub credentials: BTreeMap<String, Secret>,
    /// Exact committed config revision for newly claimed runs.
    pub config_source: crate::runlog::ConfigSource,
    /// Pinned direct-agent bytes keyed by role name.
    pub direct_agents: BTreeMap<String, Vec<u8>>,
}

impl Reconciler {
    /// One reconcile pass over every assignment: observe, subtract, budget-check, claim, spawn.
    /// A failing assignment is skipped; the level-triggered loop retries it later.
    ///
    /// # Errors
    /// The first assignment's failure, but only when the pass started nothing.
    pub async fn reconcile_once(&self) -> Result<Vec<Started>, Error> {
        let labels =
            crate::label_reconcile::reconcile(&self.config, self.state.clone(), &self.label_forges)
                .await;
        if labels.rate_limited {
            let failed = labels.errors.into_iter().map(Error::LabelRule).collect();
            return settle(failed, Vec::new(), labels.applied);
        }
        let (observed, assignment_errors) = self.observe_all().await;
        let mut failed: Vec<Error> = labels.errors.into_iter().map(Error::LabelRule).collect();
        let assignments_limited = assignment_errors.iter().any(Error::is_rate_limited);
        failed.extend(assignment_errors);
        if assignments_limited {
            return settle(failed, Vec::new(), labels.applied);
        }
        let mut started = Vec::new();
        for assignment in &observed {
            self.claim_pending(assignment, &mut started, &mut failed);
        }
        settle(failed, started, labels.applied)
    }

    /// The daemon loop: reconcile, then sleep a jittered interval or
    /// wake early. Never returns; a failed pass is dropped, not fatal —
    /// the level-triggered loop retries it next pass. The failure stays
    /// observable to direct callers of [`Self::reconcile_once`], which
    /// the CLI daemon uses for its own reporting boundary.
    pub async fn run_loop(&self, interval: Duration, mut wake: tokio::sync::mpsc::Receiver<()>) {
        loop {
            let _pass = self.reconcile_once().await;
            wait(interval, &mut wake).await;
        }
    }

    /// Claims and spawns the pending items the budget allows.
    fn claim_pending(
        &self,
        observed: &Observed<'_>,
        started: &mut Vec<Started>,
        failed: &mut Vec<Error>,
    ) {
        for item in pending(observed).take(observed.headroom) {
            if let Err(error) = self.claim_one(observed, item, started) {
                failed.push(error);
                break;
            }
        }
    }

    /// Claims one item — CAS first, then dedup — and spawns its run.
    fn claim_one(
        &self,
        observed: &Observed<'_>,
        item: Item,
        started: &mut Vec<Started>,
    ) -> Result<(), Error> {
        let name = observed.assignment.name.as_str();
        let external_id = item.external_id.clone();
        let key = forge_key(observed.assignment.work.forge);
        let run_id = new_run_id(name)?;
        let owner = LeaseOwner::new(self.state.clone(), name, key, &external_id, &run_id)?;
        if !owner.claim(crate::supervise::LEASE_TTL)? {
            return Ok(()); // CAS lost; another daemon holds the item
        }
        self.start_claimed(observed, item, &run_id, owner, started)
    }

    fn start_claimed(
        &self,
        observed: &Observed<'_>,
        item: Item,
        run_id: &str,
        owner: LeaseOwner,
        started: &mut Vec<Started>,
    ) -> Result<(), Error> {
        if self.state.seen(&item.content_hash())? {
            owner.release()?;
            return Ok(());
        }
        match self.run_plan(observed, item, run_id) {
            Ok(mut plan) => {
                plan.lease = Some(owner);
                started.push(self.spawn(plan));
            }
            Err(()) => owner.release()?,
        }
        Ok(())
    }

    /// Assembles the run's plan; a dangling repo name skips the item (the caller releases its claim).
    /// Unresolvable credentials are left out: the engine escalates at push time instead.
    fn run_plan(&self, observed: &Observed<'_>, item: Item, run_id: &str) -> Result<RunPlan, ()> {
        let assignment = observed.assignment;
        let repos = self.registry_repos(assignment)?;
        let credentials = repos
            .values()
            .filter_map(|repo| {
                let secret = self.credentials.get(&repo.credential)?.clone();
                Some((repo.credential.clone(), secret))
            })
            .collect();
        Ok(RunPlan {
            run_id: run_id.to_owned(),
            assignment: assignment.clone(),
            pipeline: self.config.pipelines[assignment.pipeline.as_str()].clone(),
            roles: self.config.roles.clone(),
            repos,
            item,
            forge: observed.forge.clone(),
            credentials,
            config_source: Some(self.config_source.clone()),
            plugin_sources: BTreeMap::new(),
            direct_agents: self.direct_agents.clone(),
            lease: None,
        })
    }

    /// The registry entries for the assignment's repos.
    fn registry_repos(&self, assignment: &Assignment) -> Result<BTreeMap<String, Repo>, ()> {
        assignment
            .repos
            .iter()
            .map(|name| {
                let repo = self.config.repos.get(name).ok_or(())?;
                Ok((name.clone(), repo.clone()))
            })
            .collect()
    }

    /// Spawns the run task; the wrapper records cost, marks the item's
    /// content seen on every terminal outcome but `Failure`, and always
    /// releases the lease.
    fn spawn(&self, plan: RunPlan) -> Started {
        start::spawn(self.engine.clone(), self.state.clone(), plan)
    }
}
