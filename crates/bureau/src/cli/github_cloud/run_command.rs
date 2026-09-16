use anyhow::Context as _;
use bureau::forge::github::cloud::{AutomationId, TaskId};
use bureau::github_cloud;

use super::network;
use super::output::{self, Output};
use super::run_args::RunArgs;

#[cfg(test)]
mod tests;

enum Operation {
    Dispatch(AutomationId),
    Track(AutomationId, TaskId),
}

impl Operation {
    fn parse(args: &RunArgs) -> anyhow::Result<Self> {
        match (
            &args.mode.dispatch_automation,
            &args.mode.track_task,
            &args.automation,
        ) {
            (Some(id), None, None) => Ok(Self::Dispatch(AutomationId::try_from(id.clone())?)),
            (None, Some(task), Some(id)) => Ok(Self::Track(
                AutomationId::try_from(id.clone())?,
                TaskId::try_from(task.clone())?,
            )),
            _ => anyhow::bail!(
                "select exactly one of --dispatch-automation or --track-task with --automation"
            ),
        }
    }

    async fn execute(
        self,
        control: &github_cloud::Control<'_>,
        key: &str,
    ) -> anyhow::Result<(github_cloud::State, &'static str)> {
        Ok(match self {
            Self::Dispatch(id) => (github_cloud::dispatch(control, key, &id).await?, "dispatch"),
            Self::Track(id, task) => (
                github_cloud::track(control, key, &id, &task).await?,
                "track",
            ),
        })
    }
}

async fn execute(args: &RunArgs) -> anyhow::Result<Output> {
    let key = args
        .request_id
        .as_deref()
        .context("--request-id is required")?;
    github_cloud::validate_key(key)?;
    let operation = Operation::parse(args)?;
    let paths = network::paths(&args.network, args.runs.as_deref(), args.state.clone())?;
    let context = network::connect(paths, &args.network, None).await?;
    let control = context.control()?;
    let (state, operation) = operation.execute(&control, key).await?;
    let mut output = output::receipt(&state, operation)?;
    context.client.scrub(&mut output.value);
    Ok(output)
}

pub(in crate::cli) async fn run(args: &RunArgs) -> anyhow::Result<i32> {
    output::finish(execute(args).await, args.json)
}
