//! Strict identity replay; notifications and caches never establish admission.

use std::io;
use std::num::NonZeroU64;

use super::{Data, Intent, Operation, Record, Records, RuntimePurpose, transitions};
use crate::adapters::copilot_factory::types::{
    FactoryRunResult, FactoryRunStatus, FactoryRunSummary,
};
use crate::runlog::{Event, EventKind};

impl Record {
    fn accept(&mut self, run_id: String, attempt: NonZeroU64) -> Result<(), String> {
        if self.run_id.as_ref().is_some_and(|known| known != &run_id) {
            return Err("local factory acceptance changed its durable run ID".to_owned());
        }
        if self.attempt.is_some_and(|previous| attempt < previous) {
            return Err("local factory acceptance regressed its native attempt".to_owned());
        }
        self.run_id = Some(run_id);
        self.attempt = Some(attempt);
        self.rejected = None;
        self.pending_admission = None;
        self.run = None;
        self.summary = None;
        Ok(())
    }

    fn observe(&mut self, run: FactoryRunResult, summary: FactoryRunSummary) -> Result<(), String> {
        transitions::snapshot(self, &run, &summary)?;
        self.consumed = Some(summary.consumed);
        self.run = Some(run);
        self.summary = Some(summary);
        if !transitions::accounting(self) {
            self.indeterminate.get_or_insert_with(|| {
                "native factory accounting is incomplete; preserve the original session".into()
            });
        }
        Ok(())
    }

    const fn opened(&mut self, purpose: RuntimePurpose) {
        self.active_runtime = Some(purpose);
        self.runtime_session = None;
        if matches!(purpose, RuntimePurpose::Execution) {
            self.execution_opened = true;
            self.execution_clean = false;
        }
    }

    const fn closed(&mut self, purpose: RuntimePurpose, clean: bool) {
        self.active_runtime = None;
        self.runtime_session = None;
        if matches!(purpose, RuntimePurpose::Execution) {
            self.execution_clean = clean;
        }
    }

    const fn dispatch(&mut self, operation: Operation) {
        self.dispatched = Some(operation);
        if matches!(operation, Operation::Start | Operation::Resume) {
            self.pending_admission = Some(operation);
        }
    }

    fn reject(&mut self, message: String) {
        self.rejected = Some(message);
        self.pending_admission = None;
    }

    const fn session(&mut self) {
        self.session_accepted = true;
        self.runtime_session = self.active_runtime;
    }

    fn apply(&mut self, data: Data) -> Result<(), String> {
        transitions::validate(self, &data)?;
        match data {
            Data::Prepared { .. } | Data::Notification { .. } => {}
            Data::SessionAccepted { .. } => self.session(),
            Data::RuntimeOpened { purpose, .. } => self.opened(purpose),
            Data::Dispatch { operation, .. } => self.dispatch(operation),
            Data::Accepted {
                run_id, attempt, ..
            } => return self.accept(run_id, attempt),
            Data::Observed { run, summary, .. } => return self.observe(*run, *summary),
            Data::Rejected { message, .. } => self.reject(message),
            Data::RuntimeClosed { purpose, clean, .. } => self.closed(purpose, clean),
            Data::Indeterminate { message, .. } => self.indeterminate = Some(message),
        }
        Ok(())
    }

    /// Whether only a known terminal outcome permits ordinary workspace cleanup.
    #[must_use]
    pub fn can_clean(&self) -> bool {
        if self.indeterminate.is_some() || !self.execution_clean || self.active_runtime.is_some() {
            return false;
        }
        if self.run_id.is_none() {
            return self.rejected.is_some();
        }
        self.accounting_complete()
            && self.summary.as_ref().is_some_and(|summary| {
                summary.can_resume == Some(false)
                    && matches!(
                        summary.status,
                        FactoryRunStatus::Completed
                            | FactoryRunStatus::Cancelled
                            | FactoryRunStatus::Error
                    )
            })
    }

