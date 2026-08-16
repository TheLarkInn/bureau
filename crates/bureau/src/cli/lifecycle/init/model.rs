use std::collections::BTreeMap;

use bureau::adapters::AdapterKind;
use bureau::config::{Limits, Repo, WorkSource};
use bureau::setup::{FirstPipeline, InitRequest, Settings};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) settings: Settings,
    pub(super) repositories: BTreeMap<String, Repo>,
    pub(super) assignment: FirstAssignment,
    pub(super) first_pipeline: FirstPipeline,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FirstAssignment {
    pub(super) name: String,
    pub(super) work: WorkSource,
    pub(super) primary_repo: String,
    #[serde(default)]
    pub(super) context_repos: Vec<String>,
    pub(super) verify: String,
    pub(super) branch_prefix: String,
    pub(super) adapter: AdapterKind,
    #[serde(default)]
    pub(super) limits: Limits,
}

impl Request {
    pub(super) fn flow_request(&self) -> InitRequest {
        InitRequest {
            settings: self.settings.clone(),
            first_pipeline: self.first_pipeline.clone(),
        }
    }

    pub(super) fn pipeline_name(&self) -> String {
        format!("{}-pipeline", self.assignment.name)
    }

    pub(super) fn repository_names(&self) -> Vec<String> {
        let mut names = vec![self.assignment.primary_repo.clone()];
        names.extend(self.assignment.context_repos.clone());
        names
    }
}
