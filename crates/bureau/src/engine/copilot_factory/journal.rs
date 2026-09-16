//! Durable identity writes share the scheduler's transactional ownership fence.

use std::fs::OpenOptions;
use std::io;
use std::io::Write as _;
use std::sync::{Arc, Mutex};

use crate::process::{Secret, scrub_json};
use crate::runlog::copilot_factory::{Data, Record, Records};
use crate::runlog::{self, EventKind};
use crate::state::LeaseOwner;

use super::super::context::RunCtx;
use super::super::{RunPlan, stream};

pub(super) fn secrets(plan: &RunPlan) -> Vec<Secret> {
    let grants = &crate::adapters::real::FORGE_GRANTS;
    let mut secrets: Vec<_> = plan.credentials.values().cloned().collect();
    for role in plan.roles.values() {
        let found =
            crate::adapters::real::scoped_credentials(&role.permissions, grants, &["GH_TOKEN"]);
        secrets.extend(found.into_iter().map(|(_, value)| Secret::new(value)));
    }
    secrets
}

#[derive(Clone)]
pub(super) struct Journal {
    owner: LeaseOwner,
    log: stream::Shared,
    records: Arc<Mutex<Records>>,
    secrets: Vec<Secret>,
}

impl Journal {
    pub(super) fn new(ctx: &RunCtx) -> Result<Self, String> {
        let owner = ctx.plan.lease.clone().ok_or(
            "local Copilot factories require a scheduler lease; no unfenced factory start is allowed",
        )?;
        let directory = stream::lock(&ctx.log).dir().to_path_buf();
        let events = runlog::read_events_tolerant(&directory).map_err(|error| error.to_string())?;
        Ok(Self {
            owner,
            log: ctx.log.clone(),
            records: Arc::new(Mutex::new(
                Records::replay(&events).map_err(|error| error.to_string())?,
            )),
            secrets: secrets(&ctx.plan),
        })
    }

    pub(super) fn check(&self) -> io::Result<()> {
        self.owner
            .with_ownership(|| Ok(()))
            .map_err(io::Error::other)
    }

    pub(super) fn pause(&self, message: &str) -> io::Result<()> {
        let mut message = serde_json::Value::String(message.to_owned());
        scrub_json(&mut message, &self.secrets);
        let message = message
            .as_str()
            .ok_or_else(|| io::Error::other("invalid pause reason"))?;
        self.owner
            .with_ownership(|| {
                let directory = stream::lock(&self.log).dir().to_path_buf();
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(directory.join("PAUSE"))?;
                file.write_all(message.as_bytes())?;
                file.sync_all()?;
                std::fs::File::open(directory)?.sync_all()
            })
            .map_err(io::Error::other)
    }

    pub(super) fn append(&self, data: Data) -> io::Result<()> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| io::Error::other("factory journal poisoned"))?;
        let mut value = serde_json::to_value(data)?;
        scrub_json(&mut value, &self.secrets);
        let next = records.updated(serde_json::from_value(value.clone())?)?;
        self.owner
            .with_ownership(|| {
                stream::lock(&self.log)
                    .append(EventKind::CopilotFactory, value)
                    .map(|_| ())
            })
            .map_err(io::Error::other)?;
        *records = next;
        drop(records);
        Ok(())
    }

    pub(super) fn record(&self, session: &str) -> Result<Record, String> {
        let records = self
            .records
            .lock()
            .map_err(|_| "factory journal poisoned")?;
        records
            .0
            .get(session)
            .cloned()
            .ok_or_else(|| "factory intent is absent".to_owned())
    }

    pub(super) fn latest(&self, step: &str) -> Result<Option<Record>, String> {
        let records = self
            .records
            .lock()
            .map_err(|_| "factory journal poisoned")?;
        Ok(records
            .0
            .values()
            .filter(|record| record.intent.step == step)
            .max_by_key(|record| record.intent.step_attempt)
            .cloned())
    }
}
