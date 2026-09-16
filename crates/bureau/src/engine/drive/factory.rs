use super::super::context::{RunCtx, WtCtx};
use crate::runlog::copilot_factory::Records;

pub(super) fn configured(plan: &super::super::RunPlan) -> bool {
    plan.pipeline
        .steps
        .iter()
        .any(|step| step.copilot_factory.is_some())
}

fn owner_matches(plan: &super::super::RunPlan, owner: &crate::state::LeaseOwner) -> bool {
    let forge = match plan.assignment.work.forge {
        crate::config::ForgeKind::Ado => "ado",
        crate::config::ForgeKind::Github => "github",
    };
    (
        owner.assignment(),
        owner.forge(),
        owner.external_id(),
        owner.run_id(),
    ) == (
        plan.assignment.name.as_str(),
        forge,
        plan.item.external_id.as_str(),
        plan.run_id.as_str(),
    )
}

pub(super) fn owned<T>(
    plan: &super::super::RunPlan,
    action: impl FnOnce() -> std::io::Result<T>,
) -> Result<T, String> {
    if !configured(plan) {
        return action().map_err(|error| error.to_string());
    }
    let owner = plan
        .lease
        .as_ref()
        .ok_or("local Copilot factories require a scheduler lease")?;
    if !owner_matches(plan, owner) {
        return Err(
            "factory lease does not belong to the reviewed assignment and work item".into(),
        );
    }
    owner
        .with_ownership(action)
        .map_err(|error| error.to_string())
}

pub(super) fn preserve_failure(ctx: &RunCtx, message: &str) -> bool {
    let prior = super::super::copilot_factory::recovery::records(ctx);
    if prior.is_ok_and(|records| records.0.is_empty()) {
        return false;
    }
    super::super::copilot_factory::preserve(ctx, message);
    true
}

fn settled(directory: &std::path::Path) -> Result<bool, String> {
    let events =
        crate::runlog::read_events_tolerant(directory).map_err(|error| error.to_string())?;
    let records = Records::replay(&events).map_err(|error| error.to_string())?;
    Ok(events
        .iter()
        .any(|event| event.kind == crate::runlog::EventKind::RunFinished)
        && records
            .0
            .values()
            .all(crate::runlog::copilot_factory::Record::can_clean))
}

pub(super) fn cleanup(directory: &std::path::Path, wt: &WtCtx) {
    if !wt.worktree.is_retained() {
        return;
    }
    if settled(directory) == Ok(true) {
        wt.worktree.allow_cleanup();
    } else {
        wt.worktree.retain();
    }
}
