use anyhow::Context as _;
use bureau::forge::github::cloud::AutomationId;
use bureau::github_cloud;
use serde_json::json;

use super::network;
use super::output::{self, Output};
use super::read_args::{ListArgs, ShowArgs};

async fn inventory_value(
    context: &network::Context,
    automation: Option<AutomationId>,
) -> anyhow::Result<serde_json::Value> {
    let repo = context.selection.repo();
    Ok(if let Some(id) = automation {
        let tasks = context.client.tasks(repo, &id).await?;
        json!({"kind": "github_cloud_tasks", "repo": repo.name(), "automation_id": id, "tasks": context.value(&tasks)?})
    } else {
        let automations = context.client.automations(repo).await?;
        json!({"kind": "github_cloud_automations", "repo": repo.name(), "automations": context.value(&automations)?})
    })
}

async fn inventory(args: &ListArgs) -> anyhow::Result<Output> {
    let automation = args
        .automation
        .as_ref()
        .map(|id| AutomationId::try_from(id.clone()))
        .transpose()?;
    let paths = network::paths(&args.network, None, None)?;
    let context = network::connect(paths, &args.network, None).await?;
    Ok(Output::read(inventory_value(&context, automation).await?))
}

async fn definition(args: &ShowArgs, id: &str) -> anyhow::Result<Output> {
    let id = AutomationId::try_from(id.to_owned())?;
    let paths = network::paths(&args.network, None, None)?;
    let context = network::connect(paths, &args.network, None).await?;
    let definition = context
        .client
        .definition(context.selection.repo(), &id)
        .await?;
    let eligibility = match definition.dispatch_event() {
        Ok(event) => json!({"event": event.as_str()}),
        Err(error) => json!({"unavailable": error.to_string()}),
    };
    Ok(Output::read(json!({
        "kind": "github_cloud_definition", "definition": context.value(&definition)?,
        "dispatch": eligibility, "remote_controls": "unsupported",
    })))
}

async fn receipt(args: &ShowArgs, key: &str) -> anyhow::Result<Output> {
    let root = network::runs_root(args.runs.as_deref())?;
    let mut state = github_cloud::read_state(&root, key)?;
    if !args.refresh {
        anyhow::ensure!(
            !args.network.has_options(),
            "receipt network options require --refresh"
        );
        return output::observation(&state, args.output.events, "show");
    }
    let paths = network::paths(&args.network, Some(&root), args.state.clone())?;
    let context = network::connect(paths, &args.network, Some(&state)).await?;
    state = github_cloud::refresh(&context.control()?, key, args.output.events).await?;
    let mut output = output::observation(&state, args.output.events, "refresh")?;
    context.client.scrub(&mut output.value);
    Ok(output)
}

async fn inspect(args: &ShowArgs) -> anyhow::Result<Output> {
    if let Some(id) = &args.automation {
        return definition(args, id).await;
    }
    let key = args
        .run_id
        .as_deref()
        .context("cloud receipt key is required")?;
    receipt(args, key).await
}

pub(in crate::cli) async fn list(args: &ListArgs) -> anyhow::Result<i32> {
    output::finish(inventory(args).await, args.json)
}

pub(in crate::cli) async fn show(args: &ShowArgs) -> anyhow::Result<i32> {
    output::finish(inspect(args).await, args.output.json)
}
