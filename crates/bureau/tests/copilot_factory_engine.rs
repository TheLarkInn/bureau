//! End-to-end local factory lifecycle, using only an offline protocol executable.

#[path = "factory_engine/admission.rs"]
mod admission;
#[path = "factory_engine/artifacts.rs"]
mod artifacts;
#[path = "factory_engine/controls.rs"]
mod controls;
#[path = "factory_engine/corrections.rs"]
mod corrections;
#[path = "factory_engine/credentials.rs"]
mod credentials;
#[path = "factory_engine/evidence.rs"]
mod evidence;
#[path = "factory_engine/fixture.rs"]
mod fixture;
#[path = "factory_engine/guards.rs"]
mod guards;
#[path = "factory_engine/interrupted.rs"]
mod interrupted;
#[path = "factory_engine/notify.rs"]
mod notify;
#[path = "factory_engine/plan.rs"]
mod plan;
#[path = "factory_engine/plugins.rs"]
mod plugins;
#[path = "factory_engine/recovery.rs"]
mod recovery;
#[path = "factory_engine/replay.rs"]
mod replay;
#[path = "factory_engine/success.rs"]
mod success;
#[path = "factory_engine/tool_policy.rs"]
mod tool_policy;
#[path = "factory_engine/workspace_identity.rs"]
mod workspace_identity;
