//! Layer 4: the pipeline engine (DESIGN.md section 7) — a state machine
//! over steps. Alternating `deterministic` and `agent` steps is the
//! entire value proposition: the agent proposes, code verifies.
//!
//! Host phases wrap the pipeline file: the engine creates the worktree
//! before the first step, and on the `done` terminal pushes the branch
//! and opens the PR (DESIGN.md section 11 steps 2 and 9 live here, not in
//! YAML — the schema has exactly two code-running step types).
//!
//! Terminal mapping: `done` → Success (push + PR), `abort` → Failure,
//! `escalate` → Blocked plus a comment on the work item. On resume the
//! engine replays `events.jsonl` and skips completed steps — the log is
//! the only source of truth.
//!
//! Between steps the engine checks for a `CANCEL` marker file in the run
//! directory (`bureau cancel <run-id>` writes it) and aborts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{Assignment, Pipeline, Repo, Role};
use crate::contract::StepOutcome;
use crate::forge::{Forge, Item, Pr};
use crate::git::CheckoutCache;
use crate::process::Secret;

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
    /// internal failures become [`StepOutcome::Failure`] outcomes.
    ///
    /// If `runs_dir/plan.run_id` already exists, the run resumes from its
    /// event log; completed steps are skipped.
    pub async fn run(&self, plan: &RunPlan) -> RunOutcome {
        let _ = plan;
        tokio::task::yield_now().await;
        todo!(
            "engine-core: create/resume run dir, cut worktree, drive the state machine, finalize on done, Worktree drop tears down"
        )
    }
}

/// A filesystem-safe run id: `{assignment}-{millis}-{pid}-{counter}`.
#[must_use]
pub fn new_run_id(assignment: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
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
    format!(
        "{safe}-{millis}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
