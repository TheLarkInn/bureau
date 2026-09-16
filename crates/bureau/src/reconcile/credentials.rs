use std::collections::BTreeMap;

use crate::config::{Assignment, Pipeline, Repo};
use crate::process::Secret;

use super::{Error, Reconciler};

impl Reconciler {
    fn check_model_reference(&self, reference: &str) -> Result<(), Error> {
        if self.model_credential_errors.contains_key(reference)
            || !self.credentials.contains_key(reference)
        {
            return Err(Error::ModelCredential(reference.to_owned()));
        }
        Ok(())
    }

    pub(super) fn check_model_credentials(&self, assignment: &Assignment) -> Result<(), Error> {
        let pipeline = self
            .config
            .pipelines
            .get(&assignment.pipeline)
            .ok_or_else(|| {
                crate::forge::Error::Parse(format!(
                    "assignment `{}` has no pipeline `{}`",
                    assignment.name, assignment.pipeline
                ))
            })?;
        pipeline
            .factory_credential_refs()
            .try_for_each(|reference| self.check_model_reference(reference))
    }

    pub(super) fn plan_credentials(
        &self,
        repos: &BTreeMap<String, Repo>,
        pipeline: &Pipeline,
    ) -> BTreeMap<String, Secret> {
        repos
            .values()
            .map(|repo| repo.credential.as_str())
            .chain(pipeline.factory_credential_refs())
            .filter_map(|reference| {
                self.credentials
                    .get(reference)
                    .map(|secret| (reference.to_owned(), secret.clone()))
            })
            .collect()
    }
}
