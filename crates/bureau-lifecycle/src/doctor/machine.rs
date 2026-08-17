use super::model::{Area, Diagnostic, Observation, Report};

/// Read-only local inspection effects.
///
/// Implementations inspect only local state. They must not contact a forge,
/// invoke a model, or mutate the filesystem, process environment, or database.
pub trait Effects {
    /// Inspects one required area.
    ///
    /// # Errors
    /// Returns a local inspection failure without stopping later checks.
    fn inspect(&self, area: Area) -> Result<Observation, String>;
}

/// Invalid doctor state-machine input.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// An observation did not match the requested area.
    #[error("expected observation for {expected:?}, received {received:?}")]
    UnexpectedArea {
        /// Area currently requested.
        expected: Area,
        /// Area supplied by the caller.
        received: Area,
    },
    /// The caller attempted to record after all checks completed.
    #[error("doctor checks are already complete")]
    AlreadyComplete,
    /// The caller requested a report before all checks completed.
    #[error("doctor report has {0} checks remaining")]
    Incomplete(usize),
}

/// Deterministic state machine requesting every required diagnostic area.
#[derive(Debug, Default)]
pub struct Machine {
    next: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Machine {
    /// Starts with the local-state check.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: 0,
            diagnostics: Vec::new(),
        }
    }

    /// Area the caller must inspect next, or `None` when complete.
    #[must_use]
    pub fn request(&self) -> Option<Area> {
        Area::ALL.get(self.next).copied()
    }

    /// Records the requested area's observation and advances once.
    ///
    /// # Errors
    /// Rejects out-of-order and post-completion responses.
    pub fn record(&mut self, area: Area, observation: Observation) -> Result<(), Error> {
        let expected = self.request().ok_or(Error::AlreadyComplete)?;
        if expected != area {
            return Err(Error::UnexpectedArea {
                expected,
                received: area,
            });
        }
        self.record_current(area, observation);
        Ok(())
    }

    /// Finishes after every required area has been recorded.
    ///
    /// # Errors
    /// Rejects an incomplete report.
    pub fn finish(self) -> Result<Report, Error> {
        let remaining = Area::ALL.len().saturating_sub(self.next);
        if remaining == 0 {
            Ok(Report::new(self.diagnostics))
        } else {
            Err(Error::Incomplete(remaining))
        }
    }

    fn record_current(&mut self, area: Area, observation: Observation) {
        self.diagnostics
            .push(Diagnostic::from_observation(area, observation));
        self.next += 1;
    }
}

fn inspect(effects: &impl Effects, area: Area) -> Observation {
    effects
        .inspect(area)
        .unwrap_or_else(Observation::inspection_failed)
}

/// Runs all read-only checks against injected effects.
#[must_use]
pub fn run(effects: &impl Effects) -> Report {
    let mut machine = Machine::new();
    while let Some(area) = machine.request() {
        let observation = inspect(effects, area);
        machine.record_current(area, observation);
    }
    Report::new(machine.diagnostics)
}
