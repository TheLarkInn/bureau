use super::{Error, Reconciler, Started};

/// Successful fresh-run and label-rule progress from one reconcile pass.
pub struct Pass {
    /// Newly claimed runs.
    pub started: Vec<Started>,
    /// Label mutations actually applied during this pass.
    pub labels_applied: usize,
}

fn settle(failed: Vec<Error>, started: Vec<Started>, labels_applied: usize) -> Result<Pass, Error> {
    match (
        failed.into_iter().next(),
        started.is_empty() && labels_applied == 0,
    ) {
        (Some(first), true) => Err(first),
        _ => Ok(Pass {
            started,
            labels_applied,
        }),
    }
}

impl Reconciler {
    /// Reconciles assignments and labels while retaining the amount of actual progress.
    ///
    /// # Errors
    /// Returns the first failure only when no run started and no label mutation was applied.
    pub async fn reconcile_pass(&self) -> Result<Pass, Error> {
        let labels =
            crate::label_reconcile::reconcile(&self.config, self.state.clone(), &self.label_forges)
                .await;
        if labels.rate_limited {
            let failed = labels.errors.into_iter().map(Error::LabelRule).collect();
            return settle(failed, Vec::new(), labels.applied);
        }
        let (observed, assignment_errors) = self.observe_all().await;
        let mut failed: Vec<Error> = labels.errors.into_iter().map(Error::LabelRule).collect();
        let assignments_limited = assignment_errors.iter().any(Error::is_rate_limited);
        failed.extend(assignment_errors);
        if assignments_limited {
            return settle(failed, Vec::new(), labels.applied);
        }
        let mut started = Vec::new();
        for assignment in &observed {
            self.claim_pending(assignment, &mut started, &mut failed);
        }
        settle(failed, started, labels.applied)
    }

    /// One reconcile pass over every assignment; a failing assignment is skipped.
    ///
    /// # Errors
    /// Returns the first failure only when no run started and no label mutation was applied.
    pub async fn reconcile_once(&self) -> Result<Vec<Started>, Error> {
        self.reconcile_pass().await.map(|pass| pass.started)
    }
}
