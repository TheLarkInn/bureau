use serde::{Deserialize, Serialize};

use super::Error;

pub(super) const ORIGIN: &str = "https://api.github.com";

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_url(url: &reqwest::Url) -> bool {
    let user_ok = url.username().is_empty() || (url.scheme() == "ssh" && url.username() == "git");
    matches!(url.scheme(), "https" | "ssh")
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && user_ok
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn reference_path(value: &str) -> Result<String, Error> {
    if value.contains("://") {
        let url = reqwest::Url::parse(value).map_err(|error| Error::Identity(error.to_string()))?;
        if !valid_url(&url) {
            return Err(Error::Unsupported(
                "only credential-free dotcom repository references are supported".to_owned(),
            ));
        }
        return Ok(url.path().trim_matches('/').to_owned());
    }
    Ok(value
        .strip_prefix("git@github.com:")
        .unwrap_or(value)
        .to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: i64,
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryRef {
    owner: String,
    name: String,
}

impl RepositoryRef {
    /// Parses a dotcom registry URL, SSH reference, or owner/name.
    ///
    /// # Errors
    /// Rejects unsupported hosts, credentials, and ambiguous paths.
    pub fn parse(value: &str) -> Result<Self, Error> {
        let path = reference_path(value)?;
        let path = path.strip_suffix(".git").unwrap_or(&path);
        let (owner, name) = path
            .split_once('/')
            .ok_or_else(|| Error::Identity("expected owner/name".to_owned()))?;
        if !valid_component(owner) || !valid_component(name) {
            return Err(Error::Identity("invalid repository path".to_owned()));
        }
        Ok(Self {
            owner: owner.to_ascii_lowercase(),
            name: name.to_ascii_lowercase(),
        })
    }

    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub(super) fn segments(&self) -> [&str; 2] {
        [&self.owner, &self.name]
    }

    pub(super) fn check(&self, repository: Option<&Repository>) -> Result<(), Error> {
        let Some(repository) = repository else {
            return Ok(());
        };
        let matches = repository.id > 0
            && repository.owner.eq_ignore_ascii_case(&self.owner)
            && repository.name.eq_ignore_ascii_case(&self.name);
        if matches {
            Ok(())
        } else {
            Err(Error::Identity(
                "automation repository differs from the selected repository".to_owned(),
            ))
        }
    }
}
