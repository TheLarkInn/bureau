//! Live approval-label admission at every step boundary.

use super::machine::RunCtx;

pub(super) async fn check(ctx: &RunCtx) -> Result<(), String> {
    let Some(label) = ctx.plan.assignment.work.approval_label.as_deref() else {
        return Ok(());
    };
    let items = query(ctx, label).await?;
    let approved = items
        .iter()
        .find(|item| item.external_id == ctx.plan.item.external_id)
        .is_some_and(|item| item.labels.iter().any(|item_label| item_label == label));
    if approved {
        Ok(())
    } else {
        Err(format!(
            "approval label `{label}` is missing; restore it, inspect the preserved evidence, then run `bureau retry {}`",
            ctx.plan.run_id
        ))
    }
}

async fn query(ctx: &RunCtx, label: &str) -> Result<Vec<crate::forge::Item>, String> {
    let work = &ctx.plan.assignment.work;
    ctx.plan
        .forge
        .query(&work.source, &work.filter)
        .await
        .map_err(|error| {
            format!("could not confirm approval label `{label}`; the forge query failed: {error}")
        })
}
