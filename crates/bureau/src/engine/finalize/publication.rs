//! Typed gates immediately before external publication.

use super::super::context::{self, RunCtx};
use super::super::{approval, control};
use crate::contract::StepOutcome;
use crate::forge::Pr;

pub(super) enum Error {
    Failure(String),
    Escalate(String),
}

pub(super) async fn check(ctx: &RunCtx) -> Result<(), Error> {
    if let Some(reason) = context::ownership_reason(ctx) {
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

pub(super) fn stop(error: Error) -> (StepOutcome, String, Option<Pr>) {
    match error {
        Error::Failure(message) => (StepOutcome::Failure, message, None),
        Error::Escalate(message) => (StepOutcome::Blocked, message, None),
    }
}
