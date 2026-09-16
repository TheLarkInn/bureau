//! Pin authority and preserve the exact filesystem before any runtime code executes.

use std::os::unix::fs::MetadataExt as _;
use std::sync::Arc;

use crate::adapters::copilot_factory::artifacts::Artifacts;
use crate::adapters::copilot_factory::context_types::PinnedContext;
use crate::config::{CopilotFactory, Permission, Role, StepDef};
use crate::contract::StepRequest;
use crate::process::Secret;
use crate::runlog::copilot_factory::{Data, Intent, Record, Workspace};

use super::super::context::{RunCtx, WtCtx};
use super::super::stream;
use super::{authentication, context, credentials, journal::Journal};

pub(super) fn workspace(wt: &WtCtx) -> Result<Workspace, String> {
    let directory = std::fs::canonicalize(wt.worktree.path()).map_err(|error| error.to_string())?;
    let metadata = std::fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
    Ok(Workspace {
        directory,
        device: metadata.dev(),
        inode: metadata.ino(),
        mirror: std::fs::canonicalize(&wt.mirror).map_err(|error| error.to_string())?,
        branch: wt.branch.clone(),
        start_head: wt.start_head.clone(),
    })
}

pub(super) struct Prepared {
    pub(super) intent: Intent,
    pub(super) artifacts: Arc<Artifacts>,
    pub(super) recovered: Option<Record>,
    pub(super) role: Role,
    pub(super) model_token: Secret,
}

pub(super) fn role<'a>(ctx: &'a RunCtx, step: &StepDef) -> Result<&'a Role, String> {
    let role = step
        .role
        .as_ref()
        .and_then(|name| ctx.plan.roles.get(name))
        .ok_or("factory step has no configured role")?;
    if !role.permissions.contains(&Permission::ModelInvoke) {
        return Err(
            "local Copilot factories require the role's explicit model:invoke permission".into(),
        );
    }
    Ok(role)
}

fn existing(
    record: Record,
    wt: &WtCtx,
    request: &StepRequest,
    role: &Role,
    model_token: Secret,
) -> Result<Prepared, String> {
    let intent = &record.intent;
    if workspace(wt)? != intent.workspace || request != &intent.request {
        return Err(
            "factory workspace or step inputs changed; preserved state cannot be resumed".into(),
        );
    }
    context::restore(&intent.context)?;
    let artifacts = Artifacts::resume(
        &intent.factory,
        &request.worktree,
        &intent.paths.root,
        &intent.session_id,
    )?;
    if artifacts.identity != intent.identity || artifacts.paths != intent.paths {
        return Err(
            "factory executable or provider ownership differs from the durable intent".into(),
        );
    }
    Ok(Prepared {
        intent: intent.clone(),
        artifacts: Arc::new(artifacts),
        recovered: Some(record),
        role: role.clone(),
        model_token,
    })
}

fn pins(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
) -> Result<(String, Artifacts, PinnedContext), String> {
    let factory = step
        .copilot_factory
        .clone()
        .ok_or("factory configuration is absent")?;
    let session_id = format!(
        "bureau-{}",
        crate::identity::random_hex().map_err(|e| e.to_string())?
    );
    let run_dir = stream::lock(&ctx.log).dir().to_path_buf();
    let root = run_dir.join("copilot-factories").join(&session_id);
    let artifacts = Artifacts::pin(&factory, &request.worktree, &root, &session_id)?;
    authentication::create(&artifacts.paths.storage.copilot_home)?;
    let context = context::prepare(
        &ctx.plan,
        role(ctx, step)?,
        &request.worktree,
        &run_dir,
        &root.join("context"),
    )?;
    Ok((session_id, artifacts, context))
}

fn fresh(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
    model_token: Secret,
) -> Result<Prepared, String> {
    let (session_id, artifacts, context) = pins(ctx, step, request)?;
    let intent = Intent {
        step: step.name.clone(),
        step_attempt: ctx.attempts.get(&step.name).copied().unwrap_or(0),
        session_id,
        workspace: workspace(wt)?,
        paths: artifacts.paths.clone(),
        identity: artifacts.identity.clone(),
        factory: step
            .copilot_factory
            .clone()
            .ok_or("factory configuration is absent")?,
        request: request.clone(),
        context,
    };
    Ok(Prepared {
        intent,
        artifacts: Arc::new(artifacts),
        recovered: None,
        role: role(ctx, step)?.clone(),
        model_token,
    })
}

fn checked<'a>(
    ctx: &RunCtx,
    step: &'a StepDef,
    journal: &Journal,
) -> Result<(&'a CopilotFactory, Secret), String> {
    role(ctx, step)?;
    let errors = step.field_errors();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let factory = step
        .copilot_factory
        .as_ref()
        .ok_or("factory configuration is absent")?;
    let model_token = credentials::resolve(ctx, factory)?;
    journal.check().map_err(|error| error.to_string())?;
    Ok((factory, model_token))
}

pub(super) fn prepare(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
    journal: &Journal,
) -> Result<Prepared, String> {
    let (factory, model_token) = checked(ctx, step, journal)?;
    wt.worktree.retain();
    let prior = journal.latest(&step.name)?.filter(|record| {
        record.intent.step_attempt == ctx.attempts.get(&step.name).copied().unwrap_or(0)
    });
    if let Some(record) = prior {
        credentials::same_reference(&record.intent.factory, factory)?;
        return existing(record, wt, request, role(ctx, step)?, model_token);
    }
    let prepared = fresh(ctx, wt, step, request, model_token)?;
    journal
        .append(Data::Prepared {
            intent: Box::new(prepared.intent.clone()),
        })
        .map_err(|error| error.to_string())?;
    Ok(prepared)
}
