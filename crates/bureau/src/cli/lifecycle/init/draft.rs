use std::collections::BTreeMap;
use std::path::PathBuf;

use bureau::config::{Assignment, Permission, Pipeline, Repo, Role, StepDef, StepKind};
use bureau::contract::Trust;
use bureau::setup::ConfigDraft;
use serde::Serialize;

use super::model::Request;

fn yaml(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(serde_yaml_ng::to_string(value)?)
}

fn empty_step(name: &str, kind: StepKind) -> StepDef {
    StepDef {
        name: name.to_owned(),
        kind,
        run: None,
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

fn agent_step(name: &str, role: &str, next: &str, inputs: &[&str]) -> StepDef {
    let mut step = empty_step(name, StepKind::Agent);
    step.role = Some(role.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = Some("escalate".to_owned());
    step.on_blocked = Some("escalate".to_owned());
    step.on_no_work = Some("done".to_owned());
    step.inputs_from = inputs.iter().map(ToString::to_string).collect();
    step
}

fn verify_step(command: &str) -> StepDef {
    let mut step = empty_step("verify", StepKind::Deterministic);
    step.run = Some(command.to_owned());
    step.next = Some("review".to_owned());
    step.on_failure = Some("escalate".to_owned());
    step
}

fn pipeline(request: &Request) -> Pipeline {
    Pipeline {
        name: request.pipeline_name(),
        steps: vec![
            agent_step("implement", "implementer", "verify", &[]),
            verify_step(&request.assignment.verify),
            agent_step("review", "reviewer", "done", &["implement", "verify"]),
        ],
    }
}

fn repos(request: &Request) -> anyhow::Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Repositories<'a> {
        repos: &'a BTreeMap<String, Repo>,
    }
    Ok(yaml(&Repositories {
        repos: &request.repositories,
    })?
    .into_bytes())
}

fn implementer(request: &Request) -> Role {
    Role {
        name: "implementer".to_owned(),
        agent: "/bureau:implementer".to_owned(),
        adapter: request.assignment.adapter,
        permissions: vec![
            Permission::RepoRead,
            Permission::RepoWrite,
            Permission::ModelInvoke,
        ],
        min_trust: Trust::Maintainer,
    }
}

fn reviewer(request: &Request) -> Role {
    Role {
        name: "reviewer".to_owned(),
        agent: "/bureau:reviewer".to_owned(),
        adapter: request.assignment.adapter,
        permissions: vec![Permission::RepoRead, Permission::ModelInvoke],
        min_trust: Trust::Derived,
    }
}

fn assignment_config(request: &Request) -> Assignment {
    Assignment {
        name: request.assignment.name.clone(),
        work: request.assignment.work.clone(),
        repos: request.repository_names(),
        pipeline: request.pipeline_name(),
        role: "implementer".to_owned(),
        verify: request.assignment.verify.clone(),
        branch_prefix: request.assignment.branch_prefix.clone(),
        limits: request.assignment.limits.clone(),
    }
}

pub(super) fn complete(request: &Request, pipeline: Vec<u8>) -> anyhow::Result<ConfigDraft> {
    let assignment = format!("assignments/{}.yaml", request.assignment.name);
    let pipeline_path = format!("pipelines/{}.yaml", request.pipeline_name());
    let files = BTreeMap::from([
        (PathBuf::from("repos.yaml"), repos(request)?),
        (
            PathBuf::from("roles/implementer.yaml"),
            yaml(&implementer(request))?.into(),
        ),
        (
            PathBuf::from("roles/reviewer.yaml"),
            yaml(&reviewer(request))?.into(),
        ),
        (
            PathBuf::from(assignment),
            yaml(&assignment_config(request))?.into(),
        ),
        (PathBuf::from(pipeline_path), pipeline),
    ]);
    Ok(ConfigDraft { files })
}

pub(super) fn fixed(request: &Request) -> anyhow::Result<ConfigDraft> {
    complete(request, yaml(&pipeline(request))?.into_bytes())
}
