//! Factory recovery precedes ordinary worktree recreation and plugin activation.

mod workspace;

use std::path::Path;

use crate::git::Worktree;
use crate::runlog;
use crate::runlog::copilot_factory::{Record, Records, Workspace};

use super::super::context::{RunCtx, WtCtx};
use super::super::stream;
use super::prepare;

pub(in crate::engine) fn records(ctx: &RunCtx) -> Result<Records, String> {
    let directory = stream::lock(&ctx.log).dir().to_path_buf();
    let events = runlog::read_events_tolerant(&directory).map_err(|error| error.to_string())?;
    Records::replay(&events).map_err(|error| error.to_string())
}

fn same_workspace(records: &Records) -> Result<Option<Workspace>, String> {
    let mut identities = records.0.values().map(|record| &record.intent.workspace);
    let Some(first) = identities.next() else {
        return Ok(None);
    };
    if identities.any(|workspace| workspace != first) {
        return Err("factory intents disagree about the preserved workspace".into());
    }
    Ok(Some(first.clone()))
}

fn directory(ctx: &RunCtx, expected: &Workspace) -> Result<(), String> {
    let owned = stream::lock(&ctx.log).dir().join("wt");
    let actual = std::fs::canonicalize(owned).map_err(|error| error.to_string())?;
    if actual != expected.directory {
        return Err("factory workspace belongs to a different durable run directory".into());
    }
    Ok(())
}

pub(in crate::engine) async fn worktree(ctx: &RunCtx) -> Result<Option<WtCtx>, String> {
    let Some(expected) = same_workspace(&records(ctx)?)? else {
        return Ok(None);
    };
    directory(ctx, &expected)?;
    let worktree = Worktree::resume(&expected.mirror, &expected.directory, &expected.branch)
        .await
        .map_err(|error| format!("factory workspace is missing or changed: {error}"))?;
    let wt = WtCtx {
        worktree,
        mirror: expected.mirror.clone(),
        branch: expected.branch.clone(),
        start_head: expected.start_head.clone(),
    };
    if prepare::workspace(&wt)? != expected {
        return Err("factory workspace was replaced; refusing journal replay or cleanup".into());
    }
    Ok(Some(wt))
}

pub(in crate::engine) fn unfinished(ctx: &RunCtx, step: &crate::config::StepDef) -> bool {
    step.copilot_factory.is_some() && ctx.pending_step.as_deref() == Some(&step.name)
}

pub(in crate::engine) fn pending(ctx: &RunCtx) -> bool {
    ctx.pending_step
        .as_deref()
        .is_some_and(|step| ctx.factory_step(step))
}

fn plain_file(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!("factory runtime database is unavailable; do not recreate it: {error}")
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "preserved runtime state {} is empty or not a plain file",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn runtime_state(record: &Record) -> Result<(), String> {
    let database = record.intent.paths.storage.session.join("session.db");
    plain_file(&database)?;
    workspace::verify(record)?;
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| {
                format!("preserved factory database cannot be opened read-only: {error}")
            })?;
    let check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| format!("checking preserved runtime database: {error}"))?;
    if check != "ok" {
        return Err("preserved runtime database is corrupt; initialization was refused".into());
    }
    Ok(())
}
