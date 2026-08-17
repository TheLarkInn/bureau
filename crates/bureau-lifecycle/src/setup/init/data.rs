use super::{
    ConfigDraft, ConfigPullRequest, FlowError, InitFlow, InitOutcome, Merge, OutcomeSummary,
    ReconcilePass, ValidatedConfig,
};

impl InitFlow {
    pub(super) fn finish<E>(&mut self, outcomes: OutcomeSummary) -> Result<(), FlowError<E>> {
        self.progress.outcome = Some(InitOutcome {
            config: self.validated()?.clone(),
            outcomes,
        });
        Ok(())
    }

    pub(super) fn verify_validated<E>(
        &self,
        merged: &str,
        validated: &ValidatedConfig,
    ) -> Result<(), FlowError<E>> {
        if validated.source != self.request.settings.config {
            return Err(FlowError::ValidatedSourceMismatch);
        }
        if validated.commit == merged {
            return Ok(());
        }
        Err(FlowError::MergedCommitMismatch {
            merged: merged.to_owned(),
            validated: validated.commit.clone(),
        })
    }

    pub(super) fn draft<E>(&self) -> Result<&ConfigDraft, FlowError<E>> {
        self.progress
            .draft
            .as_ref()
            .ok_or(FlowError::MissingStateData("config draft"))
    }

    pub(super) fn pull_request<E>(&self) -> Result<&ConfigPullRequest, FlowError<E>> {
        self.progress
            .pull_request
            .as_ref()
            .ok_or(FlowError::MissingStateData("config pull request"))
    }

    pub(super) fn merged<E>(&self) -> Result<&Merge, FlowError<E>> {
        self.progress
            .merged
            .as_ref()
            .ok_or(FlowError::MissingStateData("merged config"))
    }

    pub(super) fn validated<E>(&self) -> Result<&ValidatedConfig, FlowError<E>> {
        self.progress
            .validated
            .as_ref()
            .ok_or(FlowError::MissingStateData("validated config"))
    }

    pub(super) fn pass<E>(&self) -> Result<&ReconcilePass, FlowError<E>> {
        self.progress
            .pass
            .as_ref()
            .ok_or(FlowError::MissingStateData("reconcile pass"))
    }
}
