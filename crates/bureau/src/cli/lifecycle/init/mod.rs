mod access;
mod author;
mod draft;
mod effects;
mod files;
mod first_pass;
mod merge;
mod model;
mod proposal;
#[cfg(test)]
mod scaffold_tests;
mod validate;

use crate::cli::out;
use std::path::Path;

use anyhow::Context as _;

const REQUEST_TEMPLATE: &str = include_str!("request.yaml");

pub(super) fn print_template() {
    out::line(format_args!("{}", REQUEST_TEMPLATE.trim_end()));
}

const fn outcome_name(outcome: bureau::setup::Outcome) -> &'static str {
    match outcome {
        bureau::setup::Outcome::Success => "success",
        bureau::setup::Outcome::Failure => "failure",
        bureau::setup::Outcome::Blocked => "blocked",
        bureau::setup::Outcome::NoWork => "no-work",
    }
}

fn print_outcomes(summary: &bureau::setup::OutcomeSummary) {
    for run in &summary.runs {
        out::line(format_args!(
            "{}: {}",
            run.run_id,
            outcome_name(run.outcome)
        ));
    }
}

fn load(path: &Path) -> anyhow::Result<model::Request> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_slice(&bytes).context("parsing init request")
}

pub(super) async fn run(from: &Path) -> anyhow::Result<i32> {
    let request = load(from)?;
    let home = bureau::home::Home::discover()?;
    let layout = home.layout().clone();
    let runtime = tokio::runtime::Handle::current();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut request = request;
        let maintenance = bureau::maintenance::exclusive(layout.root())?;
        super::migrate::recover_pending(&layout, Some(&mut request.settings))?;
        let flow_request = request.flow_request();
        let mut effects = effects::local_effects(layout, request, runtime, maintenance);
        bureau::setup::InitFlow::new(flow_request)
            .run(&mut effects)
            .map_err(|error| match error {
                bureau::setup::FlowError::Effect(effect) => anyhow::Error::new(effect),
                other => anyhow::Error::new(other),
            })
    })
    .await
    .context("joining init flow")??;
    print_outcomes(&outcome.outcomes);
    Ok(0)
}

#[test]
fn request_template_builds_valid_fixed_config() {
    let request: model::Request = serde_yaml_ng::from_str(REQUEST_TEMPLATE).expect("request");
    let draft = draft::fixed(&request).expect("fixed draft");
    let temporary =
        files::Temporary::new(&std::env::temp_dir(), "init-template-test").expect("temporary");
    files::materialize(temporary.path(), &draft).expect("draft files");
    let config = bureau::config::Config::load(temporary.path()).expect("valid config");
    let assignment = &config.assignments[&request.assignment.name];
    let actual = (
        config.pipelines.len(),
        assignment.work.approval_label.as_deref(),
        request.settings.plugin.install_user_global,
    );
    assert_eq!(actual, (1, Some("bureau:approved"), false));
}

#[test]
fn print_template_keeps_the_top_level_command_cap() {
    use clap::CommandFactory as _;

    assert_eq!(crate::cli::Cli::command().get_subcommands().count(), 17);
}

#[test]
fn fixed_template_declares_config_access_and_verifies_no_work() {
    let request: model::Request = serde_yaml_ng::from_str(REQUEST_TEMPLATE).expect("request");
    let draft = draft::fixed(&request).expect("fixed draft");
    let path = format!("pipelines/{}.yaml", request.pipeline_name());
    let pipeline: bureau::config::Pipeline =
        serde_yaml_ng::from_slice(&draft.files[Path::new(&path)]).expect("pipeline");
    assert_eq!(
        (
            request.settings.credentials.contains_key("config"),
            pipeline.steps[0].on_no_work.as_deref()
        ),
        (true, Some("verify"))
    );
}
