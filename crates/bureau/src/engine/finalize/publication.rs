//! Typed gates immediately before external publication.

use super::super::machine::RunCtx;
use super::super::{approval, control, settle};
use crate::contract::StepOutcome;
use crate::forge::Pr;

pub(super) enum Error {
    Failure(String),
    Escalate(String),
}

pub(super) async fn check(ctx: &RunCtx) -> Result<(), Error> {
    if let Some(reason) = control::ownership_reason(ctx) {
        return Err(Error::Failure(reason));
    }
    if let Some(reason) = control::cancel_reason(ctx) {
        return Err(Error::Failure(reason));
    }
    if ctx.remaining().is_zero() {
        return Err(Error::Escalate(control::deadline_message(ctx)));
    }
    approval::check(ctx).await.map_err(Error::Escalate)
}

pub(super) async fn stop(ctx: &RunCtx, error: Error) -> (StepOutcome, String, Option<Pr>) {
    match error {
        Error::Failure(message) => (StepOutcome::Failure, message, None),
        Error::Escalate(message) => settle::escalate(ctx, message).await,
    }
}
