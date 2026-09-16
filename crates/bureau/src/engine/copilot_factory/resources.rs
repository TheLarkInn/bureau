//! Compare restored native resource identities to the private, approved catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::adapters::copilot_factory::context_types::{PinnedContext, ResourceIdentity};
use crate::adapters::copilot_factory::rpc::Client;

use super::invoke;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Agent {
    id: String,
    path: Option<PathBuf>,
    mcp_servers: Option<BTreeMap<String, Value>>,
    skills: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Agents {
    agents: Vec<Agent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Skill {
    name: String,
    command_name: Option<String>,
    source: String,
    enabled: bool,
    user_invocable: bool,
    path: Option<PathBuf>,
    plugin_name: Option<String>,
}

#[derive(Deserialize)]
struct Skills {
    skills: Vec<Skill>,
}

#[derive(Deserialize)]
struct Command {
    name: String,
}

#[derive(Deserialize)]
struct Commands {
    commands: Vec<Command>,
}

fn path(identity: &ResourceIdentity, actual: Option<&PathBuf>) -> bool {
    match identity {
        ResourceIdentity::Inline => actual.is_none(),
        ResourceIdentity::Plugin { path, .. } => actual == Some(path),
    }
}

fn qualified(identity: &ResourceIdentity, name: &str) -> String {
    match identity {
        ResourceIdentity::Plugin { plugin, .. } if !name.contains(':') => {
            format!("{plugin}:{name}")
        }
        _ => name.to_owned(),
    }
}

fn bindings(
    value: &Agent,
    identity: &ResourceIdentity,
    context: &PinnedContext,
) -> Result<(), String> {
    let actual: BTreeSet<_> = value
        .skills
        .iter()
        .flatten()
        .map(|name| qualified(identity, name))
        .collect();
    let expected = context
        .expected_catalog
        .agent_skills
        .get(&value.id)
        .ok_or("agent has no approved skill bindings")?;
    if &actual != expected {
        return Err(format!(
            "native agent `{}` changed its approved skill bindings",
            value.id
        ));
    }
    Ok(())
}

fn agent(value: &Agent, context: &PinnedContext) -> Result<(), String> {
    let identity = context
        .expected_catalog
        .agents
        .get(&value.id)
        .ok_or_else(|| format!("unapproved native agent `{}`", value.id))?;
    if !path(identity, value.path.as_ref()) {
        return Err(format!(
            "native agent `{}` is not backed by its approved private pin",
            value.id
        ));
    }
    if value
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| !servers.is_empty())
    {
        return Err(
            "native agent unexpectedly includes executable inline MCP configuration".into(),
        );
    }
    bindings(value, identity, context)
}

fn check_agents(value: Value, context: &PinnedContext) -> Result<(), String> {
    let response: Agents = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let expected = &context.expected_catalog.agents;
    let mut found = BTreeSet::new();
    for value in response.agents {
        agent(&value, context)?;
        if !found.insert(value.id) {
            return Err("native agent catalog repeats an identity".into());
        }
    }
    if found != expected.keys().cloned().collect() {
        return Err("native agent catalog omitted approved agents; no factory was admitted".into());
    }
    Ok(())
}

fn skill_name(value: &Skill) -> Result<String, String> {
    let name = value.command_name.as_ref().unwrap_or(&value.name);
    if name.contains(':') {
        return Ok(name.clone());
    }
    let plugin = value
        .plugin_name
        .as_ref()
        .ok_or("plugin skill has no owning plugin name")?;
    Ok(format!("{plugin}:{name}"))
}

fn skill(value: &Skill, context: &PinnedContext) -> Result<String, String> {
    let name = skill_name(value)?;
    let catalog = &context.expected_catalog;
    let identity = catalog
        .skills
        .get(&name)
        .or_else(|| catalog.commands.get(&name))
        .ok_or_else(|| format!("unapproved native skill `{name}`"))?;
    let ResourceIdentity::Plugin { plugin, .. } = identity else {
        return Err("native skill is not supplied by an approved plugin".into());
    };
    if value.source != "plugin"
        || value.plugin_name.as_ref() != Some(plugin)
        || !value.enabled
        || !path(identity, value.path.as_ref())
    {
        return Err(format!(
            "native skill `{name}` is disabled or differs from its approved pin"
        ));
    }
    Ok(name)
}

fn check_skills(value: Value, context: &PinnedContext) -> Result<BTreeSet<String>, String> {
    let response: Skills = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let mut found = BTreeSet::new();
    let mut commands = BTreeSet::new();
    for value in response
        .skills
        .into_iter()
        .filter(|skill| skill.source != "builtin")
    {
        let name = skill(&value, context)?;
        if !found.insert(name.clone()) {
            return Err("native skill catalog repeats an identity".into());
        }
        if value.user_invocable {
            commands.insert(value.command_name.unwrap_or(name));
        }
    }
    let expected = &context.expected_catalog.skills;
    if !expected.keys().all(|name| found.contains(name)) {
        return Err("native skill catalog omitted an approved skill".into());
    }
    commands.extend(context.expected_catalog.commands.keys().cloned());
    Ok(commands)
}

async fn commands(
    client: &Client,
    session: &str,
    expected: BTreeSet<String>,
) -> Result<(), String> {
    let params = json!({"sessionId": session, "includeBuiltins": false,
        "includeSkills": true, "includeClientCommands": false});
    let response = invoke::call(client, "session.commands.list", params).await?;
    let response: Commands = serde_json::from_value(response).map_err(|error| error.to_string())?;
    let count = response.commands.len();
    let names: BTreeSet<_> = response
        .commands
        .into_iter()
        .map(|command| command.name)
        .collect();
    if names != expected || names.len() != count {
        return Err("native command catalog differs from the approved plugin resources".into());
    }
    Ok(())
}

async fn skills(client: &Client, session: &str, context: &PinnedContext) -> Result<(), String> {
    let value = invoke::call(client, "session.skills.list", json!({"sessionId": session})).await?;
    let expected = check_skills(value, context)?;
    commands(client, session, expected).await
}

pub(super) async fn verify(
    client: &Client,
    session: &str,
    context: &PinnedContext,
) -> Result<(), String> {
    let params = json!({"sessionId": session, "includeBuiltInAgents": false});
    let value = invoke::call(client, "session.agent.list", params).await?;
    check_agents(value, context)?;
    skills(client, session, context).await
}
