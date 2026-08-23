//! One parser for GitHub repository references and API locations.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    scheme: String,
    host: String,
    port: u16,
    owner: String,
    name: String,
}

impl Location {
    pub fn identity(&self) -> String {
        format!("{}:{}/{}/{}", self.host, self.port, self.owner, self.name).to_ascii_lowercase()
    }

    pub fn name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn api_url(&self) -> String {
        if self.host == "github.com" && self.port == 443 {
            return "https://api.github.com".to_owned();
        }
        let port = match (self.scheme.as_str(), self.port) {
            ("https", 443) | ("http", 80) => String::new(),
            (_, port) => format!(":{port}"),
        };
        format!("{}://{}{}{}", self.scheme, self.host, port, "/api/v3")
    }
}

fn path_parts(path: &str) -> Option<(String, String)> {
    let path = path.trim_matches('/').trim_end_matches(".git");
    let mut segments = path.rsplit('/');
    let (name, owner) = (segments.next()?, segments.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| (owner.to_owned(), name.to_owned()))
}

fn transport(url: &reqwest::Url) -> Option<(String, u16)> {
    match url.scheme() {
        "http" => Some(("http".to_owned(), url.port_or_known_default()?)),
        "https" => Some(("https".to_owned(), url.port_or_known_default()?)),
        _ => Some(("https".to_owned(), 443)),
    }
}

fn from_url(url: &reqwest::Url) -> Option<Location> {
    let host = url.host_str()?.to_ascii_lowercase();
    let (owner, name) = path_parts(url.path())?;
    let (scheme, port) = transport(url)?;
    Some(Location {
        scheme,
        host,
        port,
        owner,
        name,
    })
}

fn from_scp(value: &str) -> Option<Location> {
    let (authority, path) = value.split_once(':')?;
    let host = authority.rsplit('@').next()?.to_ascii_lowercase();
    let (owner, name) = path_parts(path)?;
    Some(Location {
        scheme: "https".to_owned(),
        host,
        port: 443,
        owner,
        name,
    })
}

pub fn parse(value: &str) -> Option<Location> {
    let value = value.trim();
    if let Ok(url) = reqwest::Url::parse(value) {
        return from_url(&url);
    }
    from_scp(value).or_else(|| {
        let (owner, name) = path_parts(value)?;
        Some(Location {
            scheme: "https".to_owned(),
            host: "github.com".to_owned(),
            port: 443,
            owner,
            name,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn equivalent_references_share_identity_and_api() {
        let bare = parse("Owner/Repo").expect("bare");
        let dot_com = parse("https://github.com/owner/repo.git").expect("url");
        assert_eq!(
            (bare.identity(), dot_com.identity(), bare.api_url()),
            (
                "github.com:443/owner/repo".to_owned(),
                "github.com:443/owner/repo".to_owned(),
                "https://api.github.com".to_owned(),
            )
        );
    }

    #[test]
    fn enterprise_ports_remain_distinct() {
        let first = parse("https://ghe.example:8443/o/r").expect("first");
        let second = parse("https://ghe.example:9443/o/r").expect("second");
        assert_eq!(
            (first.identity(), second.identity(), first.api_url()),
            (
                "ghe.example:8443/o/r".to_owned(),
                "ghe.example:9443/o/r".to_owned(),
                "https://ghe.example:8443/api/v3".to_owned(),
            )
        );
    }
}
