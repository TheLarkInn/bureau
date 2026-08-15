//! Dedup policy for finished runs: which terminal outcomes leave a
//! marker (DESIGN.md section 8 teardown, section 13 idempotence).

use crate::contract::StepOutcome;
use crate::engine::RunOutcome;
use crate::state::Disposition;

/// The dedup marker a finished run leaves, if any (section 13:
/// re-reconciling an unchanged item produces no new run). A run that
/// opened a PR records `Proposed`; every other terminal but `Failure`
/// records `NoChange` — the content was engaged and settled, so the
/// next pass must not re-claim it while its hash is unchanged.
/// `Failure` leaves no marker: a failed run stays retryable.
pub(super) const fn marker(outcome: &RunOutcome) -> Option<Disposition> {
    if outcome.pr.is_some() {
        return Some(Disposition::Proposed);
    }
    match outcome.outcome {
        StepOutcome::Failure => None,
        StepOutcome::Success | StepOutcome::Blocked | StepOutcome::NoWork => {
            Some(Disposition::NoChange)
        }
    }
}
