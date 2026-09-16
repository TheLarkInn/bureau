use bureau::reconcile::{Error, Pass};

use super::Daemon;
use super::deferred::Deferred;
use crate::cli::out;

fn idle(deferred: Vec<Deferred>, admission: Option<Error>) -> anyhow::Result<()> {
    match (deferred.into_iter().next(), admission) {
        (Some(error), _) => Err(error.into()),
        (None, Some(error)) => Err(error.into()),
        (None, None) => Ok(()),
    }
}

fn settle(
    active: bool,
    labels_applied: usize,
    deferred: Vec<Deferred>,
    admission: Option<Error>,
) -> anyhow::Result<()> {
    if active || labels_applied > 0 {
        if let Some(error) = admission {
            out::error(format_args!("{error}"));
        }
        return Ok(());
    }
    idle(deferred, admission)
}

impl Daemon {
    pub(super) fn complete_pass(
        &mut self,
        current: Result<Pass, Error>,
        deferred: Vec<Deferred>,
    ) -> anyhow::Result<()> {
        let labels_applied = match current {
            Ok(pass) => {
                self.active.extend(pass.started);
                pass.labels_applied
            }
            Err(error @ Error::ModelCredential(_)) => {
                return settle(!self.active.ids().is_empty(), 0, deferred, Some(error));
            }
            Err(error) => return Err(error.into()),
        };
        settle(
            !self.active.ids().is_empty(),
            labels_applied,
            deferred,
            None,
        )
    }
}
