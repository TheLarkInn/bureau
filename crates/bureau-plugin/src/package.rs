use std::path::Path;

use serde_json::Value;

use super::Error;

const REQUIRED: [&str; 6] = [
    "plugin.json",
    ".mcp.json",
    "agents/implementer.agent.md",
    "agents/reviewer.agent.md",
    "skills/pipeline-author/SKILL.md",
    "skills/run-inspector/SKILL.md",
];

pub fn inspect(root: &Path) -> Result<PackageInfo, Error> {
    for relative in REQUIRED {
        regular_file(&root.join(relative))?;
    }

    let plugin = read_json(&root.join("plugin.json"))?;
    let mcp = read_json(&root.join(".mcp.json"))?;
    let name = required_string(&plugin, "name", root)?;
    let version = required_string(&plugin, "version", root)?;
    validate_mcp(&mcp, root)?;
    Ok(PackageInfo { name, version })
}

pub fn version(root: &Path) -> Result<String, Error> {
    let path = root.join("plugin.json");
    regular_file(&path)?;
    let plugin = read_json(&path)?;
    required_string(&plugin, "version", root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// Declared plugin name.
    pub name: String,
    /// Declared semantic version.
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    /// Complete command arguments, including `copilot`.
    pub argv: Vec<String>,
    /// Safe working directory for the command.
    pub directory: std::path::PathBuf,
}

pub fn install_commands(source_root: &Path) -> Result<[InstallCommand; 2], Error> {
    let source = source_root
        .to_str()
        .ok_or_else(|| Error::invalid(source_root, "plugin source path is not UTF-8"))?;
    Ok([
        InstallCommand {
            argv: ["copilot", "plugin", "marketplace", "add", source]
                .map(str::to_owned)
                .to_vec(),
            directory: source_root.to_path_buf(),
        },
        InstallCommand {
            argv: ["copilot", "plugin", "install", "bureau@bureau"]
                .map(str::to_owned)
                .to_vec(),
            directory: source_root.to_path_buf(),
        },
    ])
}

pub fn validate_install_result(success: bool, stderr: &[u8]) -> Result<(), Error> {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    let present =
        text.contains("already") && (text.contains("install") || text.contains("marketplace"));
    if success || present {
        Ok(())
    } else {
        Err(Error::Install(
            String::from_utf8_lossy(stderr).trim().to_owned(),
        ))
    }
}

fn regular_file(path: &Path) -> Result<(), Error> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| Error::io("inspect package", path, error))?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(Error::invalid(
            path,
            "plugin package path is not a safe file",
        ))
    }
}

fn read_json(path: &Path) -> Result<Value, Error> {
    let bytes =
        std::fs::read(path).map_err(|error| Error::io("read plugin package", path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| Error::invalid(path, error))
}

fn required_string(value: &Value, field: &str, root: &Path) -> Result<String, Error> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::invalid(root, format!("plugin `{field}` is missing")))
}

fn validate_mcp(value: &Value, root: &Path) -> Result<(), Error> {
    let server = &value["mcpServers"]["bureau-io"];
    let args = server["args"].as_array();
    let valid = server["command"] == "bureau" && args.is_some_and(|args| args == &["mcp", "serve"]);
    if valid {
        Ok(())
    } else {
        Err(Error::invalid(root, "bureau-io MCP definition is invalid"))
    }
}
