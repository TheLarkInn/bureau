//! Layer 4: the pipeline engine — a state machine over steps (DESIGN.md
//! section 7). Entry point: [`Engine::run`] with a [`RunPlan`].
//!
//! Alternating `deterministic` and `agent` steps is the entire value
//! proposition: the agent proposes, code verifies.
//!
//! Run lifecycle:
//!
//! 1. Setup: create the run directory and the worktree (`wt/`).
//! 2. Steps: follow explicit edges; a missing branch aborts (fails
//!    closed). Decision steps only route — no code, no retry budget, no
//!    events. Between steps, a `CANCEL` marker file in the run directory
//!    aborts the run (`bureau cancel <run-id>` writes it); a `PAUSE`
//!    marker instead exits the engine unfinished, resumable on re-entry.
//! 3. Terminals: `done` → `Success` (push + PR; `NoWork` when nothing
//!    changed), `abort` → `Failure`, `escalate` → `Blocked` plus a
//!    comment on the work item.
//!
//! Resume replays `events.jsonl` — the only source of truth for step
//! state — and skips completed steps. The log records outcomes, not
//! outputs, so a resumed run re-derives routing from outcomes, treats
//! earlier steps' outputs as empty, and rebuilds `wt/` fresh from the
//! mirror; steps pass state through outputs and `artifacts/`, never the
//! worktree. A finished run returns its recorded outcome without
//! appending anything (idempotent).

mod approval;
mod artifact;
mod checkpoint;
mod concurrent;
mod context;
mod control;
mod deadline;
mod drive;
mod edge;
mod execute;
mod finalize;
mod gitcmd;
mod machine;
mod plugins;
mod recovery;
mod request;
mod resume;
mod settle;
mod stream;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{Assignment, Pipeline, Repo, Role};
use crate::contract::StepOutcome;
use crate::forge::{Forge, Item, Pr};
use crate::git::CheckoutCache;
use crate::process::Secret;
use crate::runlog::{RunFinishedData, RunSnapshot};

pub use crate::state::TerminalRecord;

/// Everything one run needs. The item is already leased by the caller;
/// lease release on completion is also the caller's job (the reconcile
/// task wrapper releases on every unwind path).
#[derive(Clone)]
pub struct RunPlan {
    /// Run id; also the run directory name under `runs_dir`.
    pub run_id: String,
    /// The standing arrangement being executed.
    pub assignment: Assignment,
    /// The pipeline to run.
    pub pipeline: Pipeline,
    /// Roles by name (agent steps resolve through this).
    pub roles: BTreeMap<String, Role>,
    /// Repo registry entries for the assignment's repos.
    pub repos: BTreeMap<String, Repo>,
    /// The claimed work item.
    pub item: Item,
    /// The forge the item and PRs live on.
    pub forge: Arc<dyn Forge>,
    /// Credentials keyed by registry credential name, resolved pre-spawn.
    pub credentials: BTreeMap<String, Secret>,
    /// Exact committed config source for this run.
    pub config_source: Option<crate::runlog::ConfigSource>,
    /// Exact plugin snapshots keyed by plugin name.
    pub plugin_sources: BTreeMap<String, crate::runlog::PluginSource>,
    /// Pinned direct-agent bytes keyed by role name.
    pub direct_agents: BTreeMap<String, Vec<u8>>,
    /// Runtime-only fenced lease generation.
    pub lease: Option<crate::state::LeaseOwner>,
}

impl RunPlan {
    /// Serializable plan inputs; secret values and the forge client stay out.
    #[must_use]
    pub fn snapshot(&self) -> RunSnapshot {
        RunSnapshot {
            run_id: self.run_id.clone(),
            assignment: self.assignment.clone(),
            pipeline: self.pipeline.clone(),
            roles: self.roles.clone(),
            repos: self.repos.clone(),
            item: self.item.clone(),
            config_source: self.config_source.clone(),
            plugin_sources: self.plugin_sources.clone(),
            direct_agents: self.direct_agents.clone(),
        }
    }
}

/// Rehydrates a durable snapshot with current secret values and forge client.
#[must_use]
pub fn rehydrate(
    snapshot: RunSnapshot,
    forge: Arc<dyn Forge>,
    credentials: BTreeMap<String, Secret>,
) -> RunPlan {
    RunPlan {
        run_id: snapshot.run_id,
        assignment: snapshot.assignment,
        pipeline: snapshot.pipeline,
        roles: snapshot.roles,
        repos: snapshot.repos,
        item: snapshot.item,
        forge,
        credentials,
        config_source: snapshot.config_source,
        plugin_sources: snapshot.plugin_sources,
        direct_agents: snapshot.direct_agents,
        lease: None,
    }
}

