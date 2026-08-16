//! One claimed run task and its idempotent terminal projection.

use std::sync::Arc;

use super::Started;
use crate::engine::{Engine, RunPlan};
use crate::state::Store;

pub(super) fn spawn(engine: Arc<Engine>, state: Arc<Store>, plan: RunPlan) -> Started {
    let run_id = plan.run_id.clone();
    let owner = plan.lease.clone();
    let directory = crate::runlog::run_dir(&engine.runs_dir, &run_id);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return failed_start(run_id, plan, error);
    }
    let handle = tokio::spawn(async move {
        let (outcome, projection) = crate::supervise::run(engine, state, plan).await;
        if let Err(error) = projection {
            eprintln!(
                "run `{}` terminal projection failed: {error}",
                outcome.run_id
            );
        }
        outcome
    });
    Started {
        run_id,
        handle,
        owner,
    }
}

fn failed_start(run_id: String, plan: RunPlan, error: std::io::Error) -> Started {
    let task_run_id = run_id.clone();
    let handle = tokio::spawn(async move {
        if let Some(owner) = plan.lease {
            let _released = owner.release();
        }
        crate::engine::RunOutcome::bare(
            &task_run_id,
            crate::contract::StepOutcome::Failure,
            format!("creating run directory failed: {error}"),
        )
    });
    Started {
        run_id,
        handle,
        owner: None,
    }
}
