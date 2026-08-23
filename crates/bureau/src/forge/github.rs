//! GitHub REST forge; registry arguments accept URLs or bare `owner/name`.

mod dependencies;
mod identity;
mod labels;
mod pulls;
mod rate;
mod status;

use async_trait::async_trait;
use reqwest::{Method, RequestBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use self::pulls::{Pull, issue_number};
use super::{Error, Forge, Identity, Item, Pr, PrRequest, PrStatus};
use crate::contract::Trust;
use crate::process::Secret;

const MAX_PAGES: u32 = 10;

pub(super) use rate::response_error;

async fn json_body<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, Error> {
    if !resp.status().is_success() {
        return Err(response_error(resp).await?);
    }
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Parse(e.to_string()))
}

/// Checks the status of a call whose body carries no information.
async fn ensure_ok(resp: reqwest::Response) -> Result<(), Error> {
    if resp.status().is_success() {
        return Ok(());
    }
    Err(response_error(resp).await?)
}

/// The `rel="next"` URL from a `Link` header, when present.
fn next_page(resp: &reqwest::Response) -> Option<String> {
    let link = resp.headers().get("link")?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.trim()
            .eq_ignore_ascii_case("rel=\"next\"")
            .then(|| url.trim().trim_matches(|c| c == '<' || c == '>').to_owned())
    })
}

fn repo_name(source: &str) -> Result<String, Error> {
    crate::forge::repository::parse(source)
        .map(|location| location.name())
        .ok_or_else(|| {
            Error::Parse(format!(
                "expected a registry URL or owner/name, got {source:?}"
            ))
        })
}

fn trust(association: &str) -> Trust {
    match association {
        "OWNER" | "MEMBER" | "COLLABORATOR" => Trust::Maintainer,
        _ => Trust::Untrusted,
    }
}

fn split_item_id(item_id: &str) -> Result<(String, u64), Error> {
    let parsed = || {
        let (repo, number) = item_id.rsplit_once('#')?;
        Some((repo.to_owned(), number.parse().ok()?))
    };
    parsed().ok_or_else(|| Error::Parse(format!("expected owner/name#number, got {item_id:?}")))
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    repository_url: String,
    title: String,
    body: Option<String>,
    html_url: String,
    labels: Vec<Label>,
    author_association: String,
}

impl Issue {
    fn into_item(self, expected_repo: &str) -> Option<Item> {
        let repo = repo_name(&self.repository_url).ok()?;
        if !repo.eq_ignore_ascii_case(expected_repo) {
            return None;
        }
        Some(Item {
            external_id: format!("{repo}#{}", self.number),
            title: self.title,
            body: self.body.unwrap_or_default(),
            url: self.html_url,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            trust: trust(&self.author_association),
        })
    }
}

#[derive(Deserialize)]
struct SearchPage {
    items: Vec<Issue>,
}

pub struct GitHubForge {
    /// API root: `https://api.github.com`, or a local server in tests.
    pub base_url: String,
    token: Secret,
    client: reqwest::Client,
}

impl GitHubForge {
    /// A client against `https://api.github.com`.
    #[must_use]
    pub fn new(token: Secret) -> Self {
        Self {
            base_url: "https://api.github.com".to_owned(),
            token,
            client: reqwest::Client::new(),
        }
    }

    /// A client using the API root implied by a repository reference.
    ///
    /// # Errors
    /// Returns a parse error when `repo` is not a supported repository reference.
    pub fn for_repo(token: Secret, repo: &str) -> Result<Self, Error> {
        let location = crate::forge::repository::parse(repo)
            .ok_or_else(|| Error::Parse(format!("bad GitHub repository reference: {repo}")))?;
        Ok(Self::new(token).with_base_url(location.api_url()))
    }

    /// Points the client at another API root (tests, GitHub Enterprise).
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// A request carrying the headers every GitHub call needs, signed
    /// with one resolved credential.
    fn request_as(&self, method: Method, url: &str, token: &Secret) -> RequestBuilder {
        self.client
            .request(method, url)
            .bearer_auth(token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .header("User-Agent", "bureau")
    }

    /// The same request, signed with this client's own credential.
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.request_as(method, url, &self.token)
    }

    /// GETs `first`, following up to [`MAX_PAGES`] `rel="next"` links.
    async fn get_pages<T: Send, P: DeserializeOwned>(
        &self,
        first: RequestBuilder,
        into_items: impl Fn(P) -> Vec<T> + Send + Sync,
    ) -> Result<Vec<T>, Error> {
        let mut items = Vec::new();
        let mut next = Some(first);
        for _ in 0..MAX_PAGES {
            let Some(req) = next.take() else { break };
            let resp = req.send().await?;
            next = next_page(&resp).map(|url| self.request(Method::GET, &url));
            items.extend(into_items(json_body(resp).await?));
        }
        Ok(items)
    }
}

#[async_trait]
impl Forge for GitHubForge {
    async fn identity(&self, credential: &Secret) -> Result<Option<Identity>, Error> {
        identity::get(self, credential).await
    }

    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        let repo = repo_name(source)?;
        let q = format!("{filter} repo:{repo}");
        let url = format!("{}/search/issues", self.base_url);
        let first = self
            .request(Method::GET, &url)
            .query(&[("q", q.as_str()), ("per_page", "100")]);
        self.get_pages(first, |page: SearchPage| {
            page.items
                .into_iter()
                .filter_map(|issue| issue.into_item(&repo))
                .collect()
        })
        .await
    }

    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        let repo = repo_name(repo)?;
        let url = format!("{}/repos/{repo}/pulls", self.base_url);
        let first = self
            .request(Method::GET, &url)
            .query(&[("state", "open"), ("per_page", "100")]);
        let prs = self
            .get_pages(first, |page: Vec<Pull>| {
                page.into_iter().map(|pull| pull.into_pr(&repo)).collect()
            })
            .await?;
        Ok(prs
            .into_iter()
            .filter(|pr| pr.branch.starts_with(branch_prefix))
            .collect())
    }

    async fn create_pr(&self, req: &PrRequest) -> Result<Pr, Error> {
        let repo = repo_name(&req.repo)?;
        let body = req.item_id.as_ref().map_or_else(
            || req.body.clone(),
            |item_id| format!("{}\n\nCloses #{}", req.body, issue_number(item_id)),
        );
        let url = format!("{}/repos/{repo}/pulls", self.base_url);
        let payload = serde_json::json!({
            "title": &req.title,
            "head": &req.branch,
            "base": &req.base,
            "body": body,
        });
        let resp = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await?;
        Ok(json_body::<Pull>(resp).await?.into_pr(&repo))
    }

    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error> {
        status::get(self, repo, number).await
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        let (repo, number) = split_item_id(item_id)?;
        let url = format!("{}/repos/{repo}/issues/{number}/comments", self.base_url);
        let resp = self
            .request(Method::POST, &url)
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?;
        ensure_ok(resp).await
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        labels::set(self, item_id, labels).await
    }

    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        labels::update(self, item_id, add, remove).await
    }
}
