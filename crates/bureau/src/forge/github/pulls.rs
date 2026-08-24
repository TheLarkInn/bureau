//! GitHub pull-request response shapes and their mapping onto [`Pr`].

use serde::Deserialize;

use super::Pr;

fn closes_item(body: Option<&str>, repo: &str) -> Option<String> {
    let body = body?.to_lowercase();
    let start = body.find("closes #")? + "closes #".len();
    let digits: String = body[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("{repo}#{digits}"))
}

/// The bare number in `owner/name#42`.
pub(super) fn issue_number(item_id: &str) -> &str {
    item_id.rsplit('#').next().unwrap_or(item_id)
}

#[derive(Deserialize)]
pub(super) struct Head {
    #[serde(rename = "ref")]
    branch: String,
}

#[derive(Deserialize)]
pub(super) struct Pull {
    number: u64,
    title: String,
    html_url: String,
    body: Option<String>,
    head: Head,
}

impl Pull {
    pub(super) fn into_pr(self, repo: &str) -> Pr {
        Pr {
            number: self.number,
            repo: repo.to_owned(),
            branch: self.head.branch,
            title: self.title,
            url: self.html_url,
            item_id: closes_item(self.body.as_deref(), repo),
        }
    }
}
