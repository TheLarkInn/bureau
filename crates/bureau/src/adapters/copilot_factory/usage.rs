//! Factory accounting is cumulative, in nano-AIU, never provider invoice dollars.

use super::types::FactoryRunConsumed;
use crate::adapters::Usage;

/// Uses the same explicit credit normalization as the ordinary Copilot adapter.
/// The integer counters remain authoritative in the factory event log.
#[must_use]
pub fn measured(consumed: FactoryRunConsumed) -> Usage {
    let number = serde_json::Number::from(consumed.nano_aiu);
    let credits = number.as_f64().map(|value| value / 1_000_000_000.0);
    Usage {
        provider: "copilot_factory".to_owned(),
        credits,
        cost_usd: credits.map(|value| value * 0.01),
        cost_basis: credits.map(|_| "github_ai_credit_at_usd_0.01".to_owned()),
        ..Usage::default()
    }
}
