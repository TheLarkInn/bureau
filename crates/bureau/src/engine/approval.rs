//! Live approval-label admission at every step boundary.
//!
//! The check reads the run's own item. It never re-runs the assignment
//! filter: that filter admits new work, and its exclusions can match labels
//! the run itself adds, such as a report label.

use super::context::RunCtx;

async fn read(ctx: &RunCtx, label: &str) -> Result<crate::forge::Item, String> {
    let id = &ctx.plan.item.external_id;
    ctx.plan.forge.item(id).await.map_err(|error| {
        format!(
            "could not confirm approval label `{label}` on work item `{id}`; the forge read failed: {error}"
        )
    })
}

pub(super) async fn check(ctx: &RunCtx) -> Result<(), String> {
    let Some(label) = ctx.plan.assignment.work.approval_label.as_deref() else {
        return Ok(());
    };
    let item = tokio::time::timeout(ctx.remaining(), read(ctx, label))
        .await
        .map_err(|_| super::control::deadline_message(ctx))??;
    if item.labels.iter().any(|item_label| item_label == label) {
        Ok(())
    } else {
        Err(format!(
            "approval label `{label}` is missing; restore it, inspect the preserved evidence, then run `bureau retry {}`",
            ctx.plan.run_id
        ))
    }
}
