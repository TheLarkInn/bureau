//! Public stable session setup and the documented Claude launch options.

use agent_client_protocol::schema::v1::{
    EnvVariable, McpServer, McpServerStdio, NewSessionRequest,
};
use serde::Serialize;

use crate::config::{AdapterKind, Role};
use crate::contract::StepRequest;
use crate::mcp::Session;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOptions {
    agent: String,
    allowed_tools: Vec<&'static str>,
    disallowed_tools: Vec<&'static str>,
    strict_mcp_config: bool,
}

fn claude_options(role: &Role, agent: &str) -> ClaudeOptions {
    let (write, push) = crate::adapters::real::push_boundary(&role.permissions);
    let mut allowed_tools = vec![
        "mcp__bureau-io__get_step_context",
        "mcp__bureau-io__publish_result",
    ];
    let mut disallowed_tools = Vec::new();
    if write {
        allowed_tools.extend(["Edit", "Write", "Bash"]);
    } else {
        disallowed_tools.push("Bash(*)");
    }
    if write && !push {
        disallowed_tools.push("Bash(git push:*)");
    }
    ClaudeOptions {
        agent: agent.to_owned(),
        allowed_tools,
        disallowed_tools,
        strict_mcp_config: true,
    }
}

pub(super) fn new_session(
    role: &Role,
    request: &StepRequest,
    session: &Session,
    agent: &str,
) -> Result<NewSessionRequest, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let env = session
        .env()
        .iter()
        .map(|(name, value)| EnvVariable::new(name, value))
        .collect();
    let server = McpServerStdio::new("bureau-io", executable)
        .args(vec!["mcp".to_owned(), "serve".to_owned()])
        .env(env);
    let mut built =
        NewSessionRequest::new(&request.worktree).mcp_servers(vec![McpServer::Stdio(server)]);
    if role.adapter == AdapterKind::Claude {
        let options =
            serde_json::to_value(claude_options(role, agent)).map_err(|error| error.to_string())?;
        built.meta = Some(serde_json::Map::from_iter([(
            "claudeCode".to_owned(),
            serde_json::json!({"options": options}),
        )]));
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Permission;
    use crate::contract::Trust;

    fn role(permissions: Vec<Permission>) -> Role {
        Role {
            name: "worker".to_owned(),
            agent: "/bureau:implementer".to_owned(),
            adapter: AdapterKind::Claude,
            permissions,
            min_trust: Trust::Maintainer,
        }
    }

    #[test]
    fn claude_native_grants_preserve_write_and_push_boundaries() {
        let cases = [
            (vec![], false, vec!["Bash(*)"]),
            (vec![Permission::RepoRead], false, vec!["Bash(*)"]),
            (vec![Permission::RepoWrite], true, vec!["Bash(git push:*)"]),
            (vec![Permission::RepoPush], true, vec![]),
        ];
        for (grants, write, denied) in cases {
            let options = claude_options(&role(grants), "implementer");
            assert_eq!(
                (
                    options.allowed_tools.contains(&"Bash"),
                    options.disallowed_tools
                ),
                (write, denied)
            );
        }
    }

    #[test]
    fn claude_preapproves_only_role_tools_and_bureau_io() {
        let options = claude_options(&role(vec![Permission::RepoWrite]), "implementer");
        let json = serde_json::to_value(options).expect("options");
        assert_eq!(
            json,
            serde_json::json!({
                "agent": "implementer",
                "allowedTools": ["mcp__bureau-io__get_step_context", "mcp__bureau-io__publish_result",
                    "Edit", "Write", "Bash"],
                "disallowedTools": ["Bash(git push:*)"],
                "strictMcpConfig": true
            })
        );
    }
}
