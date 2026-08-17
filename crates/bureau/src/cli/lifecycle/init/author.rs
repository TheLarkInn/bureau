use std::path::Path;

use bureau::home::Environment as _;
use bureau::setup::ConfigDraft;

use super::draft;
use super::files::{self, Temporary};
use super::model::Request;

fn copy_environment(command: &mut std::process::Command) {
    let environment = bureau::home::ProcessEnvironment;
    for name in ["PATH", "HOME", "COPILOT_HOME", "XDG_CONFIG_HOME"] {
        if let Some(value) = environment.value(name) {
            command.env(name, value);
        }
    }
}

fn run(directory: &Path, request: &str, pipeline: &str) -> anyhow::Result<()> {
    let prompt = format!(
        "Use the bureau:pipeline-author skill. {request}\n\
         Edit only `{pipeline}`. Keep the pipeline name and use existing roles."
    );
    let mut command = std::process::Command::new("copilot");
    command
        .args(["-p", &prompt, "--allow-all-tools"])
        .current_dir(directory)
        .env_clear();
    copy_environment(&mut command);
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "pipeline author failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub(super) fn prepare(
    layout: &bureau::home::Layout,
    request: &Request,
    prompt: &str,
) -> anyhow::Result<ConfigDraft> {
    let temporary = Temporary::new(layout.config_cache(), "init-author")?;
    let base = draft::fixed(request)?;
    files::materialize(temporary.path(), &base)?;
    let pipeline = request.pipeline_name();
    let path = format!("pipelines/{pipeline}.yaml");
    run(temporary.path(), prompt, &path)?;
    let bytes = std::fs::read(temporary.path().join(path))?;
    draft::complete(request, bytes)
}
