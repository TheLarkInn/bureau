//! The reconcile loop (DESIGN.md section 8) replaces the scheduler.
//!
//! It is level-triggered: every pass asks "does reality match intent?",
//! never "what just happened?". Pending work is a query, not stored
//! state:
//!
//! ```text
//! pending = query(work_source, filter) − has_open_pr − has_active_lease
//! ```
//!
//! A webhook or `bureau reconcile --now` only shortens the interval; the
//! loop is fully correct with every webhook unplugged. Removing an
//! assignment from config drains it: no new claims, in-flight runs
//! finish.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::config::{Config, ForgeKind};
use crate::engine::{Engine, RunOutcome};

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

/// Compares desired and observed state, closing the gap.
pub struct Reconciler {
    /// The loaded runner configuration.
    pub config: Config,
    /// Leases, budget, and dedup.
    pub state: Arc<crate::state::Store>,
    /// Forge clients by kind. Config forge ≠ work forge: each assignment
    /// names the forge its work items live on.
    pub forges: BTreeMap<ForgeKind, Arc<dyn crate::forge::Forge>>,
    /// The pipeline engine.
    pub engine: Arc<Engine>,
    /// Credentials keyed by registry credential name, resolved once at
    /// startup from the daemon's environment.
    pub credentials: BTreeMap<String, crate::process::Secret>,
}

impl Reconciler {
    /// One reconcile pass over every assignment: observe, subtract,
    /// budget-gate, claim, spawn. Returns the runs started this pass.
    ///
    /// A panic inside a run is isolated to its task; the loop logs it and
    /// the lease release happens in the task's unwind path.
    ///
    /// # Errors
    /// Propagates forge query and state failures for the pass itself.
    pub async fn reconcile_once(&self) -> Result<Vec<Started>, Error> {
        tokio::task::yield_now().await;
        todo!(
            "reconcile-loop: per assignment — query, open_prs, active leases, subtract, headroom, take, try_claim, dedup, spawn engine run"
        )
    }

    /// The daemon loop: reconcile, then sleep a jittered interval or wake
    /// early. Never returns under normal operation.
    ///
    /// # Errors
    /// Returns only when a reconcile pass fails irrecoverably.
    pub async fn run_loop(
        &self,
        _interval: Duration,
        _wake: tokio::sync::mpsc::Receiver<()>,
    ) -> Result<(), Error> {
        tokio::task::yield_now().await;
        todo!("reconcile-loop: repeatedly reconcile_once, then select on wake vs jittered sleep")
    }
}
