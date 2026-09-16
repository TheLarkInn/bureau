use std::collections::BTreeMap;
use std::path::Path;

use bureau_plugin::TreeSnapshot;
use serde::Deserialize;
use serde_json::Value;

use super::{PinnedPlugin, files};

#[derive(Deserialize)]
#[serde(untagged)]
enum Locations {
    One(String),
    Many(Vec<String>),
}

impl Locations {
    fn paths(self) -> Vec<String> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    name: String,
    version: String,
    agents: Option<Locations>,
    skills: Option<Locations>,
    commands: Option<Locations>,
    mcp_servers: Option<String>,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}

fn metadata(fields: &BTreeMap<String, Value>) -> Result<(), String> {
    let supported = [
        "description",
        "author",
        "homepage",
        "repository",
        "license",
        "keywords",
    ];
    if let Some(field) = fields
        .keys()
        .find(|field| !supported.contains(&field.as_str()))
    {
        return Err(format!(
            "unsupported plugin manifest `{field}`; factory context refuses hooks and unreviewed executable configuration"
        ));
    }
    Ok(())
}

fn metadata_types(fields: &BTreeMap<String, Value>) -> Result<(), String> {
    for field in ["description", "homepage", "repository", "license"] {
        if fields.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("plugin metadata `{field}` must be a string"));
        }
    }
    Ok(())
}

fn locations(
    tree: &TreeSnapshot,
    value: Option<Locations>,
    default: &str,
) -> Result<Vec<String>, String> {
    let paths = match value {
        Some(value) => value.paths(),
        None if files::exists(&tree.directory().join(default))? => vec![default.to_owned()],
        None => Vec::new(),
    };
    for path in &paths {
        files::relative(Path::new(path))?;
        tree.path(Path::new(path))
            .map_err(|error| error.to_string())?;
    }
    Ok(paths)
}

fn mcp_paths(tree: &TreeSnapshot, declared: Option<String>) -> Result<Vec<String>, String> {
    let mut paths = declared.into_iter().collect::<Vec<_>>();
    if files::exists(&tree.directory().join(".mcp.json"))? {
        paths.push(".mcp.json".to_owned());
    }
    paths.sort();
    paths.dedup();
    for path in &paths {
        files::relative(Path::new(path))?;
    }
    Ok(paths)
}

pub(super) struct Contributions {
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub commands: Vec<String>,
    pub mcp: Vec<String>,
}

pub(super) fn read(tree: &TreeSnapshot, plugin: &PinnedPlugin) -> Result<Contributions, String> {
    let bytes = files::read(tree, Path::new("plugin.json"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("unsupported plugin manifest: {error}"))?;
    if manifest.name != plugin.source.name
        || manifest.version != plugin.source.version
        || manifest.version.trim().is_empty()
    {
        return Err("pinned plugin manifest differs from its saved identity".to_owned());
    }
    metadata(&manifest.other)?;
    metadata_types(&manifest.other)?;
    Ok(Contributions {
        agents: locations(tree, manifest.agents, "agents")?,
        skills: locations(tree, manifest.skills, "skills")?,
        commands: locations(tree, manifest.commands, "commands")?,
        mcp: mcp_paths(tree, manifest.mcp_servers)?,
    })
}
