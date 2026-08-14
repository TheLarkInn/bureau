//! The reconcile loop (DESIGN.md section 8) replaces the scheduler.
//!
//! It is level-triggered: every pass asks "does reality match intent?",
//! never "what just happened?". Pending work is a query, not stored
//! state: `pending = query(source, filter) − open PRs − live leases`.
//! A webhook or `bureau reconcile --now` only shortens the interval;
//! the loop is fully correct with every webhook unplugged.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::task::JoinHandle;

use crate::config::{Assignment, Config, ForgeKind, Repo};
use crate::engine::{Engine, RunOutcome, RunPlan, new_run_id};
use crate::forge::{Forge, Item, Pr};
use crate::process::Secret;
use crate::state::{Disposition, Lease, Store};

/// How long a claim lives before a crashed run's lease may be reclaimed.
const LEASE_TTL: Duration = Duration::from_secs(3600);

/// Reconcile-pass failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Durable-state failure.
    #[error(transparent)]
    State(#[from] crate::state::Error),
    /// Forge failure.
    #[error(transparent)]
    Forge(#[from] crate::forge::Error),
}

/// A claimed, started run.
pub struct Started {
    /// The run id.
    pub run_id: String,
    /// The run's task; joining it yields the outcome.
    pub handle: JoinHandle<RunOutcome>,
}

/// One assignment's observed world for a pass.
struct Observed<'a> {
    /// The assignment being reconciled.
    assignment: &'a Assignment,
    /// The forge its work items live on.
    forge: Arc<dyn Forge>,
    /// Work items matching the assignment's filter.
    desired: Vec<Item>,
    /// Open PRs on the primary repo carrying its branch prefix.
    open_prs: Vec<Pr>,
    /// The assignment's live leases.
    inflight: Vec<Lease>,
    /// How many more runs the assignment may start now.
    headroom: usize,
}

/// Compares desired and observed state, closing the gap.
///
/// Drain semantics: an assignment removed from the config is simply
/// never claimed again; in-flight runs finish and release their leases.
pub struct Reconciler {
    /// The loaded runner configuration.
    pub config: Config,
    /// Leases, budget, and dedup.
    pub state: Arc<Store>,
    /// Forge clients by kind — a vec, as `ForgeKind` is neither `Ord`
    /// nor `Hash`. Config forge ≠ work forge: each assignment names the
    /// forge its work items live on.
    pub forges: Vec<(ForgeKind, Arc<dyn Forge>)>,
    /// The pipeline engine.
    pub engine: Arc<Engine>,
    /// Credentials keyed by registry credential name, resolved once at
    /// startup from the daemon's environment.
    pub credentials: BTreeMap<String, Secret>,
}

impl Reconciler {
    /// One reconcile pass over every assignment: observe, subtract, budget-gate, claim, spawn.
    /// A failing assignment is skipped; the level-triggered loop retries it later.
    ///
    /// # Errors
    /// The first assignment's failure, but only when the pass started nothing.
    pub async fn reconcile_once(&self) -> Result<Vec<Started>, Error> {
        let (mut started, mut failed) = (Vec::new(), Vec::new());
        for (name, assignment) in &self.config.assignments {
            match self.observe(name, assignment).await {
                Ok(observed) => self.claim_pending(&observed, &mut started, &mut failed),
                Err(error) => failed.push(error),
            }
        }
        settle(failed, started)
    }

    /// The daemon loop: reconcile, then sleep a jittered interval or
    /// wake early. Never returns; a failed pass is logged, not fatal.
    pub async fn run_loop(&self, interval: Duration, mut wake: tokio::sync::mpsc::Receiver<()>) {
        loop {
            if let Err(error) = self.reconcile_once().await {
                eprintln!("reconcile pass failed: {error}");
            }
            wait(interval, &mut wake).await;
        }
    }

