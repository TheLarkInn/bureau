use anyhow::Context as _;

use super::McpAction;

pub(super) fn run(action: &McpAction) -> anyhow::Result<i32> {
    match action {
        McpAction::Serve => {
            bureau::mcp::serve_stdio().context("serving MCP over stdio")?;
            Ok(0)
        }
    }
}
