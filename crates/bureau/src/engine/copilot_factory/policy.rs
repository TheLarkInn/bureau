//! Exact inherited SDK tool ceilings, in addition to the existing CLI permission flags.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::config::Permission;
use crate::mcp::Session;

use super::prepare::Prepared;

const READ: &[&str] = &["view", "grep", "glob"];
const WRITE: &[&str] = &[
    "apply_patch",
    "str_replace_editor",
    "create",
    "edit",
    "bash",
    "read_bash",
    "stop_bash",
    "list_bash",
];
const AGENTS: &[&str] = &["task", "read_agent", "list_agents", "write_agent"];
pub(super) const BROKER_TOOLS: [&str; 2] = ["get_step_context", "publish_result"];

fn extend(tools: &mut BTreeSet<String>, names: &[&str]) {
    tools.extend(names.iter().map(|name| (*name).to_owned()));
}

fn repository_tools(prepared: &Prepared, tools: &mut BTreeSet<String>) {
    let permissions = &prepared.role.permissions;
    let (write, _) = crate::adapters::real::push_boundary(permissions);
    if write || permissions.contains(&Permission::RepoRead) {
        extend(tools, READ);
    }
    if write {
        extend(tools, WRITE);
    }
}

pub(super) fn builtins(prepared: &Prepared, execute: bool) -> BTreeSet<String> {
    let mut tools = BTreeSet::new();
    if !execute {
        return tools;
    }
    repository_tools(prepared, &mut tools);
    extend(&mut tools, AGENTS);
    if !prepared.intent.context.expected_catalog.skills.is_empty() {
        tools.insert("skill".to_owned());
    }
    tools
}

pub(super) fn available(prepared: &Prepared, execute: bool) -> BTreeSet<String> {
    let mut tools = builtins(prepared, execute)
        .into_iter()
        .map(|tool| format!("builtin:{tool}"))
        .collect::<BTreeSet<_>>();
    if execute {
        tools.extend(BROKER_TOOLS.map(|tool| format!("bureau-io/{tool}")));
    }
    tools
}

fn excluded(prepared: &Prepared, execute: bool) -> BTreeSet<String> {
    let mut tools: BTreeSet<String> =
        ["run_factory", "factories_manage", "ask_user", "builtin:lsp"]
            .map(str::to_owned)
            .into_iter()
            .collect();
    let allowed = builtins(prepared, execute);
    for tool in READ.iter().chain(WRITE).chain(AGENTS) {
        if !allowed.contains(*tool) {
            tools.insert((*tool).to_owned());
        }
    }
    tools
}

pub(super) fn broker_configuration(broker: &Session) -> Result<Value, String> {
    let executable =
        std::fs::canonicalize(std::env::current_exe().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let configuration = json!({
        "type": "stdio", "command": executable, "args": ["mcp", "serve"],
        "env": broker.env(), "tools": BROKER_TOOLS
    });
    if configuration.to_string().contains('$') {
        return Err(
            "factory broker configuration cannot contain native environment expansion".into(),
        );
    }
    Ok(configuration)
}

pub(super) fn parameters(
    prepared: &Prepared,
    broker: &Session,
    execute: bool,
) -> Result<Value, String> {
    Ok(json!({
        "availableTools": available(prepared, execute),
        "excludedTools": excluded(prepared, execute),
        "shell": {"credentials": {"git": false, "gh": false}},
        "envValueMode": "direct",
        "mcpServers": {"bureau-io": broker_configuration(broker)?}
    }))
}
