//! The existing Copilot permission and process contract, with private SDK storage.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::adapters::copilot;
use crate::config::StepDef;
use crate::mcp::Session;
use crate::process::{SpawnRequest, shared_log};

use super::super::context::RunCtx;
use super::super::stream::LogSink;
use super::prepare::Prepared;
use super::{credentials, policy};

fn environment(prepared: &Prepared) -> BTreeMap<String, String> {
    let paths = &prepared.artifacts.paths;
    [
        ("HOME", &paths.storage.user_home),
        ("USERPROFILE", &paths.storage.user_home),
        ("COPILOT_HOME", &paths.storage.copilot_home),
        ("XDG_CONFIG_HOME", &paths.storage.user_home),
        ("XDG_CACHE_HOME", &paths.storage.user_home),
        ("XDG_DATA_HOME", &paths.storage.user_home),
        ("XDG_STATE_HOME", &paths.storage.user_home),
        ("XDG_RUNTIME_DIR", &paths.storage.user_home),
        ("CLAUDE_CONFIG_DIR", &paths.storage.user_home),
        ("COPILOT_CLI_DIST_DIR", &prepared.artifacts.launch.dist),
        ("COPILOT_SDK_PATH", &prepared.artifacts.launch.sdk),
    ]
    .into_iter()
    .map(|(name, path)| (name.to_owned(), path.to_string_lossy().into_owned()))
    .collect()
}

fn command(prepared: &Prepared, scoped: Vec<String>) -> Vec<String> {
    let launch = &prepared.artifacts.launch;
    let mut argv = vec![launch.executable.to_string_lossy().into_owned()];
    argv.extend(
        launch
            .cli
            .iter()
            .map(|path| path.to_string_lossy().into_owned()),
    );
    argv.extend(scoped.into_iter().skip(1).map(|arg| {
        if arg == "--acp" {
            "--server".to_owned()
        } else {
            arg
        }
    }));
    argv.push("--no-auto-update".to_owned());
    argv
}

pub(super) fn request(
    ctx: &RunCtx,
    step: &StepDef,
    prepared: &Prepared,
    timeout: Duration,
) -> SpawnRequest {
    let sink = shared_log(LogSink::new(&step.name, &ctx.log, ctx.plan.lease.clone()));
    let mut request = copilot::spawn_request(
        &prepared.role,
        step,
        &prepared.intent.request,
        ctx.secrets(),
        Some(sink),
    );
    request.argv = command(prepared, request.argv);
    request.env.extend(environment(prepared));
    credentials::configure(&mut request, &prepared.model_token);
    request.timeout = timeout;
    // PAUSE/CANCEL are orderly native operations. The supervisor retains its deadline.
    request.cancel = None;
    request
}

pub(super) fn broker(prepared: &Prepared) -> Result<Session, String> {
    let broker = Session::create(&prepared.intent.request).map_err(|error| error.to_string())?;
    policy::broker_configuration(&broker)?;
    Ok(broker)
}
