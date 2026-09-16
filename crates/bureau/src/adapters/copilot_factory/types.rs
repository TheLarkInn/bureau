//! Passive projections of the experimental factory wire responses.
//!
//! `FactoryRunResult` has no verified `consumed` field: accounting is required
//! on summaries/details instead. Newer runtimes add attempts, pause metadata,
//! and summary resume eligibility; omission never proves eligibility.

use std::num::NonZeroU64;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn identity<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "factory identity must not be blank",
        ));
    }
    Ok(value)
}

/// A raw JSON-RPC error, distinct from a valid factory failure envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("remote JSON-RPC error {code}: {message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
}

impl RpcError {
    /// Exposes a string `data.code` without discarding any structured error data.
    #[must_use]
    pub fn data_code(&self) -> Option<&str> {
        self.data.as_ref()?.get("code")?.as_str()
    }
}

/// Known current/terminal wire states; unrecognized states fail decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactoryRunStatus {
    Pending,
    Running,
    Completed,
    Error,
    Cancelled,
    Halted,
    Paused,
}

impl FactoryRunStatus {
    /// Whether this attempt has settled, including failure and orderly pause.
    #[must_use]
    pub const fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Error | Self::Cancelled | Self::Halted | Self::Paused
        )
    }

    /// A status-only candidate, not authorization or proof of resumability.
    ///
    /// Callers must also check durable interruption, runtime `canResume`,
    /// ownership, accounting, and the remaining recovery/compatibility guards.
    #[must_use]
    pub const fn is_resume_candidate(self) -> bool {
        matches!(self, Self::Error | Self::Halted | Self::Paused)
    }
}

/// Cumulative counters across attempts, not deltas or a currency amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunConsumed {
    pub active_ms: u64,
    pub subagents: u64,
    pub nano_aiu: u64,
}

/// Full `run`/`getRun`/`cancel`/`pause` envelope, not a Bureau step result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunResult {
    #[serde(deserialize_with = "identity")]
    pub run_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub attempt: Option<NonZeroU64>,
    pub status: FactoryRunStatus,
    /// Explicit JSON null remains distinct from an absent result.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<String>,
    /// Preserve typed failure details, including accounting-incomplete evidence.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub failure: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub snapshot: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub pause_info: Option<Value>,
}

/// Minimum summary/detail projection requiring identity, status, and accounting.
///
/// `terminal` remains structured evidence; its result preview is not the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunSummary {
    #[serde(deserialize_with = "identity")]
    pub run_id: String,
    #[serde(deserialize_with = "identity")]
    pub factory_name: String,
    pub status: FactoryRunStatus,
    pub consumed: FactoryRunConsumed,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub terminal: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub can_resume: Option<bool>,
}

/// Explicit resume resolves the persisted factory name and returns its run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryResumeResult {
    #[serde(deserialize_with = "identity")]
    pub factory_name: String,
    pub run: FactoryRunResult,
}

#[cfg(test)]
#[path = "types_error_tests.rs"]
mod error_tests;
#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
