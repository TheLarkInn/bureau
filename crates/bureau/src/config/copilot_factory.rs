//! Explicit repository-local Copilot factory selection and native limit overrides.

mod object;
mod runtime;
mod validation;

pub use runtime::{CopilotFactoryProfile, CopilotFactoryRuntime};

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

const MAX_CONCURRENT_SUBAGENTS: u32 = 500;

fn limit_override<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Fields of independently optional native limits, decoded through `CopilotFactoryLimits`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename = "CopilotFactoryLimits")]
pub struct CopilotFactoryLimitsFields {
    /// SDK direct agent concurrency, at most 500; omitted retains the factory's limit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "limit_override"
    )]
    pub max_concurrent_subagents: Option<u32>,
    /// Maximum total factory subagents; omitted retains the factory's limit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "limit_override"
    )]
    pub max_total_subagents: Option<u32>,
    /// Maximum accumulated active-execution seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "limit_override"
    )]
    pub timeout_seconds: Option<f64>,
    /// Maximum native AI credits, not a dollar-denominated cost.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "limit_override"
    )]
    pub max_ai_credits: Option<f64>,
}

impl CopilotFactoryLimitsFields {
    fn check_integers(&self, errors: &mut Vec<String>) {
        let limits = [
            ("max_concurrent_subagents", self.max_concurrent_subagents),
            ("max_total_subagents", self.max_total_subagents),
        ];
        for (field, value) in limits {
            if value == Some(0) {
                errors.push(format!("`copilot_factory.limits.{field}` must be positive"));
            }
        }
    }

    fn check_numbers(&self, errors: &mut Vec<String>) {
        let limits = [
            ("timeout_seconds", self.timeout_seconds),
            ("max_ai_credits", self.max_ai_credits),
        ];
        for (field, value) in limits {
            if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                errors.push(format!(
                    "`copilot_factory.limits.{field}` must be positive and finite"
                ));
            }
        }
    }

    fn check_concurrency(&self, errors: &mut Vec<String>) {
        if self
            .max_concurrent_subagents
            .is_some_and(|value| value > MAX_CONCURRENT_SUBAGENTS)
        {
            errors.push(format!(
                "`copilot_factory.limits.max_concurrent_subagents` must not exceed {MAX_CONCURRENT_SUBAGENTS}"
            ));
        }
    }

    fn check(&self, errors: &mut Vec<String>) {
        self.check_integers(errors);
        self.check_numbers(errors);
        self.check_concurrency(errors);
    }
}

/// Independently optional native limits with strict map-only decoding.
pub type CopilotFactoryLimits = object::Object<CopilotFactoryLimitsFields>;

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_extension(extension: &str) -> bool {
    extension
        .strip_prefix("project:")
        .is_some_and(|directory| directory.split('.').all(valid_name))
}

fn default_metadata() -> PathBuf {
    PathBuf::from("factory.json")
}

/// Fields of an opt-in factory invocation, decoded through `CopilotFactory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename = "CopilotFactory")]
pub struct CopilotFactoryFields {
    /// Registered factory name: ASCII alphanumerics, hyphens, and underscores.
    pub name: String,
    /// Repository-local extension source ID: `project:<directory>`.
    pub extension: String,
    /// Reviewed extension tree digest in canonical `tree-sha256:<hex>` form.
    pub extension_digest: String,
    /// Relative path to the extension's standard SDK `FactoryMeta` JSON.
    #[serde(default = "default_metadata")]
    pub metadata: PathBuf,
    /// Exact operator-qualified, preprovisioned runtime and SDK artifact.
    pub runtime: CopilotFactoryRuntime,
    /// Declared credential reference authorized for Copilot model access, never its value.
    pub model_credential: String,
    /// Raw argument object or null; omitted and explicit null are equivalent.
    #[serde(default)]
    pub args: Value,
    /// Native limit overrides; omission leaves the factory's limits unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "limit_override"
    )]
    pub limits: Option<CopilotFactoryLimits>,
}

impl CopilotFactoryFields {
    fn check_name(&self, errors: &mut Vec<String>) {
        if !valid_name(&self.name) {
            errors.push(
                "`copilot_factory.name` must be nonempty and use only ASCII alphanumerics, hyphens, or underscores"
                    .to_owned(),
            );
        }
    }

    fn check_extension(&self, errors: &mut Vec<String>) {
        if !valid_extension(&self.extension) {
            errors.push(
                "`copilot_factory.extension` must be `project:<single safe directory name>` without paths, `..`, or empty segments"
                    .to_owned(),
            );
        }
    }

    fn check_model_credential(&self, errors: &mut Vec<String>) {
        if !valid_name(&self.model_credential) {
            errors.push(
                "`copilot_factory.model_credential` must be a nonempty credential reference using only ASCII alphanumerics, hyphens, or underscores"
                    .to_owned(),
            );
        }
    }

    fn check_args(&self, errors: &mut Vec<String>) {
        if !(self.args.is_object() || self.args.is_null()) {
            errors.push(
                "`copilot_factory.args` must be a raw object or null, not a JSON-encoded string or interpolation"
                    .to_owned(),
            );
        }
    }

    fn check_artifacts(&self, errors: &mut Vec<String>) {
        validation::check_digest(
            &self.extension_digest,
            "copilot_factory.extension_digest",
            errors,
        );
        validation::check_relative(&self.metadata, "copilot_factory.metadata", false, errors);
        self.runtime.check(errors);
    }

    pub(super) fn check(&self, errors: &mut Vec<String>) {
        self.check_name(errors);
        self.check_extension(errors);
        self.check_model_credential(errors);
        self.check_args(errors);
        self.check_artifacts(errors);
        if let Some(limits) = &self.limits {
            limits.check(errors);
        }
    }
}

/// Opt-in factory invocation with strict map-only decoding.
pub type CopilotFactory = object::Object<CopilotFactoryFields>;
