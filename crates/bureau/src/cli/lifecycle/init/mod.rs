mod access;
mod author;
mod draft;
mod effects;
mod files;
mod first_pass;
mod merge;
mod model;
mod proposal;
mod validate;

use crate::cli::out;
use std::path::Path;

use anyhow::Context as _;

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
