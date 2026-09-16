use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::CustomAgent;

#[derive(Deserialize)]
#[serde(untagged)]
enum Tools {
    List(Vec<String>),
    Text(String),
}

impl Tools {
    fn names(self) -> Vec<String> {
        match self {
            Self::List(names) => names,
            Self::Text(text) => text.split(',').map(|name| name.trim().to_owned()).collect(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    name: Option<String>,
    description: Option<String>,
    tools: Option<Tools>,
    model: Option<String>,
    #[serde(alias = "model-policy")]
    model_policy: Option<Value>,
    #[serde(alias = "reasoning-effort")]
    reasoning_effort: Option<String>,
    skills: Option<Vec<String>>,
    #[serde(alias = "mcp-servers")]
    mcp_servers: Option<BTreeMap<String, Value>>,
}

pub(super) fn document(text: &str) -> Result<(Option<&str>, &str), String> {
    let candidate = text.trim_start_matches(['\u{feff}', '\r', '\n', ' ', '\t']);
    let mut lines = candidate.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return Ok((None, text));
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((
                Some(&candidate[start..offset]),
                &candidate[offset + line.len()..],
            ));
        }
        offset += line.len();
    }
    Err("agent frontmatter has no closing --- delimiter".to_owned())
}

fn nonempty_names(names: Option<&[String]>) -> Result<(), String> {
    if names.is_some_and(|names| names.iter().any(|name| name.trim().is_empty())) {
        return Err("custom-agent tool/skill names must not be blank".to_owned());
    }
    Ok(())
}

pub(super) fn validate(agent: &CustomAgent) -> Result<(), String> {
    if agent.name.trim().is_empty() {
        return Err("custom-agent name must not be blank".to_owned());
    }
    if agent
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| !servers.is_empty())
    {
        return Err("custom-agent inline MCP is unsupported: server startup precedes tool filtering; use only Bureau's root MCP mapping".to_owned());
    }
    nonempty_names(agent.tools.as_deref())?;
    nonempty_names(agent.skills.as_deref())
}

fn definition(metadata: Metadata, expected: &str, prompt: &str) -> Result<CustomAgent, String> {
    if metadata
        .name
        .as_deref()
        .is_some_and(|name| name != expected)
    {
        return Err(format!(
            "agent frontmatter name must match the selected resource `{expected}`"
        ));
    }
    let agent = CustomAgent {
        name: expected.to_owned(),
        prompt: prompt.to_owned(),
        description: metadata.description,
        tools: metadata.tools.map(Tools::names),
        model: metadata.model,
        model_policy: metadata.model_policy,
        reasoning_effort: metadata.reasoning_effort,
        skills: metadata.skills,
        mcp_servers: metadata.mcp_servers,
    };
    validate(&agent)?;
    Ok(agent)
}

fn metadata(header: &str) -> Result<Metadata, String> {
    if header.trim().is_empty() {
        return Ok(Metadata::default());
    }
    let value: Value = serde_yaml_ng::from_str(header).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or("agent frontmatter must be an object")?;
    if object.values().any(Value::is_null) {
        return Err("custom-agent metadata fields cannot be null".to_owned());
    }
    serde_json::from_value(value)
        .map_err(|error| format!("unsupported or malformed custom-agent metadata: {error}"))
}

pub(super) fn parse(bytes: &[u8], expected: &str) -> Result<CustomAgent, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("agent must be UTF-8: {error}"))?;
    let (header, prompt) = document(text)?;
    let metadata = header.map(metadata).transpose()?.unwrap_or_default();
    definition(metadata, expected, prompt)
}
