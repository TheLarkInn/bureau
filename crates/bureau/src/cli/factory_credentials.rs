//! Declared model-auth references; never legacy environment-name inference.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context as _;
use bureau::config::{Assignment, Config, Pipeline};
use bureau::process::Secret;
use bureau::setup::Settings;

#[cfg(test)]
mod tests;

#[derive(Debug, Default)]
pub(super) struct Resolution {
    pub(super) values: BTreeMap<String, Secret>,
    pub(super) errors: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(super) struct ModelSourceError(#[from] bureau::credential::Error);

impl ModelSourceError {
    pub(super) fn unavailable_reference(&self) -> Option<&str> {
        match &self.0 {
            bureau::credential::Error::Unavailable(reference) => Some(reference),
            bureau::credential::Error::Undeclared(_) | bureau::credential::Error::Unsafe(_) => None,
        }
    }
}

fn resolve_reference(reference: &str, settings: Option<&Settings>) -> anyhow::Result<Secret> {
    let settings = settings.context(
        "local factory model credentials require explicit declarations in settings.yaml",
    )?;
    Ok(bureau::credential::resolve(settings, reference).map_err(ModelSourceError::from)?)
}

fn resolve(
    references: &BTreeSet<&str>,
    settings: Option<&Settings>,
) -> anyhow::Result<BTreeMap<String, Secret>> {
    references
        .iter()
        .map(|reference| {
            let secret = resolve_reference(reference, settings)?;
            Ok(((*reference).to_owned(), secret))
        })
        .collect()
}

fn resolve_available(references: BTreeSet<&str>, settings: Option<&Settings>) -> Resolution {
    let mut resolved = Resolution::default();
    for reference in references {
        match resolve_reference(reference, settings) {
            Ok(secret) => {
                resolved.values.insert(reference.to_owned(), secret);
            }
            Err(error) => {
                resolved
                    .errors
                    .insert(reference.to_owned(), error.to_string());
            }
        }
    }
    resolved
}

fn pipeline<'a>(config: &'a Config, assignment: &Assignment) -> anyhow::Result<&'a Pipeline> {
    config.pipelines.get(&assignment.pipeline).with_context(|| {
        format!(
            "assignment `{}` has no pipeline `{}`",
            assignment.name, assignment.pipeline
        )
    })
}

pub(super) fn for_pipeline(
    pipeline: &Pipeline,
    settings: Option<&Settings>,
) -> anyhow::Result<BTreeMap<String, Secret>> {
    resolve(&pipeline.factory_credential_refs().collect(), settings)
}

pub(super) fn for_assignment(
    config: &Config,
    assignment: &Assignment,
    settings: Option<&Settings>,
) -> anyhow::Result<BTreeMap<String, Secret>> {
    for_pipeline(pipeline(config, assignment)?, settings)
}

fn report_errors(assignment: &str, pipeline: &Pipeline, errors: &BTreeMap<String, String>) {
    for reference in pipeline.factory_credential_refs() {
        if let Some(error) = errors.get(reference) {
            crate::cli::out::error(format_args!(
                "assignment `{assignment}` blocked: local factory model credential \
                 `{reference}` is unavailable: {error}"
            ));
        }
    }
}

/// Retains failed references and reports every affected assignment without stopping independent work.
pub(super) fn for_assignments(
    config: &Config,
    settings: Option<&Settings>,
) -> anyhow::Result<Resolution> {
    let mut references = BTreeSet::new();
    for assignment in config.assignments.values() {
        references.extend(pipeline(config, assignment)?.factory_credential_refs());
    }
    let resolved = resolve_available(references, settings);
    for assignment in config.assignments.values() {
        report_errors(
            &assignment.name,
            pipeline(config, assignment)?,
            &resolved.errors,
        );
    }
    Ok(resolved)
}
