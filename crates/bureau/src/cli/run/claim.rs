use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::Assignment;
use bureau::forge::Item;
use bureau::state::{FreshClaim, LeaseOwner, Store};

use crate::cli::{out, prepare};

fn accepted(claim: FreshClaim, item: &Item) -> bool {
    match claim {
        FreshClaim::Claimed => return true,
        FreshClaim::Busy => out::line(format_args!(
            "item `{}` is already claimed",
            item.external_id
        )),
        FreshClaim::PreservedFactory(run) => out::error(format_args!(
            "item `{}` is preserved by local factory run `{run}`; inspect or resume that run instead",
            item.external_id,
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
        .claim_fresh(bureau::supervise::LEASE_TTL, runs_dir)
        .context("claiming work item")?;
    Ok(accepted(claim, item).then_some(owner))
}
