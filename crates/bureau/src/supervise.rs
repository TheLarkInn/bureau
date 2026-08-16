//! Shared lease renewal and terminal projection for every run entry point.

use std::sync::Arc;
use std::time::Duration;

use crate::engine::{Engine, RunOutcome, RunPlan};
use crate::state::{Error, Store};

/// How long run ownership remains live without a successful renewal.
pub const LEASE_TTL: Duration = Duration::from_secs(60 * 60);

pub async fn run(
    engine: Arc<Engine>,
    state: Arc<Store>,
    plan: RunPlan,
) -> (RunOutcome, Result<(), Error>) {
    let run_id = plan.run_id.clone();
    let Some(owner) = plan.lease.clone() else {
        return failed(&run_id, "claimed run has no lease owner", Ok(()));
    };
    let maintained = maintain(&engine, &plan, owner.clone()).await;
    finish(&state, &engine, &owner, &run_id, maintained)
}

async fn maintain(
    engine: &Engine,
    plan: &RunPlan,
    owner: crate::state::LeaseOwner,
) -> Option<RunOutcome> {
    let cancel = crate::runlog::run_dir(&engine.runs_dir, &plan.run_id).join("CANCEL");
    let future = engine.run(plan);
    crate::state::maintain_lease(owner, LEASE_TTL, &cancel, future).await
}

fn finish(
    state: &Store,
    engine: &Engine,
    owner: &crate::state::LeaseOwner,
    run_id: &str,
    maintained: Option<RunOutcome>,
) -> (RunOutcome, Result<(), Error>) {
    let Some(outcome) = maintained else {
        return failed(
            run_id,
            "run stopped after losing lease ownership",
            owner.release(),
        );
    };
    let projection = project(state, engine, owner, run_id);
    match projection {
        Ok(()) => (outcome, Ok(())),
        Err(error) => failed(
            run_id,
            &format!("terminal projection failed: {error}"),
            Err(error),
        ),
    }
}

fn failed(
    run_id: &str,
    message: &str,
    result: Result<(), Error>,
) -> (RunOutcome, Result<(), Error>) {
    let outcome = RunOutcome::bare(
        run_id,
        crate::contract::StepOutcome::Failure,
        message.to_owned(),
    );
    (outcome, result)
}

fn project(
    state: &Store,
    engine: &Engine,
    owner: &crate::state::LeaseOwner,
    run_id: &str,
) -> Result<(), Error> {
    let projected = match crate::state::project_run(state, &engine.runs_dir, run_id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::MissingTerminal(run_id.to_owned())),
        Err(error) => Err(error),
    };
    let released = owner.release();
    projected.and(released)
}
