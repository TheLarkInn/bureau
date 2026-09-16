use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bureau_plugin::TreeSnapshot;
use serde_json::Value;

use super::{
    CustomAgent, ExpectedCatalog, PinnedPlugin, ResourceIdentity, agent, files, manifest, mcp,
};

fn insert(
    names: &mut BTreeMap<String, ResourceIdentity>,
    plugin: &str,
    name: &str,
    path: PathBuf,
) -> Result<(), String> {
    let name = format!("{plugin}:{name}");
    let identity = ResourceIdentity::Plugin {
        plugin: plugin.to_owned(),
        path,
    };
    if names.insert(name.clone(), identity).is_none() {
        return Ok(());
    }
    Err(format!("duplicate pinned resource `{name}`"))
}

fn prohibited(path: &Path) -> bool {
    ["hooks", ".github/hooks", ".github/copilot/hooks"]
        .iter()
        .any(|root| path.starts_with(root))
        || [
            "hooks.json",
            ".claude-plugin/plugin.json",
            ".github/plugin/plugin.json",
        ]
        .iter()
        .any(|name| path == Path::new(name))
}

fn agent_file(path: &Path) -> bool {
    let agent_directory = ["agents", ".github/agents", ".claude/agents"]
        .iter()
        .any(|root| path.starts_with(root));
    path.to_string_lossy().ends_with(".agent.md")
        || (agent_directory && path.extension().is_some_and(|extension| extension == "md"))
}

fn executable_file(tree: &TreeSnapshot, path: &Path) -> Result<(), String> {
    if prohibited(path) {
        return Err(format!(
            "unsupported plugin hooks or alternate manifest at {}",
            path.display()
        ));
    }
    if agent_file(path) {
        agent::parse(&files::read(tree, path)?, &files::name(path)?)?;
    }
    Ok(())
}

fn executable_files(tree: &TreeSnapshot) -> Result<(), String> {
    for path in files::all(tree)? {
        executable_file(tree, &path)?;
    }
    Ok(())
}

fn content_fields(object: &serde_json::Map<String, Value>, expected: &str) -> Result<(), String> {
    for key in ["hooks", "mcpServers", "mcp-servers", "mcp_servers"] {
        if object.contains_key(key) {
            return Err(format!("unsupported resource executable metadata `{key}`"));
        }
    }
    if object
        .get("name")
        .is_some_and(|name| name.as_str() != Some(expected))
    {
        return Err(format!("resource frontmatter name must match `{expected}`"));
    }
    Ok(())
}

fn content_header(bytes: &[u8], expected: &str) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let (header, _) = agent::document(text)?;
    let Some(header) = header else {
        return Ok(());
    };
    let value: Value = serde_yaml_ng::from_str(header)
        .map_err(|error| format!("invalid resource metadata: {error}"))?;
    let object = value
        .as_object()
        .ok_or("resource frontmatter must be an object")?;
    content_fields(object, expected)
}

fn agent_skills(catalog: &mut ExpectedCatalog, plugin: &str, agent: &CustomAgent) {
    let skills = agent.skills.iter().flatten().map(|skill| {
        if skill.contains(':') {
            skill.clone()
        } else {
            format!("{plugin}:{skill}")
        }
    });
    catalog
        .agent_skills
        .insert(format!("{plugin}:{}", agent.name), skills.collect());
}

fn agents(
    tree: &TreeSnapshot,
    paths: &[String],
    plugin: &str,
    catalog: &mut ExpectedCatalog,
) -> Result<(), String> {
    for path in files::selected(tree, paths)? {
        if path.extension().is_some_and(|extension| extension == "md") {
            let name = files::name(&path)?;
            let agent = agent::parse(&files::read(tree, &path)?, &name)?;
            agent_skills(catalog, plugin, &agent);
            insert(
                &mut catalog.agents,
                plugin,
                &name,
                tree.directory().join(&path),
            )?;
        }
    }
    Ok(())
}

fn skills(
    tree: &TreeSnapshot,
    paths: &[String],
    plugin: &str,
    catalog: &mut ExpectedCatalog,
) -> Result<(), String> {
    for path in files::selected(tree, paths)? {
        if path.file_name().is_some_and(|name| name == "SKILL.md") {
            let name = files::name(path.parent().ok_or("skill has no parent directory")?)?;
            content_header(&files::read(tree, &path)?, &name)?;
            insert(
                &mut catalog.skills,
                plugin,
                &name,
                tree.directory().join(&path),
            )?;
        }
    }
    Ok(())
}

fn commands(
    tree: &TreeSnapshot,
    paths: &[String],
    plugin: &str,
    catalog: &mut ExpectedCatalog,
) -> Result<(), String> {
    for path in files::selected(tree, paths)? {
        if path.extension().is_some_and(|extension| extension == "md") {
            let name = files::name(&path)?;
            content_header(&files::read(tree, &path)?, &name)?;
            insert(
                &mut catalog.commands,
                plugin,
                &name,
                tree.directory().join(&path),
            )?;
        }
    }
    Ok(())
}

pub(super) fn plugin(plugin: &PinnedPlugin, catalog: &mut ExpectedCatalog) -> Result<(), String> {
    let tree = TreeSnapshot::open(&plugin.directory, &plugin.source.digest)
        .map_err(|error| error.to_string())?;
    executable_files(&tree)?;
    let paths = manifest::read(&tree, plugin)?;
    agents(&tree, &paths.agents, &plugin.source.name, catalog)?;
    skills(&tree, &paths.skills, &plugin.source.name, catalog)?;
    commands(&tree, &paths.commands, &plugin.source.name, catalog)?;
    for path in paths.mcp {
        if mcp::audit(&tree, Path::new(&path))? {
            catalog.bureau_io_plugins.insert(plugin.source.name.clone());
        }
    }
    Ok(())
}