    /// Queries the assignment's forge and reads its budget; a failure fails this assignment's pass only.
    async fn observe<'a>(
        &'a self,
        name: &str,
        assignment: &'a Assignment,
    ) -> Result<Observed<'a>, Error> {
        let forge = self.work_forge(name, assignment)?;
        // PRs are observed by registry name, matching engine finalize.
        let repo = assignment
            .primary_repo()
            .ok_or_else(|| bad_assignment(name, "lists no repos"))?;
        let desired = forge
            .query(&assignment.work.source, &assignment.work.filter)
            .await?;
        let open_prs = forge.open_prs(repo, &assignment.branch_prefix).await?;
        let inflight = self.state.active(name)?;
        let headroom = self
            .state
            .headroom(name, &assignment.limits, open_prs.len())?;
        Ok(Observed {
            assignment,
            forge: forge.clone(),
            desired,
            open_prs,
            inflight,
            headroom,
        })
    }

    /// The forge the assignment's work items live on.
    fn work_forge(&self, name: &str, assignment: &Assignment) -> Result<&Arc<dyn Forge>, Error> {
        let kind = assignment.work.forge;
        let client = self
            .forges
            .iter()
            .find_map(|(k, v)| (*k == kind).then_some(v));
        client.ok_or_else(|| {
            bad_assignment(name, &format!("forge `{}` has no client", forge_key(kind)))
        })
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
        if !self.state.try_claim(name, key, &external_id, LEASE_TTL)? {
            return Ok(()); // CAS lost; another daemon holds the item
        }
        if self.state.seen(&item.content_hash())? {
            self.state.release(name, &external_id)?;
            return Ok(());
        }
        match self.run_plan(observed, item) {
            Ok(plan) => started.push(self.spawn(plan)),
            Err(()) => self.state.release(name, &external_id)?,
        }
        Ok(())
    }

    /// Assembles the run's plan; a dangling repo name skips the item (the caller releases its claim).
    /// Unresolvable credentials are left out: the engine escalates at push time instead.
    fn run_plan(&self, observed: &Observed<'_>, item: Item) -> Result<RunPlan, ()> {
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
            run_id: new_run_id(&assignment.name),
            assignment: assignment.clone(),
            pipeline: self.config.pipelines[assignment.pipeline.as_str()].clone(),
            roles: self.config.roles.clone(),
            repos,
            item,
            forge: observed.forge.clone(),
            credentials,
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

    /// Spawns the run task; the wrapper records cost, marks seen when a PR opened, and always releases the lease.
    fn spawn(&self, plan: RunPlan) -> Started {
        let run_id = plan.run_id.clone();
        let (engine, state) = (self.engine.clone(), self.state.clone());
        let (name, external_id) = (plan.assignment.name.clone(), plan.item.external_id.clone());
        let hash = plan.item.content_hash();
        let handle = tokio::spawn(async move {
            let outcome = engine.run(&plan).await;
            let _ = state.record_run(&name, outcome.cost_usd);
            if outcome.pr.is_some() {
                let _ = state.mark_seen(&hash, Disposition::Proposed);
            }
            let _ = state.release(&name, &external_id);
            outcome
        });
        Started { run_id, handle }
    }
}

/// Desired items with no open PR and no live lease (section 8).
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

/// An assignment-level config failure, as a forge-shaped error.
fn bad_assignment(name: &str, reason: &str) -> Error {
    let parse = crate::forge::Error::Parse(format!("assignment `{name}`: {reason}"));
    Error::Forge(parse)
}

/// The pass result: the started runs, or the first failure when nothing started.
fn settle(failed: Vec<Error>, started: Vec<Started>) -> Result<Vec<Started>, Error> {
    match (failed.into_iter().next(), started.is_empty()) {
        (Some(first), true) => Err(first),
        _ => Ok(started),
    }
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

/// The lease's forge key: the work forge's lowercase name.
const fn forge_key(forge: ForgeKind) -> &'static str {
    match forge {
        ForgeKind::Ado => "ado",
        ForgeKind::Github => "github",
    }
}

/// The interval ± 25%, derived from the system clock's nanoseconds.
fn jittered(interval: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let base = interval.as_nanos();
    let spread = (base / 4).max(1);
    let shifted = base.saturating_sub(spread) + u128::from(nanos) % (2 * spread + 1);
    Duration::from_nanos(u64::try_from(shifted).unwrap_or(u64::MAX))
}
