use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::Assignment;
use bureau::forge::Item;
use bureau::state::{FreshClaim, LeaseOwner, Store};

use crate::cli::{out, prepare};

#[cfg(test)]
mod tests;

fn accepted(claim: Option<FreshClaim>, assignment: &Assignment, item: &Item) -> bool {
    match claim {
        Some(FreshClaim::Claimed) => return true,
        Some(FreshClaim::Busy) => out::line(format_args!(
            "item `{}` is already claimed",
            item.external_id
        )),
        Some(FreshClaim::PreservedFactory(run)) => out::error(format_args!(
            "item `{}` is preserved by local factory run `{run}`; inspect or resume that run instead",
            item.external_id,
        )),
        None => out::error(format_args!(
            "assignment `{}` has exhausted its configured limits",
            assignment.name
        )),
    }
    false
}

pub(super) fn fresh(
    store: Arc<Store>,
    assignment: &Assignment,
    item: &Item,
    run_id: &str,
    runs_dir: &Path,
    open_prs: usize,
) -> anyhow::Result<Option<LeaseOwner>> {
    let owner = LeaseOwner::new(
        store,
        &assignment.name,
        prepare::forge_name(assignment.work.forge),
        &item.external_id,
        run_id,
    )
    .context("creating lease owner")?;
    let claim = owner
        .claim_fresh_with_limits(
            bureau::supervise::LEASE_TTL,
            runs_dir,
            &assignment.limits,
            open_prs,
        )
        .context("claiming work item")?;
    Ok(accepted(claim, assignment, item).then_some(owner))
}
