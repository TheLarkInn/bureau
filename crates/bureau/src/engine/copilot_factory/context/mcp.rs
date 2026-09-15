use std::collections::BTreeMap;
use std::path::Path;

use bureau_plugin::TreeSnapshot;
use serde::Deserialize;

use super::files;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Server {
    #[serde(rename = "type")]
    kind: Option<String>,
    command: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    mcp_servers: BTreeMap<String, Server>,
}

fn bureau_server(name: &str, server: &Server) -> Result<(), String> {
    let stdio = server.kind.as_deref().is_none_or(|kind| kind == "stdio");
    let command = server.command == "bureau" && server.args == ["mcp", "serve"];
    if name == "bureau-io" && stdio && command {
        return Ok(());
    }
    Err("factory context supports only the exact bureau-io stdio declaration; arbitrary MCP commands, arguments, environment, and remote servers are refused".to_owned())
}

pub(super) fn audit(tree: &TreeSnapshot, path: &Path) -> Result<bool, String> {
    let config: Config = serde_json::from_slice(&files::read(tree, path)?).map_err(|error| {
        format!(
            "unsupported executable MCP configuration {}: {error}",
            path.display()
        )
    })?;
    for (name, server) in &config.mcp_servers {
        bureau_server(name, server)?;
    }
    Ok(!config.mcp_servers.is_empty())
}
