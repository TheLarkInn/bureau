//! Exact advertised agent selection; labels are never identifiers.

use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigSelect, SessionConfigSelectOption,
    SessionConfigSelectOptions,
};
use agent_client_protocol::{Error, Result};

pub(super) fn error(message: impl Into<String>) -> Error {
    Error::internal_error().data(message.into())
}

fn choices(options: &SessionConfigSelectOptions) -> Vec<&SessionConfigSelectOption> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|group| &group.options).collect()
        }
        _ => Vec::new(),
    }
}

pub(super) fn selector(options: &[SessionConfigOption]) -> Result<&SessionConfigSelect> {
    let mut matching = options
        .iter()
        .filter(|option| option.id.0.as_ref() == "agent");
    let option = matching
        .next()
        .ok_or_else(|| error("ACP server did not advertise a custom-agent selector"))?;
    if matching.next().is_some() {
        return Err(error("ACP server advertised duplicate agent selectors"));
    }
    match &option.kind {
        SessionConfigKind::Select(select) => Ok(select),
        _ => Err(error("ACP agent selector is not a select option")),
    }
}

pub(super) fn advertised(options: &[SessionConfigOption], agent: &str) -> Result<()> {
    let count = choices(&selector(options)?.options)
        .iter()
        .filter(|option| option.value.0.as_ref() == agent)
        .count();
    if count != 1 {
        return Err(error(format!(
            "ACP agent `{agent}` must be advertised exactly once"
        )));
    }
    Ok(())
}

pub(super) fn selected(options: &[SessionConfigOption], agent: &str) -> Result<()> {
    advertised(options, agent)?;
    if selector(options)?.current_value.0.as_ref() != agent {
        return Err(error(format!("ACP server did not retain agent `{agent}`")));
    }
    Ok(())
}
