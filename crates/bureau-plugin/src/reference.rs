//! Parsing for plugin-backed role agent references.

use super::Error;

const fn valid_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
}

fn valid_segment(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value.bytes().all(valid_byte)
}

#[derive(Debug)]
pub struct AgentReference {
    pub plugin: String,
    pub agent: String,
}

impl AgentReference {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let Some(body) = value.strip_prefix('/') else {
            return Err(Error::InvalidReference(value.to_owned()));
        };
        let Some((plugin, agent)) = body.split_once(':') else {
            return Err(Error::InvalidReference(value.to_owned()));
        };
        if valid_segment(plugin) && valid_segment(agent) && !agent.contains(':') {
            return Ok(Self {
                plugin: plugin.to_owned(),
                agent: agent.to_owned(),
            });
        }
        Err(Error::InvalidReference(value.to_owned()))
    }
}
