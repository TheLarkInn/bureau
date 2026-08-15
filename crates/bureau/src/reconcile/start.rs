//! One claimed run task and its idempotent terminal projection.

use std::sync::Arc;

use super::{LEASE_TTL, Started};
use crate::engine::{Engine, RunPlan};
use crate::state::{Store, maintain_lease};

pub(super) fn spawn(engine: Arc<Engine>, state: Arc<Store>, plan: RunPlan) -> Started {
    let run_id = plan.run_id.clone();
    let name = plan.assignment.name.clone();
    let external_id = plan.item.external_id.clone();
    let cancel = crate::runlog::run_dir(&engine.runs_dir, &run_id).join("CANCEL");
    let handle = tokio::spawn(async move {
        let future = engine.run(&plan);
        let outcome = maintain_lease(
            state.clone(),
            &name,
            &external_id,
            LEASE_TTL,
            &cancel,
            future,
        )
        .await;
        if !matches!(
            crate::state::project_run(&state, &engine.runs_dir, &outcome.run_id),
            Ok(true)
        ) {
            let _ = state.release(&name, &external_id);
        }
        outcome
    });
    Started { run_id, handle }
}