/// How a run ended. Failures are data, not exceptions.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// The run id.
    pub run_id: String,
    /// Terminal outcome.
    pub outcome: StepOutcome,
    /// Total model cost of the run.
    pub cost_usd: f64,
    /// Human-readable summary (also the last run-log message).
    pub message: String,
    /// The PR opened on the `done` terminal, when one was.
    pub pr: Option<Pr>,
}

impl RunOutcome {
    /// An outcome with no PR and no recorded cost, for runs that never
    /// reached (or never re-entered) the machine.
    pub(crate) fn bare(run_id: &str, outcome: StepOutcome, message: String) -> Self {
        Self {
            run_id: run_id.to_owned(),
            outcome,
            cost_usd: 0.0,
            message,
            pr: None,
        }
    }

    fn finished(run_id: &str, data: RunFinishedData) -> Self {
        Self {
            run_id: run_id.to_owned(),
            outcome: data.outcome,
            cost_usd: data.cost_usd,
            message: data.message,
            pr: data.pr,
        }
    }
}

/// Awaits the machine's task; a panic inside it is data, not an unwind.
struct AbortTask(tokio::task::JoinHandle<RunOutcome>);

impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn joined(run_id: String, mut task: AbortTask) -> RunOutcome {
    match (&mut task.0).await {
        Ok(outcome) => outcome,
        Err(error) => RunOutcome::bare(
            &run_id,
            StepOutcome::Failure,
            format!("run task failed: {error}"),
        ),
    }
}

/// The engine: runs pipelines in worktrees, logging every event.
pub struct Engine {
    /// Where run directories live.
    pub runs_dir: PathBuf,
    /// The bare-mirror cache worktrees are cut from.
    pub cache: CheckoutCache,
}

impl Engine {
    /// An engine writing runs under `runs_dir` and mirrors under
    /// `cache_dir`.
    #[must_use]
    pub const fn new(runs_dir: PathBuf, cache_dir: PathBuf) -> Self {
        Self {
            runs_dir,
            cache: CheckoutCache::new(cache_dir),
        }
    }

    /// Runs a pipeline to a terminal. Never panics across the boundary:
    /// the machine runs in its own task and a join failure becomes a
    /// [`StepOutcome::Failure`] outcome.
    ///
    /// If `runs_dir/plan.run_id` already exists, the run resumes from its
    /// event log; a finished run returns its recorded outcome untouched.
    pub async fn run(&self, plan: &RunPlan) -> RunOutcome {
        let (runs_dir, cache, plan) = (self.runs_dir.clone(), self.cache.clone(), plan.clone());
        let run_id = plan.run_id.clone();
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            tokio::spawn(async move { drive::run(&runs_dir, &cache, &plan).await })
        }));
        match spawned {
            Ok(task) => joined(run_id, AbortTask(task)).await,
            Err(_) => RunOutcome::bare(
                &run_id,
                StepOutcome::Failure,
                "run task panicked before spawn".to_owned(),
            ),
        }
    }

    /// Immutable snapshots for started runs lacking a terminal event.
    ///
    /// # Errors
    /// Returns an error when a run log cannot be read or replayed.
    pub fn unfinished(&self) -> std::io::Result<Vec<RunSnapshot>> {
        recovery::unfinished(&self.runs_dir)
    }

    /// Terminal logs safe to idempotently project into `SQLite` on startup.
    ///
    /// # Errors
    /// Returns an error when a run log cannot be read or replayed.
    pub fn finished(&self) -> std::io::Result<Vec<TerminalRecord>> {
        recovery::finished(&self.runs_dir)
    }

    /// Marks an unfinished durable snapshot blocked without executing it.
    ///
    /// # Errors
    /// Propagates run-log open, append, and close failures.
    pub fn block(&self, snapshot: &RunSnapshot, message: &str) -> std::io::Result<()> {
        recovery::block(&self.runs_dir, snapshot, message)
    }
}

/// Wall-clock milliseconds for run-id uniqueness, bound once so this
/// module stays the single place naming the system clock.
fn wall_millis() -> u64 {
    use std::time::UNIX_EPOCH;
    let now = std::time::SystemTime::now;
    now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A filesystem-safe run id with operating-system random entropy.
///
/// # Errors
/// Fails when the operating system random source is unavailable.
pub fn new_run_id(assignment: &str) -> std::io::Result<String> {
    let millis = wall_millis();
    let safe: String = assignment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    Ok(format!(
        "{safe}-{millis}-{}",
        crate::identity::random_hex()?
    ))
}