    /// Whether the original start may have happened without correlated acceptance.
    #[must_use]
    pub fn ambiguous_start(&self) -> bool {
        self.dispatched == Some(Operation::Start)
            && self.run_id.is_none()
            && self.rejected.is_none()
    }

    /// Whether a clean acknowledged session may dispatch its first factory run.
    ///
    /// No accounting exists before dispatch; a sent start is never eligible here.
    /// Filesystem, lease, credential, and runtime eligibility checks still apply.
    #[must_use]
    pub const fn can_start(&self) -> bool {
        self.session_accepted
            && self.execution_clean
            && self.active_runtime.is_none()
            && self.pending_admission.is_none()
            && self.dispatched.is_none()
            && self.run_id.is_none()
            && self.rejected.is_none()
            && self.indeterminate.is_none()
    }

    /// A status candidate still requires intact runtime state and explicit authorization.
    #[must_use]
    pub fn can_resume(&self) -> bool {
        self.summary.as_ref().is_some_and(|summary| {
            summary.can_resume == Some(true) && summary.status.is_resume_candidate()
        }) && self.indeterminate.is_none()
            && self.accounting_complete()
            && self.execution_clean
            && self.active_runtime.is_none()
            && self.pending_admission.is_none()
    }

    /// Observed native status without deriving it from progress prose.
    #[must_use]
    pub fn status(&self) -> Option<FactoryRunStatus> {
        self.summary.as_ref().map(|summary| summary.status)
    }

    /// Whether current result/detail evidence leaves cumulative accounting complete.
    #[must_use]
    pub fn accounting_complete(&self) -> bool {
        self.indeterminate.is_none() && transitions::accounting(self)
    }

    /// A conservative actionable diagnosis, never an invitation to start a new run.
    #[must_use]
    pub fn diagnosis(&self) -> String {
        if self.ambiguous_start() {
            return "ambiguous factory start: no correlated run ID; never retry or match a latest run".into();
        }
        if let Some(message) = &self.indeterminate {
            return message.clone();
        }
        format!(
            "native factory {:?}; canResume {:?}; clean execution {}; preserve session {} and worktree",
            self.status(),
            self.summary.as_ref().and_then(|summary| summary.can_resume),
            self.execution_clean,
            self.intent.session_id
        )
    }
}

impl Records {
    fn prepare(&mut self, intent: Intent) -> Result<(), String> {
        transitions::intent(self, &intent)?;
        self.0
            .insert(intent.session_id.clone(), Record::from(intent));
        Ok(())
    }

    /// Folds a checked local factory fact without accepting an unbound observation.
    ///
    /// # Errors
    /// Rejects missing or changed identity and malformed lifecycle transitions.
    pub(in crate::runlog) fn apply(&mut self, data: Data) -> Result<(), String> {
        if let Data::Prepared { intent } = data {
            return self.prepare(*intent);
        }
        let record = self
            .0
            .get_mut(data.session_id())
            .ok_or("local factory event has no durable intent")?;
        record.apply(data)
    }

    /// Produces a checked next snapshot without mutating the current projection.
    ///
    /// # Errors
    /// Rejects malformed identity or lifecycle transitions.
    pub fn updated(&self, data: Data) -> io::Result<Self> {
        let mut next = self.clone();
        next.apply(data).map_err(io::Error::other)?;
        Ok(next)
    }

    /// Replays only this product's events; any malformed local event stops recovery.
    ///
    /// # Errors
    /// Rejects corrupt event data instead of silently falling back to a fresh run.
    pub fn replay(events: &[Event]) -> io::Result<Self> {
        let mut records = Self::default();
        for event in events
            .iter()
            .filter(|event| event.kind == EventKind::CopilotFactory)
        {
            let data = serde_json::from_value(event.data.clone()).map_err(|error| {
                io::Error::other(format!(
                    "invalid local factory event {}: {error}",
                    event.seq
                ))
            })?;
            records.apply(data).map_err(io::Error::other)?;
        }
        Ok(records)
    }
}
