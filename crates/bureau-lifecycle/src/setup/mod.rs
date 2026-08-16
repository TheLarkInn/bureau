//! Deterministic local lifecycle flows.
//!
//! The command layer supplies all filesystem, forge, plugin, and model
//! behavior through injected effect traits. The flows only enforce ordering.

mod effects;
mod error;
mod file;
mod init;
mod model;
mod settings;

pub use effects::{InitEffects, PluginEffects, SettingsEffects};
pub use error::FlowError;
pub use file::{FileEffects, FileError, load_settings, save_settings};
pub use init::{InitFlow, InitState};
pub use model::{
    ConfigDraft, ConfigPullRequest, ConfigSource, CredentialSource, FirstPipeline, InitOutcome,
    InitRequest, Merge, MigrationSettings, Outcome, OutcomeSummary, PluginSettings, ReconcilePass,
    RunSummary, Settings, ValidatedConfig,
};
pub use settings::{SetupFlow, SetupState};
